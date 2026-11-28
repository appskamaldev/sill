//! The catalog (Windows): the Apps drawer and the window list, from one place so a window and its
//! app share one icon.
//!
//! - Apps: Start's apps, scanned when the first device connects (not at launch: the shell and the
//!   icons cost about 50 MB, and an idle host should stay small), then every 15 minutes while a
//!   device is connected. Icons are made one by one in the
//!   background (about 70 ms each on Intel Arc) and announced as each is ready. A launch is checked
//!   against the list.
//! - Windows: read every 2 s (upstream reads only while a device is connected; a read here is
//!   about 1 ms, and a device that picks a window as it connects then finds it listed). The list
//!   is kept in first-seen order and sent only on a change; thumbnails only while a device is
//!   connected. A streamed window that left the list
//!   makes the source None. A launched app's first window is picked (upstream `pendingLaunch`).
//! - Locked (the input desktop is not the user's): nothing is read, sent or launched. At the unlock
//!   the list and the icons go again, to the devices that connected while it was locked.

use sill_catalog::windows_list::{self, Win};
use sill_net::apps::{AppEvent, AppSource, Icon};
use sill_net::windows::{WindowEvent, WindowSource};
use sill_net::Hub;
use sill_wire::{AppInfo, StreamSource, WindowAction, WindowCommand, WindowInfo};
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

const RESCAN: Duration = Duration::from_secs(15 * 60);
const POLL: Duration = Duration::from_secs(2);
const ICON_SIZE: i32 = 64;

#[derive(Default)]
struct Apps {
    apps: Vec<sill_catalog::App>,
    list: Arc<Vec<AppInfo>>,
    /// Lower-case full exe path → index in `apps`, for the desktop apps whose ID names a known folder.
    by_path: HashMap<String, usize>,
    icons: HashMap<String, Arc<[u8]>>,
}

#[derive(Default)]
struct Windows {
    /// First-seen order of HWNDs.
    order: Vec<isize>,
    list: Arc<Vec<WindowInfo>>,
    hwnd: HashMap<u32, isize>,
}

pub struct Catalog {
    apps: Mutex<Apps>,
    windows: Mutex<Windows>,
    app_tx: broadcast::Sender<AppEvent>,
    win_tx: broadcast::Sender<WindowEvent>,
    hub: Arc<Hub>,
    /// How many sessions are connected (the window poll runs only then).
    sessions: Box<dyn Fn() -> usize + Send + Sync>,
    /// The bundle ID of the app launched last, until its first window shows.
    pending_launch: Mutex<Option<String>>,
    /// Wakes the window poll at once (after a command).
    poke: Mutex<mpsc::Sender<()>>,
    thumbs: Mutex<crate::thumbs::Thumbs>,
}

fn locked() -> bool {
    !sill_input::win::user_desktop_active()
}

