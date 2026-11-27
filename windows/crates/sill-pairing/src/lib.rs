//! Sill pairing for the Windows host, ported from upstream `Pairing.swift`, `RemoteIdentity.swift`,
//! `Remote.swift` (kind 18's signature), `AddressParser.swift` and `SafeText.swift`, and checked
//! against golden vectors generated from them (`windows/vectors/`). Portable: no platform APIs.
#![forbid(unsafe_code)]

pub mod address;
pub mod base64url;
pub mod code;
pub mod link;
pub mod proof;
pub mod safe_text;
pub mod signed_info;
pub mod spki;
pub mod tag;

pub use address::{AddressFailure, AddressKind, ParsedAddress};
pub use code::CodeProblem;
pub use link::{LinkFailure, PairLink};
pub use spki::{mac_id, mac_id_is_well_formed, spki_der, spki_fingerprint};
pub use tag::Door;

/// `RemoteTLS.sessionALPN`: a session.
pub const SESSION_ALPN: &str = "sill/1";
/// `RemoteTLS.pairingALPN`: a pairing connection.
pub const PAIRING_ALPN: &str = "sill-pair/1";
/// What every host offers, for good.
pub const SERVER_ALPNS: [&str; 2] = [SESSION_ALPN, PAIRING_ALPN];
/// The client's SNI (ignored by the host).
pub const SERVER_NAME: &str = "sill";
