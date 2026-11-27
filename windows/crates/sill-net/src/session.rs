//! A `sill/1` session (upstream `StreamServer` + `StreamCoordinator`, Phase 1 subset): the device
//! gate, the catalog (window list then host settings), pings, ticks, stats, source and viewport,
//! keyframe requests, and the synthetic picture. Input is logged at debug level only.

use crate::conn::{drain, message};
use crate::host::{Host, SessionGuard};
use crate::video::{ClientVideo, Out};
use sill_pairing::spki;
use sill_policy::{device_gate, door::Door, viewport};
use sill_wire::{
    decode, encode, ClientStats, FrameReader, Goodbye, Hello, HostSettingsChange, HostSettingsState, InputEvent, Kind,
    LaunchApp, Limits, Message, StreamSource, Viewport, WindowList, PROTOCOL_CURRENT,
};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, WriteHalf};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Ticks go out this often while the picture streams, to a client sent nothing in that time.
pub const TICK: Duration = Duration::from_millis(30);
/// Kind 44 honoured at most this often per session.
pub const KEYFRAME_REQUEST_SPACING: Duration = Duration::from_secs(1);

/// The host settings this host reports (kind 16, PROTOCOL.md §10), answering `token` if given.
pub fn host_settings(host: &Host, answering: Option<i64>) -> HostSettingsState {
    crate::settings::state(&host.hub.video(), answering)
}

pub fn window_list(host: &Host) -> WindowList {
    let windows = host.windows.get().map(|w| (*w.list()).clone()).unwrap_or_default();
    let mut l = WindowList::new(host.config.name.clone(), windows, host.hub.source());
    l.launch_id = Some(host.launch_id.clone());
    l.host_version = Some(env!("CARGO_PKG_VERSION").to_string());
    l.protocol = Some(PROTOCOL_CURRENT);
    // Fork extension: the client can show Windows keys (PROTOCOL.md §12).
    l.host_platform = Some("windows".into());
    l
}

async fn writer<S: AsyncWrite>(
    mut w: WriteHalf<S>,
    mut rx: mpsc::UnboundedReceiver<Out>,
    in_flight: Arc<AtomicUsize>,
    stats: Arc<crate::stats::Stats>,
) {
    while let Some(o) = rx.recv().await {
        let r = match o {
            Out::Control(b) => w.write_all(&b).await,
            Out::Frame { header, au, captured } => {
                let r = match w.write_all(&header).await {
                    Ok(()) => w.write_all(&au).await,
                    e => e,
                };
                if let (Ok(()), Some(c)) = (&r, captured) {
                    // The last byte is in the TLS stream (written to the socket, or about to be at the
                    // flush below, which follows at once when nothing else waits).
                    stats.note_sent_latency(c.elapsed());
                }
                in_flight.fetch_sub(1, Ordering::SeqCst);
                r
            }
        };
        if r.is_err() {
            break;
        }
        if rx.is_empty() && w.flush().await.is_err() {
            break;
        }
    }
    // Our end: close_notify, then FIN.
    let _ = w.shutdown().await;
}

struct Sender {
    tx: mpsc::UnboundedSender<Out>,
    last_sent: Arc<AtomicU64>,
    host: Arc<Host>,
}

impl Sender {
    fn send(&self, bytes: Vec<u8>) {
        self.last_sent.store(self.host.now_us(), Ordering::Relaxed);
        let _ = self.tx.send(Out::Control(bytes));
    }
}

/// Why a session ended.
enum End {
    Closed,
    Goodbye(Goodbye),
}

