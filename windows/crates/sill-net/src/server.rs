//! The doors: listeners on each allowed address (remote door on its fixed port, home door on one
//! dynamic port), Bonjour for the home door, the synthetic stream and the once-a-second chores.

use crate::bind::{self, AddrClass};
use crate::conn;
use crate::host::Host;
use sill_pairing::tag;
use sill_policy::door::Door;
use std::collections::HashSet;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpSocket};
use tracing::{info, warn};

pub struct Running {
    pub home_port: u16,
    pub remote_port: u16,
    pub host: Arc<Host>,
    bound: Arc<Mutex<Vec<IpAddr>>>,
    bonjour: Arc<Mutex<Option<sill_platform::bonjour::Registration>>>,
}

impl Running {
    pub fn addresses(&self) -> Vec<IpAddr> {
        self.bound.lock().unwrap().clone()
    }

    /// Goodbye `quit` to every session, waits (≤ 2 s) for them to drain, then withdraws the
    /// Bonjour registration before the process ends, so a device's browser sees the row go and
    /// takes the next registration as the host coming back (`DiscoveryPolicy.reconnectRow`).
    pub async fn quit(&self) {
        self.host.quit();
        for _ in 0..40 {
            if self.host.sessions() == (0, 0) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        // The condition's temporaries (the registration, the guard) are dropped before the block.
        if self.bonjour.lock().unwrap().take().is_some() {
            // Windows sends the withdrawal off this thread: a moment for it before the process ends.
            tokio::time::sleep(Duration::from_millis(250)).await;
            info!("Bonjour: withdrawn.");
        }
    }
}

fn listen(ip: IpAddr, port: u16) -> io::Result<TcpListener> {
    let sock = if ip.is_ipv4() { TcpSocket::new_v4()? } else { TcpSocket::new_v6()? };
    // Windows' SO_REUSEADDR lets another process take the port: only elsewhere.
    #[cfg(unix)]
    sock.set_reuseaddr(true)?;
    if ip.is_ipv6() {
        socket2::SockRef::from(&sock).set_only_v6(true)?;
    }
    sock.bind(SocketAddr::new(ip, port))?;
    sock.listen(128)
}

fn spawn_accept(host: Arc<Host>, door: Door, l: TcpListener) {
    let mut stop = host.shutdown.subscribe();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                a = l.accept() => match a {
                    Ok((tcp, peer)) => { tokio::spawn(conn::handle(host.clone(), door, tcp, peer)); }
                    Err(e) => { warn!("accept: {e}"); tokio::time::sleep(Duration::from_millis(100)).await; }
                },
                _ = stop.changed() => return,
            }
        }
    });
}

/// Binds both doors on `ip`; whether either bound.
fn bind_both(host: &Arc<Host>, ip: IpAddr, home_port: u16, remote_port: u16) -> bool {
    let mut any = false;
    for (door, port) in [(Door::Remote, remote_port), (Door::Home, home_port)] {
        match listen(ip, port) {
            Ok(l) => {
                spawn_accept(host.clone(), door, l);
                any = true;
            }
            Err(e) => warn!(
                "Could not listen on {} for the {} door: {e}",
                SocketAddr::new(ip, port),
                crate::tls::door_word(door)
            ),
        }
    }
    any
}

