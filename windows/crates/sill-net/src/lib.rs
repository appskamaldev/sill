//! The Windows host's network side (Phase 1): TLS 1.3 doors with pin-only trust, pairing on
//! `sill-pair/1`, sessions on `sill/1` and a synthetic test picture. Portable (tokio + rustls);
//! the Windows-only pieces (Bonjour, DPAPI) live in `sill-platform`.

pub mod apps;
pub mod bind;
pub mod cert;
pub mod conn;
pub mod control;
pub mod host;
pub mod input;
pub mod pairing;
pub mod server;
pub mod session;
pub mod settings;
pub mod stats;
pub mod tls;
pub mod video;
pub mod windows;

pub use host::{Config, Host, PairingOffer};
pub use server::{start, Running};
pub use video::{ClientVideo, Clip, Encoded, Hub};

/// Seconds since 1970: the timestamp in every header the host writes.
pub fn wall_clock() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}