pub async fn run<S: AsyncRead + AsyncWrite + Send + 'static>(
    host: Arc<Host>,
    door: Door,
    stream: S,
    peer: SocketAddr,
    fp: [u8; 32],
    _guard: Option<SessionGuard>,
) {
    let (mut r, w) = tokio::io::split(stream);
    let (tx, rx) = mpsc::unbounded_channel();
    let in_flight = Arc::new(AtomicUsize::new(0));
    let writer = tokio::spawn(writer(w, rx, in_flight.clone(), host.stats.clone()));
    let out = Sender { tx: tx.clone(), last_sent: Arc::new(AtomicU64::new(0)), host: host.clone() };
    let name =
        host.paired_name(&fp).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("key {}", spki::key_label(&fp)));
    let started = Instant::now();
    let mut reader = FrameReader::new(Limits::HostSession);
    let mut buf = vec![0u8; 64 * 1024];

    // The device gate. With the floor at "0" every device is admitted at once and nothing is read
    // ahead (upstream `StreamServer.gate`); above it, the first message must be a hello it admits.
    let mut early: Vec<Message> = Vec::new();
    if host.config.device_floor != sill_wire::SillVersion::ZERO {
        match first_message(&mut r, &mut reader, &mut buf).await {
            Some(m)
                if m.kind == Kind::Hello
                    && device_gate::admits(decode::<Hello>(&m.payload).as_ref(), &host.config.device_floor) =>
            {
                early.push(m);
            }
            m => {
                let hello = m.as_ref().filter(|m| m.kind == Kind::Hello).and_then(|m| decode::<Hello>(&m.payload));
                info!("{}", device_gate::refused_line(hello.as_ref(), &peer.to_string(), &host.config.device_floor));
                let g = device_gate::refusal(hello.as_ref(), &host.config.device_floor, &host.config.name);
                out.send(message(Kind::Goodbye, encode(&g)));
                drop(out);
                drop(tx);
                let _ = writer.await;
                drain(&mut r, host.config.close_wait).await;
                return;
            }
        }
    }

    info!("Client connected: {name} at {peer} ({} door).", crate::tls::door_word(door));
    // Catalog first (kinds 2 then 16), as upstream's `sendCatalog`.
    out.send(message(Kind::WindowList, encode(&window_list(&host))));
    out.send(message(Kind::HostSettings, encode(&host_settings(&host, None))));
    let video = Arc::new(ClientVideo::new(tx.clone(), in_flight, out.last_sent.clone()));
    // The Apps drawer after the catalog, as upstream's `sendCatalog`: icons, then the list.
    let mut window_events = host.windows.get().map(|w| {
        w.resend_thumbnails();
        w.subscribe()
    });
    let mut active_rx = host.hub.active.subscribe();
    active_rx.borrow_and_update();
    let mut app_events = match host.apps.get() {
        Some(src) => {
            if src.locked() {
                debug!("{name} at {peer}: no apps or icons (the computer is locked).");
            } else {
                let (list, icons) = src.snapshot();
                for (id, png) in &icons {
                    out.send(message(Kind::AppIcon, sill_wire::encode_icon(id, png)));
                }
                out.send(message(Kind::AppList, encode(&*list)));
                debug!("{name} at {peer}: {} apps, {} icons.", list.len(), icons.len());
            }
            Some(src.subscribe())
        }
        None => None,
    };
    let video_id = host.hub.join(video.clone());

    let mut shutdown = host.shutdown.subscribe();
    let mut removed = host.removed.subscribe();
    let mut settings = host.hub.settings.subscribe();
    settings.borrow_and_update();
    let mut ticks = tokio::time::interval(TICK);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sweep = tokio::time::interval(Duration::from_millis(500));
    let mut last_heard = Instant::now();
    let mut last_keyframe_request: Option<Instant> = None;
    let mut fps: Option<i64> = None;
    let mut s = SessionState {
        host: &host,
        out: &out,
        video: &video,
        peer,
        fp,
        name: &name,
        fps: &mut fps,
        last_keyframe_request: &mut last_keyframe_request,
        picked: None,
        session_id: video_id,
        literal_keys: false,
        settings_arrivals: Vec::new(),
        settings_rx: &mut settings,
    };
    for m in early {
        s.handle(m);
    }

    let end = loop {
        tokio::select! {
            n = r.read(&mut buf) => {
                let n = match n { Ok(0) | Err(_) => break End::Closed, Ok(n) => n };
                last_heard = Instant::now();
                if let Err(e) = reader.push(&buf[..n]) {
                    warn!("{name} at {peer}: {e}; closed.");
                    break End::Closed;
                }
                loop {
                    match reader.next_message() {
                        Ok(Some(m)) => s.handle(m),
                        Ok(None) => break,
                        Err(e) => {
                            warn!("{name} at {peer}: {e}; closed.");
                            break;
                        }
                    }
                }
                if reader.next_message().is_err() {
                    break End::Closed;
                }
            }
            _ = ticks.tick() => {
                let since = host.now_us().saturating_sub(out.last_sent.load(Ordering::Relaxed));
                if video.wants.load(Ordering::Relaxed) && since >= TICK.as_micros() as u64 {
                    out.send(message(Kind::Tick, Vec::new()));
                    host.stats.ticks.fetch_add(1, Ordering::Relaxed);
                }
            }
            _ = sweep.tick() => {
                if door == Door::Remote && last_heard.elapsed() >= host.config.remote_silence {
                    info!("{name} at {peer}: silent for {} s; dropped.", host.config.remote_silence.as_secs());
                    break End::Closed;
                }
            }
            ev = async {
                match app_events.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                use crate::apps::AppEvent;
                match ev {
                    Ok(e) if !host.apps.get().is_some_and(|a| a.locked()) => match e {
                        AppEvent::List(list) => out.send(message(Kind::AppList, encode(&*list))),
                        AppEvent::Icon(id, png) => out.send(message(Kind::AppIcon, sill_wire::encode_icon(&id, &png))),
                    },
                    Ok(_) => {}
                    // Fell behind (a burst of icons): the next list still comes.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => app_events = None,
                }
            }
            ev = async {
                match window_events.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                use crate::windows::WindowEvent;
                match ev {
                    Ok(WindowEvent::List(_)) => out.send(message(Kind::WindowList, encode(&window_list(&host)))),
                    Ok(WindowEvent::Thumbnail(id, jpeg)) => {
                        out.send(message(Kind::Thumbnail, sill_wire::encode_thumbnail(id, &jpeg)));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        out.send(message(Kind::WindowList, encode(&window_list(&host))));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => window_events = None,
                }
            }
            changed = active_rx.changed() => {
                // The source changed (this device's pick or another's): the list says it.
                if changed.is_ok() {
                    active_rx.borrow_and_update();
                    video.needs_keyframe.store(true, Ordering::Relaxed);
                    out.send(message(Kind::WindowList, encode(&window_list(&host))));
                }
            }
            changed = s.settings_rx.changed() => {
                // Another session changed the settings: every device hears the new state.
                if changed.is_ok() {
                    s.settings_rx.borrow_and_update();
                    out.send(message(Kind::HostSettings, encode(&host_settings(&host, None))));
                }
            }
            _ = shutdown.changed() => {
                break End::Goodbye(Goodbye { reconnect: Some(true), ..Goodbye::reason(Goodbye::QUIT) });
            }
            gone = removed.recv() => {
                if let Ok(gone) = gone {
                    if gone == fp {
                        info!("{name} at {peer}: no longer paired; goodbye removed.");
                        break End::Goodbye(Goodbye::reason(Goodbye::REMOVED));
                    }
                }
            }
        }
    };
    host.hub.leave(video_id);
    if let Some(sink) = host.input.get() {
        sink.left(video_id);
    }
    if let End::Goodbye(g) = &end {
        out.send(message(Kind::Goodbye, encode(g)));
    }
    drop(out);
    drop(video);
    drop(tx);
    let _ = writer.await;
    if matches!(end, End::Goodbye(_)) {
        drain(&mut r, host.config.close_wait).await;
    }
    info!("Client left: {name} at {peer} after {:.0} s.", started.elapsed().as_secs_f64());
}

