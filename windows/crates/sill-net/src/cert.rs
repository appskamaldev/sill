//! The host's certificate, and the one thing read from a device's: its P-256 key.
//!
//! Upstream (`RemoteIdentity.swift: SelfSignedCertificate`) builds a v3, P-256, ECDSA-SHA256,
//! self-signed certificate with a random 16-hex-character CN, a random positive 16-byte serial and
//! validity 2026-01-01 to 9999-12-31, at every launch from the stored key. Here rcgen builds the
//! same shape. Nothing in either certificate but the key is ever trusted.

use p256::pkcs8::EncodePrivateKey;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sill_store::Identity;

/// The DER certificate and its PKCS#8 key, as rustls takes them.
pub struct HostCertificate {
    pub chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
}

impl Clone for HostCertificate {
    fn clone(&self) -> HostCertificate {
        HostCertificate { chain: self.chain.clone(), key: self.key.clone_key() }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A fresh certificate for `identity`'s key. `random` fills the CN and the serial.
pub fn make(identity: &Identity, mut random: impl FnMut(&mut [u8]) -> bool) -> Result<HostCertificate, String> {
    let pkcs8 = identity.key.to_pkcs8_der().map_err(|e| format!("pkcs8: {e}"))?;
    let key_pair = rcgen::KeyPair::try_from(pkcs8.as_bytes()).map_err(|e| format!("rcgen key: {e}"))?;
    let mut cn = [0u8; 8];
    let mut serial = [0u8; 16];
    if !random(&mut cn) || !random(&mut serial) {
        return Err("the random generator failed".into());
    }
    serial[0] = (serial[0] & 0x7F) | 0x01;
    let mut params = rcgen::CertificateParams::default();
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, hex(&cn));
    params.distinguished_name = dn;
    params.not_before = rcgen::date_time_ymd(2026, 1, 1);
    params.not_after = rcgen::date_time_ymd(9999, 12, 31);
    params.serial_number = Some(rcgen::SerialNumber::from_slice(&serial));
    let cert = params.self_signed(&key_pair).map_err(|e| format!("rcgen: {e}"))?;
    Ok(HostCertificate {
        chain: vec![cert.der().clone()],
        key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(pkcs8.as_bytes().to_vec())),
    })
}

// MARK: - A minimal DER reader: Certificate → TBSCertificate → SubjectPublicKeyInfo.

/// One TLV: (tag, contents, rest).
fn tlv(d: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, d) = d.split_first()?;
    let (&first, mut d) = d.split_first()?;
    let len = if first < 0x80 {
        first as usize
    } else {
        let n = (first & 0x7F) as usize;
        if n == 0 || n > 4 || d.len() < n {
            return None;
        }
        let mut l = 0usize;
        for &b in &d[..n] {
            l = (l << 8) | b as usize;
        }
        d = &d[n..];
        l
    };
    if d.len() < len {
        return None;
    }
    Some((tag, &d[..len], &d[len..]))
}

/// id-ecPublicKey (1.2.840.10045.2.1) and prime256v1 (1.2.840.10045.3.1.7).
const EC_PUBLIC_KEY: [u8; 7] = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
const PRIME256V1: [u8; 8] = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];

/// The uncompressed P-256 point (65 bytes) of a certificate's key; `None` for anything else (a
/// malformed certificate, another algorithm or curve, a compressed point). Upstream's
/// `SPKI.fingerprint` likewise yields nothing for a key that is not P-256.
pub fn p256_point(cert: &[u8]) -> Option<Vec<u8>> {
    let (0x30, cert, _) = tlv(cert)? else { return None };
    let (0x30, tbs, _) = tlv(cert)? else { return None };
    let mut rest = tbs;
    // [0] EXPLICIT version, optional.
    if rest.first() == Some(&0xA0) {
        rest = tlv(rest)?.2;
    }
    // serial, signature, issuer, validity, subject.
    for _ in 0..5 {
        rest = tlv(rest)?.2;
    }
    let (0x30, spki, _) = tlv(rest)? else { return None };
    let (0x30, alg, after) = tlv(spki)? else { return None };
    let (0x06, oid, alg_rest) = tlv(alg)? else { return None };
    if oid != EC_PUBLIC_KEY {
        return None;
    }
    let (0x06, curve, _) = tlv(alg_rest)? else { return None };
    if curve != PRIME256V1 {
        return None;
    }
    let (0x03, bits, _) = tlv(after)? else { return None };
    let (&unused, point) = bits.split_first()?;
    if unused != 0 || point.len() != 65 || point[0] != 0x04 {
        return None;
    }
    Some(point.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sill_pairing::spki;

    fn identity() -> Identity {
        Identity { key: p256::SecretKey::from_slice(&[0x11; 32]).unwrap(), recognition_key: [0; 32] }
    }

    #[test]
    fn the_certificate_carries_the_stored_key() {
        let id = identity();
        let c = make(&id, sill_store::random).unwrap();
        let point = p256_point(&c.chain[0]).unwrap();
        assert_eq!(point, id.public_point());
        assert_eq!(spki::spki_fingerprint(&point), id.fingerprint());
        // A fresh CN and serial each time, the same key.
        let d = make(&id, sill_store::random).unwrap();
        assert_ne!(c.chain[0], d.chain[0]);
        assert_eq!(p256_point(&d.chain[0]).unwrap(), point);
    }

    #[test]
    fn other_keys_and_junk_give_nothing() {
        assert_eq!(p256_point(&[]), None);
        assert_eq!(p256_point(&[0x30, 0x03, 0x02, 0x01, 0x00]), None);
        let ed = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P384_SHA384).unwrap();
        let cert = rcgen::CertificateParams::default().self_signed(&ed).unwrap();
        assert_eq!(p256_point(cert.der()), None);
    }
}