/// Starts every listener and background task. `remote_port` or `home_port` 0: a free port.
pub async fn start(host: Arc<Host>) -> io::Result<Running> {
    let c = &host.config;
    let explicit = !c.bind.is_empty();
    let wanted = if explicit { c.bind.clone() } else { bind::default_addresses(c.allow_internet) };
    for ip in &wanted {
        if !bind::allowed(*ip, c.allow_internet) {
            let why = if bind::classify(*ip) == AddrClass::Public {
                "a public address (needs --allow-internet)"
            } else {
                "not a usable address"
            };
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("--bind {ip}: {why}")));
        }
    }
    // The first address fixes both ports (a free one when 0); the rest reuse them.
    let first = wanted[0];
    let remote = listen(first, c.remote_port)?;
    let remote_port = remote.local_addr()?.port();
    let home = listen(first, c.home_port)?;
    let home_port = home.local_addr()?.port();
    spawn_accept(host.clone(), Door::Remote, remote);
    spawn_accept(host.clone(), Door::Home, home);
    let mut bound = vec![first];
    for ip in wanted.iter().skip(1) {
        if bind_both(&host, *ip, home_port, remote_port) {
            bound.push(*ip);
        }
    }
    for ip in &bound {
        info!(
            "Listening on {} ({:?}): remote door {}, home door {}",
            ip,
            bind::classify(*ip),
            SocketAddr::new(*ip, remote_port),
            SocketAddr::new(*ip, home_port)
        );
    }
    host.set_bound(bound.clone(), remote_port);
    let bound = Arc::new(Mutex::new(bound));

    // New addresses (Tailscale coming up after launch, a new Wi-Fi) are picked up every 10 s.
    if !explicit {
        let (h, b) = (host.clone(), bound.clone());
        let mut stop = host.shutdown.subscribe();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_secs(10));
            loop {
                tokio::select! { _ = every.tick() => {}, _ = stop.changed() => return }
                let have: HashSet<IpAddr> = b.lock().unwrap().iter().copied().collect();
                for ip in bind::default_addresses(h.config.allow_internet) {
                    if !have.contains(&ip) && bind_both(&h, ip, home_port, remote_port) {
                        info!("Listening on {ip} too ({:?}).", bind::classify(ip));
                        b.lock().unwrap().push(ip);
                        h.set_bound(b.lock().unwrap().clone(), remote_port);
                    }
                }
            }
        });
    }

    let bonjour = Arc::new(Mutex::new(None));
    advertise(&host, home_port, &bonjour);

    if let Some(clip) = host.clip.clone() {
        let hub = host.hub.clone();
        let h = host.clone();
        tokio::spawn(hub.run_clip(clip, host.shutdown.subscribe(), move || h.now_us()));
    }
    tokio::spawn(chores(host.clone()));
    Ok(Running { home_port, remote_port, host, bound, bonjour })
}

/// Registers the home door; the registration lives until `Running::quit` drops it (Windows
/// deregisters on drop). `r` is a fresh tag from the stored recognition key, so a device that
/// paired before a restart still recognizes this host.
fn advertise(host: &Arc<Host>, home_port: u16, slot: &Arc<Mutex<Option<sill_platform::bonjour::Registration>>>) {
    if !host.config.bonjour {
        return;
    }
    if !sill_platform::bonjour::SUPPORTED {
        info!("Bonjour: not advertised (this is not a Windows build); type the address on the device.");
        return;
    }
    let p = tag::door_value(host.require_pairing());
    let Some(r) = tag::make_tag(&host.identity.recognition_key, sill_store::random) else {
        warn!("Bonjour: no tag (random generator failed); not advertised.");
        return;
    };
    match sill_platform::bonjour::register(
        &host.config.name,
        home_port,
        &[(tag::DOOR_TXT_KEY, p), (tag::TAG_TXT_KEY, &r)],
    ) {
        Ok(reg) => {
            info!(
                "Bonjour: advertising \"{}\" as {} on port {home_port} (p={p}).",
                host.config.name,
                sill_platform::bonjour::SERVICE_TYPE
            );
            *slot.lock().unwrap() = Some(reg);
            let slot = slot.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let Some(status) = slot.lock().unwrap().as_ref().map(|r| r.status()) else { return };
                match status {
                    sill_platform::bonjour::Status::Registered => info!("Bonjour: registered."),
                    sill_platform::bonjour::Status::Pending => {
                        warn!("Bonjour: no answer from DnsServiceRegister after 3 s.")
                    }
                    sill_platform::bonjour::Status::Failed(e) => warn!("Bonjour: registration failed (error {e})."),
                }
            });
        }
        Err(e) => warn!("Bonjour: {e}"),
    }
}

/// Once a second: the paired list read again if another process changed it, the pairing window's
/// end, and the stats line while any device is connected.
async fn chores(host: Arc<Host>) {
    let mut stop = host.shutdown.subscribe();
    let mut every = tokio::time::interval(Duration::from_secs(1));
    let mut window_was_open = host.pairing_open();
    loop {
        tokio::select! { _ = every.tick() => {}, _ = stop.changed() => return }
        host.reload_paired_if_changed();
        let open = host.pairing_open();
        if window_was_open && !open {
            info!("Pairing window closed.");
        }
        window_was_open = open;
        let s = host.stats.take();
        let (home, remote) = host.sessions();
        if home + remote > 0 {
            info!(
                "stats: sessions {} (home {home}, remote {remote}) | frames sent {} dropped {} waitKey {} keyframes {} | {} kbit/s | pongs {} ticks {} refused {}{}",
                home + remote,
                s.sent,
                s.dropped,
                s.wait_key,
                s.keyframes,
                s.bytes * 8 / 1000,
                s.pongs,
                s.ticks,
                s.refused,
                s.media_words()
            );
        }
    }
}
