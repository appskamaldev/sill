//! base64url without padding (RFC 4648 §5), canonical only (upstream `Base64URL`).

use base64::alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, URL_SAFE_NO_PAD};
use base64::engine::DecodePaddingMode;
use base64::Engine;

const LENIENT: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::RequireNone),
);

pub fn encode(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

/// `None` for anything that is not canonical unpadded base64url: padding, `+` or `/`,
/// whitespace, a length no encoding produces, non-zero trailing bits.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    if !text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') || text.len() % 4 == 1 {
        return None;
    }
    let d = LENIENT.decode(text).ok()?;
    (encode(&d) == text).then_some(d)
}
