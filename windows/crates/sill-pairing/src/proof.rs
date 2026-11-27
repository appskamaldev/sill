//! The keys and proofs of one pairing exchange (upstream `PairingProof`). Fingerprints come from
//! the TLS session, never from a message.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub const ROUNDS: u32 = 600_000;
const LABEL: &str = "sill-pair-v1";

/// The typed path's key: PBKDF2-HMAC-SHA256(the 12 ASCII digits, "sill-pair-v1" ‖ fp_mac,
/// 600,000 rounds, 32 bytes). `None` for a code that is not 12 ASCII digits.
pub fn code_key(code: &str, mac_fingerprint: &[u8]) -> Option<[u8; 32]> {
    if code.len() != super::code::LENGTH || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut salt = LABEL.as_bytes().to_vec();
    salt.extend_from_slice(mac_fingerprint);
    let mut out = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(code.as_bytes(), &salt, ROUNDS, &mut out);
    Some(out)
}

fn message(who: &str, first: &[u8], second: &[u8]) -> Vec<u8> {
    let mut m = format!("{LABEL} {who}").into_bytes();
    m.push(0);
    m.extend_from_slice(first);
    m.extend_from_slice(second);
    m
}

fn hmac(key: &[u8], message: &[u8]) -> HmacSha256 {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(message);
    mac
}

/// proof_D = HMAC-SHA256(key, "sill-pair-v1 device" ‖ 0x00 ‖ fp_device ‖ fp_mac). `key` is the
/// code key, or the QR link's 16-byte secret.
pub fn device_proof(key: &[u8], device_fingerprint: &[u8], mac_fingerprint: &[u8]) -> [u8; 32] {
    hmac(key, &message("device", device_fingerprint, mac_fingerprint)).finalize().into_bytes().into()
}

/// proof_M = HMAC-SHA256(key, "sill-pair-v1 mac" ‖ 0x00 ‖ fp_mac ‖ fp_device).
pub fn mac_proof(key: &[u8], mac_fingerprint: &[u8], device_fingerprint: &[u8]) -> [u8; 32] {
    hmac(key, &message("mac", mac_fingerprint, device_fingerprint)).finalize().into_bytes().into()
}

/// The host's check of proof_D, in constant time.
pub fn is_valid_device_proof(proof: &[u8], key: &[u8], device_fingerprint: &[u8], mac_fingerprint: &[u8]) -> bool {
    hmac(key, &message("device", device_fingerprint, mac_fingerprint)).verify_slice(proof).is_ok()
}

/// The device's check of proof_M, in constant time.
pub fn is_valid_mac_proof(proof: &[u8], key: &[u8], mac_fingerprint: &[u8], device_fingerprint: &[u8]) -> bool {
    hmac(key, &message("mac", mac_fingerprint, device_fingerprint)).verify_slice(proof).is_ok()
}
