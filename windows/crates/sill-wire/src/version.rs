//! `SillVersion` and `SillProtocol` (upstream `Compatibility.swift`).

use std::cmp::Ordering;
use std::fmt;

/// The wire's generation (`SillProtocol.current`).
pub const PROTOCOL_CURRENT: i64 = 1;

/// A version as tags, bundles and the wire write it: "v0.4.0", "0.4", "1.2.3-beta.1".
/// Compared part by part, a missing part counting as 0.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct SillVersion {
    /// Trailing zeros dropped, so "0.4" and "0.4.0" are equal; empty is 0.
    components: Vec<u32>,
}

impl SillVersion {
    /// 0: what a device that sends no version counts as.
    pub const ZERO: SillVersion = SillVersion { components: Vec::new() };

    pub fn from_components(mut components: Vec<u32>) -> SillVersion {
        while components.last() == Some(&0) {
            components.pop();
        }
        SillVersion { components }
    }

    pub fn components(&self) -> &[u32] {
        &self.components
    }

    /// Trim whitespace, drop one leading "v"/"V", keep the longest prefix of ASCII digits and
    /// dots without the dots at its end; it must start with a digit and have at most 8 parts of
    /// 1–9 digits each. What follows the prefix is ignored.
    pub fn parse(text: &str) -> Option<SillVersion> {
        let t = text.trim();
        let t = t.strip_prefix(['v', 'V']).unwrap_or(t);
        let end = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
        let prefix = t[..end].trim_end_matches('.');
        if !prefix.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        let parts: Vec<&str> = prefix.split('.').collect();
        if parts.len() > 8 {
            return None;
        }
        let mut numbers = Vec::with_capacity(parts.len());
        for p in parts {
            if !(1..=9).contains(&p.len()) {
                return None;
            }
            numbers.push(p.parse().ok()?);
        }
        Some(SillVersion::from_components(numbers))
    }
}

impl fmt::Display for SillVersion {
    /// At least two parts: 0.4.0 is "0.4", 1 is "1.0".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut c = self.components.clone();
        while c.len() < 2 {
            c.push(0);
        }
        let parts: Vec<String> = c.iter().map(u32::to_string).collect();
        f.write_str(&parts.join("."))
    }
}

impl Ord for SillVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let n = self.components.len().max(other.components.len());
        for i in 0..n {
            let x = self.components.get(i).copied().unwrap_or(0);
            let y = other.components.get(i).copied().unwrap_or(0);
            if x != y {
                return x.cmp(&y);
            }
        }
        Ordering::Equal
    }
}

impl PartialOrd for SillVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
