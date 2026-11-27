//! Kind 18's signature (upstream `SignedMacInfo.signing` and `verified()`): ECDSA P-256 /
//! SHA-256, DER, over the exact JSON bytes of a `MacInfo`.

use crate::spki;
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use sill_wire::swift_json::{data_from_base64, data_to_base64};
use sill_wire::{MacInfo, SignedMacInfo};

/// Signs `info` with the host's identity key (RFC 6979 deterministic nonces).
pub fn sign(info: &MacInfo, key: &SigningKey) -> SignedMacInfo {
    sign_bytes(&sill_wire::encode(info), key)
}

/// Signs JSON bytes already made from a `MacInfo`.
pub fn sign_bytes(bytes: &[u8], key: &SigningKey) -> SignedMacInfo {
    let sig: Signature = key.sign(bytes);
    let point = key.verifying_key().to_encoded_point(false);
    SignedMacInfo {
        info: data_to_base64(bytes),
        key: data_to_base64(point.as_bytes()),
        sig: data_to_base64(sig.to_der().as_bytes()),
    }
}

/// The MacInfo and the signing key's fingerprint, when the signature verifies over the exact
/// `info` bytes, the key is a 65-byte P-256 point, the JSON decodes and its `macID` is that key's.
pub fn verify(signed: &SignedMacInfo) -> Option<(MacInfo, [u8; 32])> {
    let bytes = data_from_base64(&signed.info)?;
    let point = data_from_base64(&signed.key)?;
    if point.len() != 65 || point[0] != 0x04 {
        return None;
    }
    let sig = Signature::from_der(&data_from_base64(&signed.sig)?).ok()?;
    VerifyingKey::from_sec1_bytes(&point).ok()?.verify(&bytes, &sig).ok()?;
    let info: MacInfo = sill_wire::decode(&bytes)?;
    let fp = spki::spki_fingerprint(&point);
    (info.mac_id == spki::mac_id(&fp)).then_some((info, fp))
}
