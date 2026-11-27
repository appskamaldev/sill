//! The strict address parser (upstream `AddressParser.swift`), with ports of the BSD `inet_aton`,
//! `inet_pton` and `inet_ntop` it relies on, so Windows reads every address exactly as macOS does.

/// What kind of host a parsed address is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AddressKind {
    Ipv4,
    Ipv6,
    Name,
}

impl AddressKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AddressKind::Ipv4 => "ipv4",
            AddressKind::Ipv6 => "ipv6",
            AddressKind::Name => "name",
        }
    }
}

/// One address as Sill dials it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ParsedAddress {
    /// "100.101.102.103", "fd7a:115c:a1e0::1234" (no brackets, compressed, lower case) or a name
    /// as typed.
    pub host: String,
    /// `None`: the host's remote port applies (7455 when unknown).
    pub port: Option<u16>,
    pub kind: AddressKind,
}

impl ParsedAddress {
    /// "host" or "host:port", IPv6 in brackets.
    pub fn text(&self) -> String {
        let h = if self.kind == AddressKind::Ipv6 { format!("[{}]", self.host) } else { self.host.clone() };
        match self.port {
            Some(p) => format!("{h}:{p}"),
            None => h,
        }
    }
}

/// Why an input was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressFailure {
    Empty,
    Malformed,
    Zone,
    Ipv4,
    Ipv6,
    Name,
    Port,
    PairingLink,
}

impl AddressFailure {
    /// Upstream's case name.
    pub fn as_str(self) -> &'static str {
        match self {
            AddressFailure::Empty => "empty",
            AddressFailure::Malformed => "malformed",
            AddressFailure::Zone => "zone",
            AddressFailure::Ipv4 => "ipv4",
            AddressFailure::Ipv6 => "ipv6",
            AddressFailure::Name => "name",
            AddressFailure::Port => "port",
            AddressFailure::PairingLink => "pairingLink",
        }
    }
}

/// Parses `raw`. The ends are trimmed first; any other whitespace refuses it.
pub fn parse(raw: &str) -> Result<ParsedAddress, AddressFailure> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(AddressFailure::Empty);
    }
    if s.to_lowercase().starts_with("sill:") {
        return Err(AddressFailure::PairingLink);
    }
    if s.chars().any(|c| c.is_whitespace() || (c as u32) < 0x20 || c as u32 == 0x7F) {
        return Err(AddressFailure::Malformed);
    }
    if s.contains('%') {
        return Err(AddressFailure::Zone);
    }
    if let Some(inner) = s.strip_prefix('[') {
        let Some(close) = inner.find(']') else { return Err(AddressFailure::Ipv6) };
        let host = &inner[..close];
        let rest = &inner[close + 1..];
        let port = if rest.is_empty() {
            None
        } else {
            match rest.strip_prefix(':') {
                Some(p) => Some(p),
                None => return Err(AddressFailure::Malformed),
            }
        };
        let v6 = ipv6(host).ok_or(AddressFailure::Ipv6)?;
        return match port {
            Some(p) => Ok(ParsedAddress { port: Some(parse_port(p).ok_or(AddressFailure::Port)?), ..v6 }),
            None => Ok(v6),
        };
    }
    let colons = s.matches(':').count();
    if colons >= 2 {
        // A bare IPv6 address, never an address plus a port.
        return ipv6(s).ok_or(AddressFailure::Ipv6);
    }
    let (host_part, port_part) = match s.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (s, None),
    };
    let host = host_only(host_part)?;
    match port_part {
        Some(p) => Ok(ParsedAddress { port: Some(parse_port(p).ok_or(AddressFailure::Port)?), ..host }),
        None => Ok(host),
    }
}

fn host_only(h: &str) -> Result<ParsedAddress, AddressFailure> {
    if h.is_empty() {
        return Err(AddressFailure::Malformed);
    }
    let body = h.strip_suffix('.').unwrap_or(h);
    let labels: Vec<&str> = body.split('.').collect();
    let all_numeric = labels.iter().all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()));
    if all_numeric {
        let canonical = !h.ends_with('.')
            && labels.len() == 4
            && labels.iter().all(|l| {
                (*l == "0" || !l.starts_with('0')) && l.len() <= 3 && l.parse::<u32>().is_ok_and(|n| n <= 255)
            });
        return if canonical {
            Ok(ParsedAddress { host: h.to_string(), port: None, kind: AddressKind::Ipv4 })
        } else {
            Err(AddressFailure::Ipv4)
        };
    }
    // Whatever inet_aton would read as an IPv4 address is refused: the system would dial the
    // number, not a name.
    if inet_aton(h.as_bytes()).is_some() {
        return Err(AddressFailure::Ipv4);
    }
    if !is_name(h) {
        return Err(AddressFailure::Name);
    }
    Ok(ParsedAddress { host: h.to_string(), port: None, kind: AddressKind::Name })
}