async fn first_message<R: AsyncRead + Unpin>(r: &mut R, reader: &mut FrameReader, buf: &mut [u8]) -> Option<Message> {
    let read = async {
        loop {
            if let Ok(Some(m)) = reader.next_message() {
                return Some(m);
            }
            let n = r.read(buf).await.ok().filter(|n| *n > 0)?;
            reader.push(&buf[..n]).ok()?;
        }
    };
    tokio::time::timeout(Duration::from_secs_f64(device_gate::FIRST_MESSAGE_DEADLINE), read).await.ok().flatten()
}

struct SessionState<'a> {
    host: &'a Arc<Host>,
    out: &'a Sender,
    video: &'a Arc<ClientVideo>,
    peer: SocketAddr,
    fp: [u8; 32],
    name: &'a str,
    fps: &'a mut Option<i64>,
    last_keyframe_request: &'a mut Option<Instant>,
    /// This session's last pick: a device sends its own pick on the first window list and its
    /// switcher's, often in one read (Checkpoint 1: two lines 6 µs apart); a repeat is not logged.
    picked: Option<StreamSource>,
    /// The hub's id for this session, also its input state's.
    session_id: u64,
    /// Its hello said `pcKeys` ≥ 1: its keys are literal PC keys (fork extension).
    literal_keys: bool,
    /// When this session's last settings changes came (at most `settings::PER_SECOND` a second).
    settings_arrivals: Vec<Instant>,
    settings_rx: &'a mut tokio::sync::watch::Receiver<crate::settings::Video>,
}

