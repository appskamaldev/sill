//! The SubjectPublicKeyInfo of a P-256 key, its SHA-256 (the pin) and the Mac ID
//! (upstream `SPKI`, `MacID`, `DER`).

use sha2::{Digest, Sha256};

const EC_PUBLIC_KEY: [u8; 7] = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01]; // 1.2.840.10045.2.1
const PRIME256V1: [u8; 8] = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07]; // 1.2.840.10045.3.1.7

pub(crate) fn der_length(n: usize) -> Vec<u8> {
    if n < 0x80 {
        return vec![n as u8];
    }
    let bytes: Vec<u8> = n.to_be_bytes().into_iter().skip_while(|&b| b == 0).collect();
    let mut out = vec![0x80 | bytes.len() as u8];
    out.extend(bytes);
    out
}

pub(crate) fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend(der_length(content.len()));
    out.extend_from_slice(content);
    out
}

/// `SEQUENCE { SEQUENCE { OID ecPublicKey, OID prime256v1 }, BIT STRING (0x00 ‖ point) }` for a
/// 65-byte uncompressed X9.63 point (04 ‖ X ‖ Y).
pub fn spki_der(point: &[u8]) -> Vec<u8> {
    let algorithm = tlv(0x30, &[tlv(0x06, &EC_PUBLIC_KEY), tlv(0x06, &PRIME256V1)].concat());
    let mut bits = vec![0x00];
    bits.extend_from_slice(point);
    tlv(0x30, &[algorithm, tlv(0x03, &bits)].concat())
}

/// SHA-256 of `spki_der(point)`: 32 bytes.
pub fn spki_fingerprint(point: &[u8]) -> [u8; 32] {
    Sha256::digest(spki_der(point)).into()
}

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Crockford base32 of the fingerprint's first 10 bytes: 16 characters; "" for fewer bytes.
pub fn mac_id(fingerprint: &[u8]) -> String {
    let Some(bytes) = fingerprint.get(..10) else { return String::new() };
    let mut out = String::with_capacity(16);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
        buffer &= (1 << bits) - 1;
    }
    out
}

/// 16 characters of the Crockford alphabet, upper case.
pub fn mac_id_is_well_formed(text: &str) -> bool {
    text.chars().count() == 16 && text.bytes().all(|b| ALPHABET.contains(&b))
}

/// "5KD2Q7": the first six characters of the Mac ID, for log lines.
pub fn short_name(fingerprint: &[u8]) -> String {
    mac_id(fingerprint).chars().take(6).collect()
}