impl Catalog {
    pub fn start(hub: Arc<Hub>, sessions: impl Fn() -> usize + Send + Sync + 'static) -> Arc<Catalog> {
        let (poke, woken) = mpsc::channel();
        let me = Arc::new(Catalog {
            apps: Mutex::new(Apps::default()),
            windows: Mutex::new(Windows::default()),
            app_tx: broadcast::channel(1024).0,
            win_tx: broadcast::channel(256).0,
            hub,
            sessions: Box::new(sessions),
            pending_launch: Mutex::new(None),
            poke: Mutex::new(poke),
            thumbs: Mutex::new(crate::thumbs::Thumbs::default()),
        });
        let m = me.clone();
        let _ = std::thread::Builder::new().name("sill-apps".into()).spawn(move || {
            let mut last: Option<std::time::Instant> = None;
            loop {
                let due = last.is_none_or(|t| t.elapsed() >= RESCAN);
                if due && (m.sessions)() > 0 {
                    m.refresh_apps();
                    last = Some(std::time::Instant::now());
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        });
        let m = me.clone();
        let _ = std::thread::Builder::new().name("sill-windows".into()).spawn(move || {
            let mut was_locked = false;
            let mut first = true;
            loop {
                if !std::mem::replace(&mut first, false) {
                    let _ = woken.recv_timeout(POLL);
                }
                let now_locked = locked();
                if was_locked && !now_locked {
                    m.resend_after_unlock();
                }
                was_locked = now_locked;
                if now_locked {
                    continue;
                }
                // The list is read even with no device (about 1 ms), so a device that picks a
                // window the moment it connects finds it listed; thumbnails only for devices.
                m.refresh_windows();
                if (m.sessions)() > 0 {
                    m.take_thumbnails();
                }
            }
        });
        me
    }

    /// The HWND of a listed window.
    pub fn hwnd(&self, id: u32) -> Option<isize> {
        self.windows.lock().unwrap().hwnd.get(&id).copied()
    }

    fn refresh_apps(&self) {
        let apps = match sill_catalog::scan() {
            Ok(a) => a,
            Err(e) => {
                warn!("Apps: the Start apps could not be read ({e}).");
                return;
            }
        };
        let list: Arc<Vec<AppInfo>> =
            Arc::new(apps.iter().map(|a| AppInfo { name: a.name.clone(), bundle_id: a.id.clone() }).collect());
        let by_path: HashMap<String, usize> = apps
            .iter()
            .enumerate()
            .filter_map(|(i, a)| sill_catalog::app_path(&a.id).map(|p| (p.to_lowercase(), i)))
            .collect();
        let missing: Vec<String> = {
            let mut s = self.apps.lock().unwrap();
            if *s.list == *list {
                Vec::new()
            } else {
                s.apps = apps.clone();
                s.list = list.clone();
                s.by_path = by_path;
                let _ = self.app_tx.send(AppEvent::List(list.clone()));
                info!("Apps: {} in Start.", list.len());
                apps.iter().filter(|a| !s.icons.contains_key(&a.id)).map(|a| a.id.clone()).collect()
            }
        };
        for id in missing {
            self.make_icon(&id);
        }
    }

    fn make_icon(&self, id: &str) {
        match sill_catalog::icon_png(id, ICON_SIZE) {
            Ok(png) => {
                let png: Arc<[u8]> = Arc::from(png);
                self.apps.lock().unwrap().icons.insert(id.to_string(), png.clone());
                let _ = self.app_tx.send(AppEvent::Icon(id.to_string(), png));
            }
            Err(e) => debug!("Catalog: no icon for {id} ({e})."),
        }
    }

    fn refresh_windows(&self) {
        let found: Vec<Win> = windows_list::windows();
        let (infos, new_icons) = {
            let a = self.apps.lock().unwrap();
            let mut infos = Vec::with_capacity(found.len());
            let mut new_icons = Vec::new();
            for w in &found {
                let (bundle_id, app_name) = windows_list::app_of(w, &a.apps, &a.by_path);
                if !a.icons.contains_key(&bundle_id) && !new_icons.contains(&bundle_id) {
                    new_icons.push(bundle_id.clone());
                }
                infos.push((
                    w.hwnd,
                    WindowInfo {
                        id: w.id(),
                        title: w.title.clone(),
                        app_name,
                        bundle_id,
                        width: w.width,
                        height: w.height,
                    },
                ));
            }
            (infos, new_icons)
        };
        // Icons first, so a list that names a new app finds its icon already there.
        for id in new_icons {
            self.make_icon(&id);
        }
        let changed = {
            let mut s = self.windows.lock().unwrap();
            let present: Vec<isize> = infos.iter().map(|(h, _)| *h).collect();
            s.order.retain(|h| present.contains(h));
            for h in &present {
                if !s.order.contains(h) {
                    s.order.push(*h);
                }
            }
            let by_hwnd: HashMap<isize, &WindowInfo> = infos.iter().map(|(h, i)| (*h, i)).collect();
            let list: Vec<WindowInfo> = s.order.iter().filter_map(|h| by_hwnd.get(h).map(|i| (*i).clone())).collect();
            if *s.list != list {
                s.hwnd = infos.iter().map(|(h, i)| (i.id, *h)).collect();
                s.list = Arc::new(list);
                Some(s.list.clone())
            } else {
                None
            }
        };
        let Some(list) = changed else { return };
        let _ = self.win_tx.send(WindowEvent::List(list.clone()));
        // A streamed window that left the list (closed, minimized): nothing streams (upstream); the
        // device asks for the Desktop itself.
        if let StreamSource::Window(id) = self.hub.source() {
            if !list.iter().any(|w| w.id == id) {
                info!("Window {id} is gone: nothing streams.");
                self.hub.select(StreamSource::None);
            }
        }
        // A launched app's first window is the source (upstream `pendingLaunch`).
        let pending = self.pending_launch.lock().unwrap().clone();
        if let Some(bundle_id) = pending {
            if let Some(w) = list.iter().find(|w| w.bundle_id == bundle_id) {
                info!("Launched {} shows window {}: streaming it.", w.app_name, w.id);
                *self.pending_launch.lock().unwrap() = None;
                self.hub.select(StreamSource::Window(w.id));
            }
        }
    }

    fn take_thumbnails(&self) {
        let list = self.windows.lock().unwrap().list.clone();
        let active = match self.hub.source() {
            StreamSource::Window(id) => Some(id),
            _ => None,
        };
        let due = self.thumbs.lock().unwrap().due(&list, active, Instant::now());
        for w in due {
            let Some(h) = self.hwnd(w.id) else { continue };
            match crate::thumbs::thumbnail(h) {
                Some(jpeg) => {
                    let _ = self.win_tx.send(WindowEvent::Thumbnail(w.id, Arc::from(jpeg)));
                }
                None => debug!("Catalog: no thumbnail for window {}.", w.id),
            }
        }
    }

    fn resend_after_unlock(&self) {
        info!("Unlocked: the apps, icons and windows go again.");
        let (list, icons) = self.snapshot();
        for (id, png) in icons {
            let _ = self.app_tx.send(AppEvent::Icon(id, png));
        }
        let _ = self.app_tx.send(AppEvent::List(list));
        let wl = self.windows.lock().unwrap().list.clone();
        let _ = self.win_tx.send(WindowEvent::List(wl));
        self.thumbs.lock().unwrap().reset();
    }
}

impl AppSource for Catalog {
    fn snapshot(&self) -> (Arc<Vec<AppInfo>>, Vec<Icon>) {
        let s = self.apps.lock().unwrap();
        // The listed apps' icons in the list's order, then the window-only apps'.
        let mut icons: Vec<Icon> =
            s.list.iter().filter_map(|a| s.icons.get(&a.bundle_id).map(|p| (a.bundle_id.clone(), p.clone()))).collect();
        for (id, p) in &s.icons {
            if !s.list.iter().any(|a| &a.bundle_id == id) {
                icons.push((id.clone(), p.clone()));
            }
        }
        (s.list.clone(), icons)
    }

    fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.app_tx.subscribe()
    }

    fn launch(&self, bundle_id: &str) -> Result<(), String> {
        let listed = sill_catalog::may_launch(&self.apps.lock().unwrap().apps, bundle_id);
        if !listed {
            return Err("not an app this host listed".into());
        }
        sill_catalog::launch(bundle_id).map_err(|e| e.to_string())?;
        *self.pending_launch.lock().unwrap() = Some(bundle_id.to_string());
        let _ = self.poke.lock().unwrap().send(());
        Ok(())
    }

    fn locked(&self) -> bool {
        locked()
    }
}

impl WindowSource for Catalog {
    fn list(&self) -> Arc<Vec<WindowInfo>> {
        if locked() {
            return Arc::new(Vec::new());
        }
        self.windows.lock().unwrap().list.clone()
    }

    fn subscribe(&self) -> broadcast::Receiver<WindowEvent> {
        self.win_tx.subscribe()
    }

    fn command(&self, c: &WindowCommand) -> Result<(), String> {
        if locked() {
            return Err("the computer is locked".into());
        }
        let h = self.hwnd(c.id).ok_or("not a listed window")?;
        let r = match c.action {
            WindowAction::Close => sill_catalog::window_ops::close(h),
            WindowAction::Minimize => sill_catalog::window_ops::minimize(h),
            WindowAction::FullScreen => sill_catalog::window_ops::toggle_maximize(h),
        };
        // Show the result now, not at the next poll (upstream).
        let _ = self.poke.lock().unwrap().send(());
        r
    }

    fn resend_thumbnails(&self) {
        self.thumbs.lock().unwrap().reset();
        let _ = self.poke.lock().unwrap().send(());
    }
}
