//! One accepted connection: the TLS handshake under the door's deadline, `.ready`'s decision, then
//! a pairing exchange or a session.

use crate::host::Host;
use crate::tls::{self, PinVerifier};
use crate::{pairing, session, wall_clock};
use rustls::server::Acceptor;
use sill_policy::door::{self, AtReady, Door};
use sill_wire::{encode, Goodbye, Kind, Message};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::LazyConfigAcceptor;
use tracing::{debug, info};

/// No Nagle; keepalive 5 s idle, 2 s apart, 3 probes (upstream `StreamServer.swift:651-658`).
pub fn tcp_options(tcp: &TcpStream) {
    let _ = tcp.set_nodelay(true);
    let ka = socket2::TcpKeepalive::new()
        .with_time(Duration::from_secs(5))
        .with_interval(Duration::from_secs(2))
        .with_retries(3);
    let _ = socket2::SockRef::from(tcp).set_tcp_keepalive(&ka);
}

/// Reads and drops whatever the peer still sends, until it closes or `limit` passes, so closing
/// does not answer its unread bytes with a reset that could overtake our last message.
pub async fn drain<R: AsyncRead + Unpin>(r: &mut R, limit: Duration) {
    let mut buf = [0u8; 16 * 1024];
    let _ = tokio::time::timeout(limit, async {
        loop {
            match r.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    })
    .await;
}

pub fn message(kind: Kind, payload: Vec<u8>) -> Vec<u8> {
    Message::new(kind, wall_clock(), false, payload).serialized()
}

/// The goodbye, our end (close_notify, FIN), then the device's bytes drained for `wait`.
pub async fn close_with_goodbye<S: AsyncRead + AsyncWrite + Unpin>(mut s: S, goodbye: &Goodbye, wait: Duration) {
    let _ = s.write_all(&message(Kind::Goodbye, encode(goodbye))).await;
    let _ = s.flush().await;
    let _ = s.shutdown().await;
    drain(&mut s, wait).await;
}

pub async fn handle(host: Arc<Host>, door: Door, tcp: TcpStream, peer: SocketAddr) {
    tcp_options(&tcp);
    let accepted = Instant::now();
    let known = host.is_known(peer.ip());
    let limit = Duration::from_secs_f64(door::deadline(known, None));
    let where_ = format!("{peer} ({} door)", tls::door_word(door));
    let handshake = async {
        let start = LazyConfigAcceptor::new(Acceptor::default(), tcp).await?;
        let alpn = tls::chosen_alpn(start.client_hello().alpn());
        let verifier = Arc::new(PinVerifier::new(host.clone(), door, alpn, peer.to_string()));
        let config = tls::server_config(&host, verifier.clone()).map_err(std::io::Error::other)?;
        match start.into_stream(config).into_fallible().await {
            Ok(s) => Ok(Ok((s, verifier.fingerprint()))),
            // The alert is written; take the socket back to close it without a reset.
            Err((e, tcp)) => Ok::<_, std::io::Error>(Err((e, tcp))),
        }
    };
    let (stream, fp) = match tokio::time::timeout(limit, handshake).await {
        Ok(Ok(Ok(x))) => x,
        Ok(Ok(Err((e, mut tcp)))) => {
            debug!("TLS handshake with {where_} failed: {e}");
            let _ = tcp.shutdown().await;
            drain(&mut tcp, host.config.close_wait).await;
            return;
        }
        Ok(Err(e)) => {
            debug!("TLS handshake with {where_} failed: {e}");
            return;
        }
        Err(_) => {
            debug!("TLS handshake with {where_} timed out after {:.0} s", limit.as_secs_f64());
            return;
        }
    };
    let ready_after = accepted.elapsed().as_secs_f64();
    let alpn = stream.get_ref().1.alpn_protocol().and_then(|a| std::str::from_utf8(a).ok()).map(str::to_owned);
    let (verdict, guard) = host.admit(door, alpn.as_deref(), fp.as_ref().map(|f| &f[..]), peer.ip());
    match verdict {
        AtReady::Session => {
            let fp = fp.expect("a session has a key");
            session::run(host.clone(), door, stream, peer, fp, guard).await;
        }
        AtReady::Pairing => {
            let fp = fp.expect("a pairing connection has a key");
            let deadline = accepted + Duration::from_secs_f64(door::deadline(known, Some(ready_after)));
            pairing::run(&host, door, stream, peer, fp, deadline).await;
        }
        AtReady::Goodbye(reason) => {
            info!("Goodbye {reason} to {where_} (not admitted).");
            close_with_goodbye(stream, &Goodbye::reason(reason), host.config.close_wait).await;
        }
        AtReady::Refuse(word) => {
            info!("Refused {where_} at ready: {word}.");
        }
    }
}
