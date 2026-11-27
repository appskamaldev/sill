//! The Apps drawer's source (Windows): Start's apps scanned on a thread of their own at launch and
//! every 15 minutes, their icons made one by one in the background (about 70 ms each on Intel
//! Arc, so 236 apps take about 17 s) and announced as each is ready, and launches checked against
//! the list.

use sill_net::apps::{AppEvent, AppSource, Icon};
use sill_wire::AppInfo;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{info, warn};

const RESCAN: Duration = Duration::from_secs(15 * 60);
const ICON_SIZE: i32 = 64;

struct State {
    apps: Vec<sill_catalog::App>,
    list: Arc<Vec<AppInfo>>,
    icons: HashMap<String, Arc<[u8]>>,
}

pub struct StartApps {
    state: Mutex<State>,
    tx: broadcast::Sender<AppEvent>,
}

impl StartApps {
    /// Starts scanning at once; the list is empty until the first scan ends (about 1 s).
    pub fn start() -> Arc<StartApps> {
        let me = Arc::new(StartApps {
            state: Mutex::new(State { apps: Vec::new(), list: Arc::new(Vec::new()), icons: HashMap::new() }),
            tx: broadcast::channel(512).0,
        });
        let m = me.clone();
        let _ = std::thread::Builder::new().name("sill-apps".into()).spawn(move || loop {
            m.refresh();
            std::thread::sleep(RESCAN);
        });
        me
    }

    fn refresh(&self) {
        let apps = match sill_catalog::scan() {
            Ok(a) => a,
            Err(e) => {
                warn!("Apps: the Start apps could not be read ({e}).");
                return;
            }
        };
        let list: Arc<Vec<AppInfo>> =
            Arc::new(apps.iter().map(|a| AppInfo { name: a.name.clone(), bundle_id: a.id.clone() }).collect());
        let missing: Vec<String> = {
            let mut s = self.state.lock().unwrap();
            if *s.list == *list {
                Vec::new()
            } else {
                s.apps = apps.clone();
                s.list = list.clone();
                let _ = self.tx.send(AppEvent::List(list.clone()));
                info!("Apps: {} in Start.", list.len());
                apps.iter().filter(|a| !s.icons.contains_key(&a.id)).map(|a| a.id.clone()).collect()
            }
        };
        for id in missing {
            match sill_catalog::icon_png(&id, ICON_SIZE) {
                Ok(png) => {
                    let png: Arc<[u8]> = Arc::from(png);
                    self.state.lock().unwrap().icons.insert(id.clone(), png.clone());
                    let _ = self.tx.send(AppEvent::Icon(id, png));
                }
                Err(e) => tracing::debug!("Apps: no icon for {id} ({e})."),
            }
        }
    }
}

impl AppSource for StartApps {
    fn snapshot(&self) -> (Arc<Vec<AppInfo>>, Vec<Icon>) {
        let s = self.state.lock().unwrap();
        // Icons of listed apps only, in the list's order.
        let icons =
            s.list.iter().filter_map(|a| s.icons.get(&a.bundle_id).map(|p| (a.bundle_id.clone(), p.clone()))).collect();
        (s.list.clone(), icons)
    }

    fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.tx.subscribe()
    }

    fn launch(&self, bundle_id: &str) -> Result<(), String> {
        let listed = sill_catalog::may_launch(&self.state.lock().unwrap().apps, bundle_id);
        if !listed {
            return Err("not an app this host listed".into());
        }
        sill_catalog::launch(bundle_id).map_err(|e| e.to_string())
    }

    fn locked(&self) -> bool {
        !sill_input::win::user_desktop_active()
    }
}
