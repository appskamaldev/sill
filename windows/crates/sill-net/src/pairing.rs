//! A `sill-pair/1` connection: exactly one kind 19 (≤ 4096 bytes) within its deadline, one kind
//! 20, then close (upstream `Door.swift`, `RemoteAccess.judge`).

use crate::conn::{drain, message};
use crate::host::Host;
use sill_policy::door::Door;
use sill_wire::{decode, encode, FrameReader, Kind, Limits, PairRequest};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::{debug, info};

/// After kind 20, the device's own close is waited for this long at most (upstream 250 ms).
const AFTER_RESULT: Duration = Duration::from_millis(250);

pub async fn run<S: AsyncRead + AsyncWrite + Unpin>(
    host: &Host,
    door: Door,
    mut s: S,
    peer: SocketAddr,
    fp: [u8; 32],
    deadline: Instant,
) {
    let mut reader = FrameReader::new(Limits::HostPairing);
    let mut buf = [0u8; 4096];
    let read = async {
        loop {
            match reader.next_message() {
                Ok(Some(m)) => return Some(m),
                Ok(None) => {}
                Err(e) => {
                    info!("Pairing from {peer}: {e}; closed.");
                    return None;
                }
            }
            let n = match s.read(&mut buf).await {
                Ok(0) | Err(_) => return None,
                Ok(n) => n,
            };
            if let Err(e) = reader.push(&buf[..n]) {
                info!("Pairing from {peer}: {e}; closed.");
                return None;
            }
        }
    };
    let m = match tokio::time::timeout_at(deadline.into(), read).await {
        Ok(Some(m)) => m,
        Ok(None) => return,
        Err(_) => {
            info!("Pairing from {peer}: no request in time; closed.");
            return;
        }
    };
    if m.kind != Kind::PairRequest {
        info!("Pairing from {peer}: kind {} instead of a pairing request; closed.", m.kind.as_u8());
        return;
    }
    let Some(req) = decode::<PairRequest>(&m.payload) else {
        info!("Pairing from {peer}: a request that does not decode; closed.");
        return;
    };
    debug!("Pairing request from {peer}: method {}, v {}", req.method, req.v);
    let result = host.judge(door, &req, &fp, &peer.ip().to_string());
    let _ = s.write_all(&message(Kind::PairResult, encode(&result))).await;
    let _ = s.flush().await;
    let _ = s.shutdown().await;
    drain(&mut s, AFTER_RESULT).await;
}
