//! A `sill/1` session (upstream `StreamServer` + `StreamCoordinator`, Phase 1 subset): the device
//! gate, the catalog (window list then host settings), pings, ticks, stats, source and viewport,
//! keyframe requests, and the synthetic picture. Input is logged at debug level only.

use crate::conn::{drain, message};
use crate::host::{Host, SessionGuard};
use crate::video::{ClientVideo, Out};
use sill_pairing::spki;
use sill_policy::{device_gate, door::Door, viewport};
use sill_wire::{
    decode, encode, ClientStats, FrameReader, Goodbye, Hello, HostSettingsState, InputEvent, Kind, Limits, Message,
    StreamSettings, StreamSource, Viewport, WindowList, PROTOCOL_CURRENT,
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

/// The host settings this host reports (kind 16, PROTOCOL.md §10): fixed values, nothing persistent.
pub fn host_settings() -> HostSettingsState {
    HostSettingsState {
        settings: StreamSettings {
            max_fps: 60,
            bitrate: 15_000_000,
            capture_scale: 1.0,
            prioritize_speed: true,
            virtual_display: false,
            direct_wireless: None,
            send_audio: None,
        },
        persistent: false,
        virtual_display_available: false,
        virtual_display_note: None,
        software_encoder: false,
        stream: None,
        answering: None,
        away: None,
        link: None,
        audio_note: None,
        session_paired: None,
    }
}

pub fn window_list(host: &Host) -> WindowList {
    let mut l = WindowList::new(host.config.name.clone(), Vec::new(), StreamSource::Desktop);
    l.launch_id = Some(host.launch_id.clone());
    l.host_version = Some(env!("CARGO_PKG_VERSION").to_string());
    l.protocol = Some(PROTOCOL_CURRENT);
    l
}

async fn writer<S: AsyncWrite>(mut w: WriteHalf<S>, mut rx: mpsc::UnboundedReceiver<Out>, in_flight: Arc<AtomicUsize>) {
    while let Some(o) = rx.recv().await {
        let r = match o {
            Out::Control(b) => w.write_all(&b).await,
            Out::Frame { header, au } => {
                let r = match w.write_all(&header).await {
                    Ok(()) => w.write_all(&au).await,
                    e => e,
                };
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
    let writer = tokio::spawn(writer(w, rx, in_flight.clone()));
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
    out.send(message(Kind::HostSettings, encode(&host_settings())));
    let video = Arc::new(ClientVideo::new(tx.clone(), in_flight, out.last_sent.clone()));
    let video_id = host.hub.join(video.clone());

    let mut shutdown = host.shutdown.subscribe();
    let mut removed = host.removed.subscribe();
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
        picked_desktop: false,
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
    /// The Desktop was picked and not unpicked since: a device sends its own pick on the first
    /// window list and its switcher's, often in one read (Checkpoint 1: two lines 6 µs apart).
    picked_desktop: bool,
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
                Some(StreamSource::Desktop) if self.picked_desktop => {
                    debug!("{name} at {peer} picked the Desktop again; it streams already.");
                }
                Some(StreamSource::Desktop) => {
                    info!("{name} at {peer} picked the Desktop.");
                    self.picked_desktop = true;
                    self.video.needs_keyframe.store(true, Ordering::Relaxed);
                    self.video.wants.store(true, Ordering::Relaxed);
                    self.host.hub.request_keyframe();
                }
                Some(StreamSource::None) => {
                    info!("{name} at {peer} picked nothing.");
                    self.picked_desktop = false;
                    self.video.wants.store(false, Ordering::Relaxed);
                }
                Some(StreamSource::Window(id)) => {
                    info!("{name} at {peer} picked window {id}; this host lists none, the Desktop stays.")
                }
                None => debug!("{name} at {peer}: a selectSource that does not decode."),
            },
            Kind::Viewport => match decode::<Viewport>(&m.payload) {
                Some(v) => match viewport::refusal(&v) {
                    Some(why) => info!("Viewport from {peer} refused: {why}"),
                    None => {
                        *self.fps = v.fps;
                        self.video.fps.store(v.fps.unwrap_or(0).max(0), Ordering::Relaxed);
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
            Kind::Input => match decode::<InputEvent>(&m.payload) {
                Some(e) => debug!("Input from {name} (not injected in Phase 1): {e:?}"),
                None => debug!("Input from {name}: does not decode."),
            },
            k => debug!("{name} at {peer}: kind {} ({}) skipped.", k.as_u8(), k.name()),
        }
    }
}
