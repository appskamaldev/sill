//! The Bonjour TXT record's `r` (recognition tag) and `p` (the home door), upstream
//! `RecognitionTag` and `HomeDoorTXT`.

use crate::base64url;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;

/// The recognition tag's TXT key.
pub const TAG_TXT_KEY: &str = "r";
const TAG_LABEL: &str = "sill-tag-v1";

fn tag_mac(recognition_key: &[u8], nonce: &[u8]) -> [u8; 6] {
    let mut mac = Hmac::<Sha256>::new_from_slice(recognition_key).expect("HMAC takes any key length");
    mac.update(TAG_LABEL.as_bytes());
    mac.update(nonce);
    let full = mac.finalize().into_bytes();
    let mut out = [0u8; 6];
    out.copy_from_slice(&full[..6]);
    out
}

/// base64url(nonce ‖ HMAC-SHA256(key, "sill-tag-v1" ‖ nonce)[0..6]): 16 characters.
pub fn make_tag_with_nonce(recognition_key: &[u8], nonce: [u8; 6]) -> String {
    let mut tag = nonce.to_vec();
    tag.extend_from_slice(&tag_mac(recognition_key, &nonce));
    base64url::encode(&tag)
}

/// A tag with a fresh nonce from `random`; `None` if it fails. Make one per registration.
pub fn make_tag(recognition_key: &[u8], mut random: impl FnMut(&mut [u8]) -> bool) -> Option<String> {
    let mut nonce = [0u8; 6];
    random(&mut nonce).then(|| make_tag_with_nonce(recognition_key, nonce))
}

/// Whether `tag` was made with `recognition_key`. Constant time in the 6 compared bytes.
pub fn tag_matches(tag: &str, recognition_key: &[u8]) -> bool {
    let Some(bytes) = base64url::decode(tag) else { return false };
    if bytes.len() != 12 {
        return false;
    }
    let expected = tag_mac(recognition_key, &bytes[..6]);
    expected.iter().zip(&bytes[6..]).fold(0u8, |diff, (a, b)| diff | (a ^ b)) == 0
}

/// The home door's TXT key.
pub const DOOR_TXT_KEY: &str = "p";

/// How the home door speaks, as its TXT record says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Door {
    /// No `p`: plain TCP (a release client never dials it).
    Plain,
    /// `p=1`, or any value but "0": TLS, paired devices only.
    PairingRequired,
    /// `p=0`: TLS, any device.
    Open,
}

/// The value a TLS home door advertises.
pub fn door_value(require_pairing: bool) -> &'static str {
    if require_pairing {
        "1"
    } else {
        "0"
    }
}

/// The door a TXT record describes: "p", else "P".
pub fn door(txt: &HashMap<String, String>) -> Door {
    match txt.get("p").or_else(|| txt.get("P")) {
        None => Door::Plain,
        Some(v) if v == "0" => Door::Open,
        Some(_) => Door::PairingRequired,
    }
}
