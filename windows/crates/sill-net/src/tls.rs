//! TLS 1.3 for both doors (upstream `RemoteTLS.swift`): the host's self-signed P-256 certificate,
//! a client certificate required, ALPNs `sill/1` and `sill-pair/1`, no tickets, no resumption, and
//! trust decided by the key's pin alone, inside the handshake, for the ALPN the ClientHello asked
//! for (`DoorPolicy.trusts`).
//!
//! A key the door does not trust is refused with the TLS alert `certificate_unknown` (46), the
//! alert the Mac host sends (Network.framework's verify block answering false), which Apple's
//! client stack reports as -9825: the device then marks the host "Not paired" and stops redialing
//! (`iOSClient/RemoteDialPolicy.swift`, `classify`). In TLS 1.3 the device has sent its Finished by
//! then, so it sees the connection as ready and its first read fails with the alert, exactly as
//! with the Mac host.
//!
//! Never `bad_certificate` (42): Apple's client reports that one as -9808, which the device reads
//! as "Something answered …, but it isn't Sill" and keeps redialing (Checkpoint 1, 2026-11-27, R1;
//! measured with `windows/tools/tlsprobe` against both hosts).

use crate::host::Host;
use rustls::client::danger::HandshakeSignatureValid;
use rustls::crypto::{verify_tls13_signature_with_raw_key, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, SubjectPublicKeyInfoDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::server::{NoServerSessionStorage, ServerConfig};
use rustls::{CertificateError, DigitallySignedStruct, DistinguishedName, Error, SignatureScheme};
use sill_pairing::{spki, SERVER_ALPNS};
use sill_policy::door::Door;
use std::sync::{Arc, Mutex};
use tracing::info;

/// The ALPN rustls will choose for this ClientHello: the first of the host's that the device
/// offered (a device offers exactly one).
pub fn chosen_alpn<'a>(offered: Option<impl Iterator<Item = &'a [u8]>>) -> Option<&'static str> {
    let offered: Vec<&[u8]> = offered?.collect();
    SERVER_ALPNS.iter().copied().find(|a| offered.contains(&a.as_bytes()))
}

fn algorithms() -> WebPkiSupportedAlgorithms {
    rustls::crypto::ring::default_provider().signature_verification_algorithms
}

/// One connection's verifier: the door, the ALPN, and the host's trust at the moment the device's
/// certificate arrives. Remembers the key's fingerprint for the session.
#[derive(Debug)]
pub struct PinVerifier {
    host_door: Door,
    alpn: Option<&'static str>,
    host: Arc<HostRef>,
    fingerprint: Mutex<Option<[u8; 32]>>,
    peer: String,
}

/// `Host` without Debug noise.
pub struct HostRef(pub Arc<Host>);

impl std::fmt::Debug for HostRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Host")
    }
}

impl PinVerifier {
    pub fn new(host: Arc<Host>, door: Door, alpn: Option<&'static str>, peer: String) -> PinVerifier {
        PinVerifier { host_door: door, alpn, host: Arc::new(HostRef(host)), fingerprint: Mutex::new(None), peer }
    }

    /// The device key's fingerprint, once its certificate was read.
    pub fn fingerprint(&self) -> Option<[u8; 32]> {
        *self.fingerprint.lock().unwrap()
    }
}

/// The pin did not match. Only its alert matters to the device.
#[derive(Debug)]
struct NotTrusted;

impl std::fmt::Display for NotTrusted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("key not trusted")
    }
}

impl std::error::Error for NotTrusted {}

/// `CertificateError::Other` is the variant rustls sends as `certificate_unknown` (46), the alert
/// the Mac host sends and the one Apple's client turns into -9825 (see the module comment).
fn refuse() -> Error {
    Error::InvalidCertificate(CertificateError::Other(rustls::OtherError(Arc::new(NotTrusted))))
}

impl ClientCertVerifier for PinVerifier {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        let host = &self.host.0;
        let fp = crate::cert::p256_point(end_entity).map(|p| spki::spki_fingerprint(&p));
        *self.fingerprint.lock().unwrap() = fp;
        if host.trusts(self.host_door, self.alpn, fp.as_ref().map(|f| &f[..])) {
            return Ok(ClientCertVerified::assertion());
        }
        host.stats.refused.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let key =
            fp.map(|f| format!("key {}…", spki::short_name(&f))).unwrap_or_else(|| "a key that is not P-256".into());
        info!(
            "Refused {} at the {} door: {key} not trusted for {} (TLS alert certificate_unknown).",
            self.peer,
            door_word(self.host_door),
            self.alpn.unwrap_or("no ALPN")
        );
        Err(refuse())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::PeerIncompatible(rustls::PeerIncompatible::Tls12NotOffered))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        let point = crate::cert::p256_point(cert).ok_or_else(refuse)?;
        let spki_der = spki::spki_der(&point);
        verify_tls13_signature_with_raw_key(
            message,
            &SubjectPublicKeyInfoDer::from(spki_der.as_slice()),
            dss,
            &algorithms(),
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ECDSA_NISTP256_SHA256]
    }
}

pub fn door_word(d: Door) -> &'static str {
    match d {
        Door::Home => "home",
        Door::Remote => "remote",
    }
}

/// The server configuration for one connection.
pub fn server_config(host: &Host, verifier: Arc<PinVerifier>) -> Result<Arc<ServerConfig>, Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut c = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(verifier)
        .with_single_cert(host.cert.chain.clone(), host.cert.key.clone_key())?;
    c.alpn_protocols = SERVER_ALPNS.iter().map(|a| a.as_bytes().to_vec()).collect();
    c.send_tls13_tickets = 0;
    c.session_storage = Arc::new(NoServerSessionStorage {});
    c.max_early_data_size = 0;
    Ok(Arc::new(c))
}
