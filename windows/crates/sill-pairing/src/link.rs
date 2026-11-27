//! The QR code's content: `sill://pair?v=1&m=<macID>&k=<fp>&s=<secret>&n=<name>&p=<port>&a=<addr>…`
//! (upstream `PairLink`).

use crate::address::{self, ParsedAddress};
use crate::{base64url, safe_text, spki};

pub const MAX_ADDRESSES: usize = 5;
pub const NAME_LIMIT: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairLink {
    /// Always `mac_id(fingerprint)`.
    pub mac_id: String,
    /// The host's pin: SHA-256 of its SPKI (32 bytes).
    pub fingerprint: Vec<u8>,
    /// The QR path's one-time key (16 bytes).
    pub secret: Vec<u8>,
    /// The name after SafeText, at most 64 characters; "Mac" when it had none.
    pub name: String,
    pub port: u16,
    /// At most 5, in dial order.
    pub addresses: Vec<ParsedAddress>,
}

/// Why a link was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkFailure {
    NotALink,
    Version,
    /// `m`, `k`, `s` or `p` missing, repeated or malformed.
    Field(&'static str),
    MacIdMismatch,
    /// An `a` the address parser refuses ("" when it had no value).
    Address(String),
}

impl PairLink {
    pub fn new(fingerprint: &[u8], secret: &[u8], name: &str, port: u16, addresses: &[ParsedAddress]) -> PairLink {
        let clean = safe_text::label(name, NAME_LIMIT);
        PairLink {
            mac_id: spki::mac_id(fingerprint),
            fingerprint: fingerprint.to_vec(),
            secret: secret.to_vec(),
            name: if clean.is_empty() { "Mac".to_string() } else { clean },
            port,
            addresses: addresses.iter().take(MAX_ADDRESSES).cloned().collect(),
        }
    }

    /// Values percent-encoded down to RFC 3986's unreserved characters and ":".
    pub fn url(&self) -> String {
        let mut items: Vec<(&str, String)> = vec![
            ("v", "1".to_string()),
            ("m", self.mac_id.clone()),
            ("k", base64url::encode(&self.fingerprint)),
            ("s", base64url::encode(&self.secret)),
            ("n", self.name.clone()),
            ("p", self.port.to_string()),
        ];
        items.extend(self.addresses.iter().take(MAX_ADDRESSES).map(|a| ("a", a.text())));
        let query: Vec<String> = items.iter().map(|(k, v)| format!("{k}={}", escape(v))).collect();
        format!("sill://pair?{}", query.join("&"))
    }

    /// Refuses `v ≠ 1`; a missing, repeated or malformed `m`, `k`, `s` or `p`; `m ≠ MacID(k)`;
    /// any `a` the address parser refuses. Ignores unknown parameters, keeps the first 5
    /// addresses and cleans `n`.
    pub fn parse(text: &str) -> Result<PairLink, LinkFailure> {
        let items = query_items(text.trim()).ok_or(LinkFailure::NotALink)?;
        let single = |key: &str| -> Option<&str> {
            let values: Vec<&(String, Option<String>)> = items.iter().filter(|(n, _)| n == key).collect();
            match values.as_slice() {
                [(_, Some(v))] if !v.is_empty() => Some(v.as_str()),
                _ => None,
            }
        };
        if single("v") != Some("1") {
            return Err(LinkFailure::Version);
        }
        let m = single("m").filter(|m| spki::mac_id_is_well_formed(m)).ok_or(LinkFailure::Field("m"))?;
        let fp = single("k").and_then(base64url::decode).filter(|f| f.len() == 32).ok_or(LinkFailure::Field("k"))?;
        let secret =
            single("s").and_then(base64url::decode).filter(|s| s.len() == 16).ok_or(LinkFailure::Field("s"))?;
        let port = single("p")
            .filter(|p| p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|p| p.parse::<u32>().ok())
            .filter(|p| (1..=65535).contains(p))
            .ok_or(LinkFailure::Field("p"))? as u16;
        if m != spki::mac_id(&fp) {
            return Err(LinkFailure::MacIdMismatch);
        }
        let mut addresses = Vec::new();
        for (_, value) in items.iter().filter(|(n, _)| n == "a") {
            let raw = value.clone().unwrap_or_default();
            match value.as_deref().map(address::parse) {
                Some(Ok(a)) => addresses.push(a),
                _ => return Err(LinkFailure::Address(raw)),
            }
        }
        let name = items.iter().find(|(n, _)| n == "n").and_then(|(_, v)| v.clone()).unwrap_or_default();
        Ok(PairLink::new(&fp, &secret, &name, port, &addresses))
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b':') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `URLComponents` as far as a link needs it (measured in `vectors/pairing.json`): the scheme
/// must be "sill" and the host "pair" (any case, after any user info, without a port), the path
/// empty or "/", and a query present. `None` otherwise. Each `&`-separated item is a name and,
/// after its first "=", a value; both percent-decoded, an invalid escape kept as typed, "+" kept.
fn query_items(text: &str) -> Option<Vec<(String, Option<String>)>> {
    let (scheme, rest) = text.split_once(':')?;
    let mut sc = scheme.chars();
    if !sc.next()?.is_ascii_alphabetic() || !sc.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    if !scheme.eq_ignore_ascii_case("sill") {
        return None;
    }
    let rest = rest.strip_prefix("//")?;
    let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = match host_port.rsplit_once(':') {
        Some((h, p)) if p.bytes().all(|b| b.is_ascii_digit()) => h,
        Some(_) => return None,
        None => host_port,
    };
    if !host.eq_ignore_ascii_case("pair") {
        return None;
    }
    let after = &rest[authority_end..];
    let (path, query) = after.split_once('?')?;
    if !(path.is_empty() || path == "/") {
        return None;
    }
    if query.is_empty() {
        return Some(Vec::new());
    }
    Some(
        query
            .split('&')
            .map(|item| match item.split_once('=') {
                Some((n, v)) => (percent_decode(n), Some(percent_decode(v))),
                None => (percent_decode(item), None),
            })
            .collect(),
    )
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hexit = |j: usize| b.get(j).and_then(|c| (*c as char).to_digit(16));
        if b[i] == b'%' {
            if let (Some(h), Some(l)) = (hexit(i + 1), hexit(i + 2)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}
