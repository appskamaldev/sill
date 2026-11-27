//! Binary payloads: parameter sets (kind 0), thumbnails (3), app icons (4), cursor shapes (14).

/// HEVC VPS/SPS/PPS plus the NAL length-prefix size (normally 4).
/// Layout: nalLen(1) count(1), then per set length(2, BE) bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterSets {
    pub nal_unit_header_length: u8,
    pub sets: Vec<Vec<u8>>,
}

impl ParameterSets {
    /// Panics on more than 255 sets or a set over 65535 bytes, which no encoder produces.
    pub fn encoded(&self) -> Vec<u8> {
        let mut d = vec![self.nal_unit_header_length, u8::try_from(self.sets.len()).expect("over 255 sets")];
        for s in &self.sets {
            d.extend_from_slice(&u16::try_from(s.len()).expect("set over 65535 bytes").to_be_bytes());
            d.extend_from_slice(s);
        }
        d
    }

    /// `None` when shorter than announced; bytes after the last set are ignored, as upstream does.
    pub fn decode(d: &[u8]) -> Option<ParameterSets> {
        if d.len() < 2 {
            return None;
        }
        let count = d[1] as usize;
        let mut offset = 2;
        let mut sets = Vec::with_capacity(count);
        for _ in 0..count {
            let len = u16::from_be_bytes(d.get(offset..offset + 2)?.try_into().ok()?) as usize;
            offset += 2;
            sets.push(d.get(offset..offset + len)?.to_vec());
            offset += len;
        }
        Some(ParameterSets { nal_unit_header_length: d[0], sets })
    }
}

/// Kind 3: windowID(4, BE) + JPEG.
pub fn encode_thumbnail(window_id: u32, jpeg: &[u8]) -> Vec<u8> {
    let mut d = window_id.to_be_bytes().to_vec();
    d.extend_from_slice(jpeg);
    d
}

/// `None` unless at least one image byte follows the id.
pub fn decode_thumbnail(d: &[u8]) -> Option<(u32, &[u8])> {
    if d.len() <= 4 {
        return None;
    }
    Some((u32::from_be_bytes(d[..4].try_into().ok()?), &d[4..]))
}

/// Kind 4: bundleIDLength(2, BE) + bundle ID (UTF-8) + PNG. Panics on an id over 65535 bytes.
pub fn encode_icon(bundle_id: &str, png: &[u8]) -> Vec<u8> {
    let id = bundle_id.as_bytes();
    let mut d = u16::try_from(id.len()).expect("bundle id over 65535 bytes").to_be_bytes().to_vec();
    d.extend_from_slice(id);
    d.extend_from_slice(png);
    d
}

/// `None` unless at least one image byte follows a valid UTF-8 id.
pub fn decode_icon(d: &[u8]) -> Option<(&str, &[u8])> {
    if d.len() <= 2 {
        return None;
    }
    let len = u16::from_be_bytes([d[0], d[1]]) as usize;
    if d.len() <= 2 + len {
        return None;
    }
    let id = std::str::from_utf8(&d[2..2 + len]).ok()?;
    Some((id, &d[2 + len..]))
}

/// Kind 14: hotX, hotY, width, height as f32 BE (points), then PNG.
#[derive(Clone, Debug, PartialEq)]
pub struct CursorShape {
    pub hot_x: f32,
    pub hot_y: f32,
    pub width: f32,
    pub height: f32,
    pub png: Vec<u8>,
}

impl CursorShape {
    pub fn encoded(&self) -> Vec<u8> {
        let mut d = Vec::with_capacity(16 + self.png.len());
        for v in [self.hot_x, self.hot_y, self.width, self.height] {
            d.extend_from_slice(&v.to_bits().to_be_bytes());
        }
        d.extend_from_slice(&self.png);
        d
    }

    /// `None` unless at least one PNG byte follows the 16.
    pub fn decode(d: &[u8]) -> Option<CursorShape> {
        if d.len() <= 16 {
            return None;
        }
        let f = |i: usize| f32::from_bits(u32::from_be_bytes([d[i * 4], d[i * 4 + 1], d[i * 4 + 2], d[i * 4 + 3]]));
        Some(CursorShape { hot_x: f(0), hot_y: f(1), width: f(2), height: f(3), png: d[16..].to_vec() })
    }
}
