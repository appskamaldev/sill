//! What Swift's `JSONDecoder` accepts, where serde's defaults differ.
//!
//! - An integer field takes any JSON number whose value is a whole number in range: `42`, `42.0`
//!   and `1e3` all decode, `42.5` does not (measured in `vectors/json.json`).
//! - `Data` is standard base64 with padding; Foundation accepts non-zero trailing bits and extra
//!   `=` as long as the text is a multiple of 4, and refuses whitespace and missing padding.

use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::DecodePaddingMode;
use base64::{alphabet, Engine};
use serde::de::{Deserialize, Deserializer, Error};
use serde::ser::Serializer;
use serde_json::Value;

/// A JSON number as Swift reads it into an integer type.
pub fn int_from_value<T: TryFrom<i128>>(v: &Value) -> Option<T> {
    let Value::Number(n) = v else { return None };
    let wide: i128 = if let Some(u) = n.as_u64() {
        u as i128
    } else if let Some(i) = n.as_i64() {
        i as i128
    } else {
        let f = n.as_f64()?;
        if !f.is_finite() || f.fract() != 0.0 || f.abs() >= 1.7e38 {
            return None;
        }
        f as i128
    };
    T::try_from(wide).ok()
}

pub fn int<'de, D: Deserializer<'de>, T: TryFrom<i128>>(d: D) -> Result<T, D::Error> {
    let v = Value::deserialize(d)?;
    int_from_value(&v).ok_or_else(|| D::Error::custom(format!("not an integer in range: {v}")))
}

pub fn opt_int<'de, D: Deserializer<'de>, T: TryFrom<i128>>(d: D) -> Result<Option<T>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => int_from_value(&v).map(Some).ok_or_else(|| D::Error::custom(format!("not an integer in range: {v}"))),
    }
}

const LENIENT: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// `Data(base64Encoded:)`.
pub fn data_from_base64(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let body = text.trim_end_matches('=');
    if body.len() % 4 == 1 || !body.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/') {
        return None;
    }
    LENIENT.decode(body).ok()
}

/// What `JSONEncoder` writes for `Data`.
pub fn data_to_base64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

pub fn opt_data<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
    match Option::<String>::deserialize(d)? {
        None => Ok(None),
        Some(s) => data_from_base64(&s).map(Some).ok_or_else(|| D::Error::custom("not base64")),
    }
}

pub fn ser_opt_data<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(d) => s.serialize_str(&data_to_base64(d)),
        None => s.serialize_none(),
    }
}

/// A nested struct field: Swift decodes a struct only from an object, never from an array, which
/// serde's derive would also take positionally.
pub fn obj<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(d: D) -> Result<T, D::Error> {
    let v = Value::deserialize(d)?;
    if !v.is_object() {
        return Err(D::Error::custom("expected an object"));
    }
    T::deserialize(v).map_err(D::Error::custom)
}

pub fn opt_obj<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v if v.is_object() => T::deserialize(v).map(Some).map_err(D::Error::custom),
        _ => Err(D::Error::custom("expected an object")),
    }
}

/// A list of structs: an array of objects.
pub fn obj_list<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    let v = Value::deserialize(d)?;
    match &v {
        Value::Array(items) if items.iter().all(Value::is_object) => Vec::<T>::deserialize(v).map_err(D::Error::custom),
        _ => Err(D::Error::custom("expected an array of objects")),
    }
}