impl SessionState<'_> {
    fn handle(&mut self, m: Message) {
        let (name, peer) = (self.name, self.peer);
        match m.kind {
            Kind::Ping => {
                // At once, its payload and timestamp unchanged.
                self.out.send(Message::new(Kind::Pong, m.timestamp, false, m.payload).serialized());
                self.host.stats.pongs.fetch_add(1, Ordering::Relaxed);
            }
            Kind::Hello => match decode::<Hello>(&m.payload) {
                Some(h) => {
                    info!("{}", device_gate::hello_line(&h, &peer.to_string()));
                    self.literal_keys = h.pc_keys.is_some_and(|g| g >= 1);
                    if self.literal_keys {
                        info!("{name} at {peer} sends literal PC keys (pcKeys {}).", h.pc_keys.unwrap_or(0));
                    }
                    self.host.note_app(&self.fp, device_gate::app_label(&h));
                }
                None => info!("{name} at {peer}: a hello that does not decode."),
            },
            Kind::ClientStats => {
                if let Some(s) = decode::<ClientStats>(&m.payload) {
                    info!(
                        "Device stats {}: fps {} frame age {} ms (max {}) rtt {} ms (max {})",
                        sill_pairing::safe_text::label64(&s.device),
                        s.fps,
                        s.frame_age_ms,
                        s.frame_age_max_ms.map_or("-".into(), |v| v.to_string()),
                        s.rtt_ms,
                        s.rtt_max_ms.map_or("-".into(), |v| v.to_string())
                    );
                }
            }
            Kind::SelectSource => match decode::<StreamSource>(&m.payload) {
                // One source for every device; the last pick wins (upstream).
                Some(src) if src == self.host.hub.source() && self.picked == Some(src) => {
                    debug!("{name} at {peer} picked {src:?} again; it streams already.");
                }
                Some(StreamSource::Desktop) => {
                    info!("{name} at {peer} picked the Desktop.");
                    self.picked = Some(StreamSource::Desktop);
                    self.video.needs_keyframe.store(true, Ordering::Relaxed);
                    self.video.wants.store(true, Ordering::Relaxed);
                    self.host.hub.select(StreamSource::Desktop);
                }
                Some(StreamSource::None) => {
                    info!("{name} at {peer} picked nothing.");
                    self.picked = Some(StreamSource::None);
                    self.host.hub.select(StreamSource::None);
                    if let Some(sink) = self.host.input.get() {
                        sink.left(self.session_id);
                    }
                }
                Some(StreamSource::Window(id)) => {
                    let listed = self.host.windows.get().is_some_and(|w| w.list().iter().any(|x| x.id == id));
                    self.picked = Some(StreamSource::Window(id));
                    if listed {
                        info!("{name} at {peer} picked window {id}.");
                        self.video.needs_keyframe.store(true, Ordering::Relaxed);
                        self.video.wants.store(true, Ordering::Relaxed);
                        self.host.hub.select(StreamSource::Window(id));
                    } else {
                        // Upstream: a window not on the list streams nothing, and the list says so.
                        info!("{name} at {peer} picked window {id}, which is not listed: nothing streams.");
                        self.host.hub.select(StreamSource::None);
                    }
                }
                None => debug!("{name} at {peer}: a selectSource that does not decode."),
            },
            Kind::Viewport => match decode::<Viewport>(&m.payload) {
                Some(v) => match viewport::refusal(&v) {
                    Some(why) => info!("Viewport from {peer} refused: {why}"),
                    None => {
                        *self.fps = v.fps;
                        self.video.fps.store(v.fps.unwrap_or(0).max(0), Ordering::Relaxed);
                        // With a text scale, a streamed window is fitted to the panel; without one
                        // (Aa Off), the fit ends. The last device's viewport wins (upstream).
                        let fit = v.scale.filter(|s| *s > 0.0).map(|s| (v.width, v.height, s));
                        self.host.hub.viewport.send_replace(fit);
                        info!(
                            "Viewport from {name}: {}×{} points, fps {} ({}).",
                            viewport::whole(v.width),
                            viewport::whole(v.height),
                            v.fps.map_or("-".into(), |f| f.to_string()),
                            self.host.hub.picture()
                        );
                    }
                },
                None => info!("Viewport from {peer} refused: it does not decode."),
            },
            Kind::KeyframeRequest => {
                let due = self.last_keyframe_request.is_none_or(|t| t.elapsed() >= KEYFRAME_REQUEST_SPACING);
                if due {
                    *self.last_keyframe_request = Some(Instant::now());
                    self.video.needs_keyframe.store(true, Ordering::Relaxed);
                    self.host.hub.request_keyframe();
                    debug!("{name} at {peer} asked for a keyframe.");
                }
            }
            Kind::ChangeSettings => {
                // Malformed JSON has no token to answer, so it gets nothing (upstream).
                let Some(change) = decode::<HostSettingsChange>(&m.payload) else {
                    debug!("{name} at {peer}: a changeSettings that does not decode.");
                    return;
                };
                self.settings_arrivals.retain(|t| t.elapsed() < Duration::from_secs(1));
                if self.settings_arrivals.len() >= crate::settings::PER_SECOND {
                    info!("Settings from {name} ignored: more than {} changes a second.", crate::settings::PER_SECOND);
                } else {
                    self.settings_arrivals.push(Instant::now());
                    let before = self.host.hub.video();
                    let (after, refused) = crate::settings::accept(&before, &change);
                    if !refused.is_empty() {
                        info!("Settings from {name} refused: {}", refused.join(", "));
                    }
                    if after != before {
                        info!("Settings from {name}: {}", crate::settings::changes(&before, &after));
                        self.host.hub.settings.send_replace(after);
                        // This session answers below; it need not hear its own change again.
                        self.settings_rx.borrow_and_update();
                    }
                }
                // Exactly one answer, to this device: the settings as they now stand.
                self.out.send(message(Kind::HostSettings, encode(&host_settings(self.host, change.token))));
            }
            Kind::WindowCommand => match (decode::<sill_wire::WindowCommand>(&m.payload), self.host.windows.get()) {
                (Some(c), Some(w)) => match w.command(&c) {
                    Ok(()) => info!("{name} at {peer}: window {} {:?}.", c.id, c.action),
                    Err(e) => info!("{name} at {peer}: window {} {:?} refused: {e}", c.id, c.action),
                },
                _ => debug!("{name} at {peer}: a windowCommand this host cannot take."),
            },
            Kind::LaunchApp => match (decode::<LaunchApp>(&m.payload), self.host.apps.get()) {
                (Some(l), Some(src)) if !src.locked() => match src.launch(&l.bundle_id) {
                    Ok(()) => info!("{name} at {peer} launched {}.", sill_pairing::safe_text::label64(&l.bundle_id)),
                    Err(e) => info!("{name} at {peer}: launch refused: {e}"),
                },
                (Some(_), Some(_)) => info!("{name} at {peer}: launch refused while the computer is locked."),
                _ => debug!("{name} at {peer}: a launchApp this host cannot take."),
            },
            Kind::Input => match decode::<InputEvent>(&m.payload) {
                Some(e) => match self.host.input.get() {
                    // Only to the picture this device sees.
                    Some(sink) if self.video.wants.load(Ordering::Relaxed) => {
                        debug!("Input from {name}: {e:?}");
                        sink.event(self.session_id, self.literal_keys, &e);
                    }
                    _ => debug!("Input from {name} (not injected): {e:?}"),
                },
                None => debug!("Input from {name}: does not decode."),
            },
            k => debug!("{name} at {peer}: kind {} ({}) skipped.", k.as_u8(), k.name()),
        }
    }
}