/// RFC 1123: labels of 1–63 ASCII letters, digits and hyphens, no hyphen at either end, at most
/// 253 characters without the optional trailing dot.
fn is_name(h: &str) -> bool {
    let body = h.strip_suffix('.').unwrap_or(h);
    if body.is_empty() || body.chars().count() > 253 {
        return false;
    }
    body.split('.').all(|label| {
        (1..=63).contains(&label.chars().count())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// A bare IPv6 address: its compressed lower-case form, or IPv4 for an IPv4-mapped one.
fn ipv6(h: &str) -> Option<ParsedAddress> {
    if h.is_empty() || h.contains('%') {
        return None;
    }
    let bytes = inet_pton6(h.as_bytes())?;
    if bytes[..10].iter().all(|&b| b == 0) && bytes[10] == 0xFF && bytes[11] == 0xFF {
        let host = bytes[12..].iter().map(u8::to_string).collect::<Vec<_>>().join(".");
        return Some(ParsedAddress { host, port: None, kind: AddressKind::Ipv4 });
    }
    Some(ParsedAddress { host: inet_ntop6(&bytes), port: None, kind: AddressKind::Ipv6 })
}

fn parse_port(p: &str) -> Option<u16> {
    if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: u32 = p.parse().ok()?;
    (1..=65535).contains(&v).then_some(v as u16)
}

/// BSD `inet_aton`: one to four parts, each decimal, octal (leading 0) or hex (0x), the last
/// filling the remaining bytes. Values wrap as the C code's `u_long` does.
pub(crate) fn inet_aton(s: &[u8]) -> Option<u32> {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut i = 0;
    let mut parts: Vec<u64> = Vec::with_capacity(3);
    let mut val: u64;
    let mut digit: bool;
    loop {
        if !at(i).is_ascii_digit() {
            return None;
        }
        val = 0;
        let mut base = 10u64;
        digit = false;
        if at(i) == b'0' {
            i += 1;
            if at(i) == b'x' || at(i) == b'X' {
                base = 16;
                i += 1;
            } else {
                base = 8;
                digit = true;
            }
        }
        loop {
            let c = at(i);
            if c.is_ascii_digit() {
                if base == 8 && (c == b'8' || c == b'9') {
                    return None;
                }
                val = val.wrapping_mul(base).wrapping_add((c - b'0') as u64);
                digit = true;
            } else if base == 16 && c.is_ascii_hexdigit() {
                val = (val << 4) | (c.to_ascii_lowercase() - b'a' + 10) as u64;
                digit = true;
            } else {
                break;
            }
            i += 1;
        }
        if at(i) == b'.' {
            if parts.len() >= 3 || val > 0xFF {
                return None;
            }
            parts.push(val);
            i += 1;
        } else {
            break;
        }
    }
    let c = at(i);
    if c != 0 && !matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r') {
        return None;
    }
    if !digit {
        return None;
    }
    let value = match parts.len() {
        0 => val,
        1 if val <= 0xFF_FFFF => val | (parts[0] << 24),
        2 if val <= 0xFFFF => val | (parts[0] << 24) | (parts[1] << 16),
        3 if val <= 0xFF => val | (parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8),
        _ => return None,
    };
    Some(value as u32)
}

/// Apple's (BIND 4.9.4) `inet_pton4`: a dotted quad, leading zeros allowed (measured), used
/// inside an IPv6 address.
fn inet_pton4(s: &[u8]) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let (mut octets, mut saw_digit, mut tp) = (0, false, 0usize);
    for &c in s {
        if c.is_ascii_digit() {
            let new = out[tp] as u32 * 10 + (c - b'0') as u32;
            if new > 255 {
                return None;
            }
            out[tp] = new as u8;
            if !saw_digit {
                octets += 1;
                if octets > 4 {
                    return None;
                }
                saw_digit = true;
            }
        } else if c == b'.' && saw_digit {
            if octets == 4 {
                return None;
            }
            tp += 1;
            out[tp] = 0;
            saw_digit = false;
        } else {
            return None;
        }
    }
    (octets >= 4).then_some(out)
}

/// Apple's (BIND 4.9.4) `inet_pton6`.
pub(crate) fn inet_pton6(src: &[u8]) -> Option<[u8; 16]> {
    let mut tmp = [0u8; 16];
    let mut tp = 0usize;
    let mut colonp: Option<usize> = None;
    let mut i = 0usize;
    if src.first() == Some(&b':') {
        if src.get(1) != Some(&b':') {
            return None;
        }
        i = 1;
    }
    let mut curtok = i;
    let mut seen_xdigits = 0;
    let mut val: u32 = 0;
    while i < src.len() {
        let ch = src[i];
        i += 1;
        if ch.is_ascii_hexdigit() {
            // A group's value, not its digit count, is limited: "00001::" is 1:: (measured).
            val = (val << 4) | (ch as char).to_digit(16)?;
            if val > 0xFFFF {
                return None;
            }
            seen_xdigits += 1;
            continue;
        }
        if ch == b':' {
            curtok = i;
            if seen_xdigits == 0 {
                if colonp.is_some() {
                    return None;
                }
                colonp = Some(tp);
                continue;
            } else if i == src.len() {
                return None;
            }
            if tp + 2 > 16 {
                return None;
            }
            tmp[tp] = (val >> 8) as u8;
            tmp[tp + 1] = val as u8;
            tp += 2;
            seen_xdigits = 0;
            val = 0;
            continue;
        }
        if ch == b'.' && tp + 4 <= 16 {
            if let Some(v4) = inet_pton4(&src[curtok..]) {
                tmp[tp..tp + 4].copy_from_slice(&v4);
                tp += 4;
                seen_xdigits = 0;
                break;
            }
        }
        return None;
    }
    if seen_xdigits > 0 {
        if tp + 2 > 16 {
            return None;
        }
        tmp[tp] = (val >> 8) as u8;
        tmp[tp + 1] = val as u8;
        tp += 2;
    }
    if let Some(c) = colonp {
        if tp == 16 {
            return None;
        }
        let n = tp - c;
        for k in 1..=n {
            tmp[16 - k] = tmp[c + n - k];
            tmp[c + n - k] = 0;
        }
        tp = 16;
    }
    (tp == 16).then_some(tmp)
}

/// BSD `inet_ntop6`: the longest run of two or more zero words becomes "::" (the first on a tie),
/// and an address whose first 96 bits are zero (or 80 zero bits then ffff) ends in dotted IPv4.
pub(crate) fn inet_ntop6(b: &[u8; 16]) -> String {
    let words: Vec<u16> = (0..8).map(|i| u16::from_be_bytes([b[2 * i], b[2 * i + 1]])).collect();
    // (first word, length) of a run of zero words.
    type Run = Option<(usize, usize)>;
    let (mut best, mut cur): (Run, Run) = (None, None);
    for (i, &w) in words.iter().enumerate() {
        if w == 0 {
            cur = Some(match cur {
                Some((base, len)) => (base, len + 1),
                None => (i, 1),
            });
        } else if let Some(c) = cur.take() {
            if best.map_or(true, |b| c.1 > b.1) {
                best = Some(c);
            }
        }
    }
    if let Some(c) = cur {
        if best.map_or(true, |b| c.1 > b.1) {
            best = Some(c);
        }
    }
    let best = best.filter(|b| b.1 >= 2);
    let mut out = String::new();
    let mut i = 0;
    while i < 8 {
        if let Some((base, len)) = best {
            if i >= base && i < base + len {
                if i == base {
                    out.push(':');
                }
                i += 1;
                continue;
            }
        }
        if i != 0 {
            out.push(':');
        }
        if i == 6 {
            if let Some((0, len)) = best {
                if len == 6 || (len == 5 && words[5] == 0xFFFF) {
                    out.push_str(&format!("{}.{}.{}.{}", b[12], b[13], b[14], b[15]));
                    break;
                }
            }
        }
        out.push_str(&format!("{:x}", words[i]));
        i += 1;
    }
    if let Some((base, len)) = best {
        if base + len == 8 {
            out.push(':');
        }
    }
    out
}
