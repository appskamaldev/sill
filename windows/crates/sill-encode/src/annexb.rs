//! Annex-B (start codes, what Media Foundation emits) to what Sill sends: kind 0 holds VPS, SPS
//! and PPS with 4-byte length prefixes, and kind 1 holds the rest of the access unit with 4-byte
//! length prefixes. Access unit delimiters, SEI, filler, end-of-sequence and end-of-bitstream NAL
//! units are dropped: the device decodes without them (PROTOCOL.md §8).

use sill_wire::ParameterSets;

pub const VPS: u8 = 32;
pub const SPS: u8 = 33;
pub const PPS: u8 = 34;
const AUD: u8 = 35;
const EOS: u8 = 36;
const EOB: u8 = 37;
const FD: u8 = 38;
const SEI_PREFIX: u8 = 39;
const SEI_SUFFIX: u8 = 40;

/// HEVC `nal_unit_type`: bits 1..6 of the first header byte.
pub fn nal_type(nal: &[u8]) -> Option<u8> {
    nal.first().map(|b| (b >> 1) & 0x3F)
}

/// IRAP pictures (BLA, IDR, CRA: types 16..=23) start a decodable stream.
pub fn is_irap(t: u8) -> bool {
    (16..=23).contains(&t)
}

/// The NAL units of an Annex-B buffer, without their start codes (3- or 4-byte). Bytes before the
/// first start code are not a NAL unit and are skipped; trailing zero bytes of a unit (the next
/// 4-byte start code's leading zero, or `trailing_zero_8bits`) are trimmed.
pub fn split(d: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= d.len() {
        if d[i] == 0 && d[i + 1] == 0 && d[i + 2] == 1 {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut out = Vec::with_capacity(starts.len());
    for (k, &s) in starts.iter().enumerate() {
        let mut end = if k + 1 < starts.len() { starts[k + 1] - 3 } else { d.len() };
        while end > s && d[end - 1] == 0 {
            end -= 1;
        }
        if end > s {
            out.push(&d[s..end]);
        }
    }
    out
}

/// One encoder output, repacked.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Repacked {
    /// VPS, SPS and PPS found in this output, in order (empty if it carried none).
    pub parameter_sets: Vec<Vec<u8>>,
    /// The picture's NAL units, each with a 4-byte big-endian length prefix.
    pub au: Vec<u8>,
    /// It holds an IRAP picture.
    pub keyframe: bool,
}

pub fn repack(annexb: &[u8]) -> Repacked {
    let mut r = Repacked::default();
    for nal in split(annexb) {
        let Some(t) = nal_type(nal) else { continue };
        match t {
            VPS | SPS | PPS => r.parameter_sets.push(nal.to_vec()),
            AUD | EOS | EOB | FD | SEI_PREFIX | SEI_SUFFIX => {}
            _ => {
                r.keyframe |= is_irap(t);
                r.au.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                r.au.extend_from_slice(nal);
            }
        }
    }
    r
}

/// Kind 0's payload for VPS, SPS, PPS (in that order whatever order they came in); `None` unless
/// all three are there exactly once and each fits kind 0's 16-bit length.
pub fn kind0(sets: &[Vec<u8>]) -> Option<Vec<u8>> {
    let mut ordered = Vec::with_capacity(3);
    for want in [VPS, SPS, PPS] {
        let mut found = sets.iter().filter(|s| nal_type(s) == Some(want));
        let s = found.next()?;
        if found.next().is_some() || s.len() > usize::from(u16::MAX) {
            return None;
        }
        ordered.push(s.clone());
    }
    Some(ParameterSets { nal_unit_header_length: 4, sets: ordered }.encoded())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A NAL header for type `t` (layer 0, temporal id 1) and a body.
    fn nal(t: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![t << 1, 1];
        v.extend_from_slice(body);
        v
    }

    fn annexb(nals: &[(&[u8], Vec<u8>)]) -> Vec<u8> {
        nals.iter().flat_map(|(sc, n)| sc.iter().copied().chain(n.iter().copied())).collect()
    }

    const SC4: &[u8] = &[0, 0, 0, 1];
    const SC3: &[u8] = &[0, 0, 1];

    #[test]
    fn a_keyframe_output_splits_into_kind_0_and_a_length_prefixed_au() {
        let d = annexb(&[
            (SC4, nal(AUD, &[0x50])),
            (SC4, nal(VPS, &[1, 2])),
            (SC4, nal(SPS, &[3, 4, 5])),
            (SC3, nal(PPS, &[6])),
            (SC4, nal(SEI_PREFIX, &[9, 9])),
            (SC3, nal(19, &[0xAA, 0xBB, 0x80])), // IDR_W_RADL
        ]);
        let r = repack(&d);
        assert!(r.keyframe);
        assert_eq!(r.parameter_sets, vec![nal(VPS, &[1, 2]), nal(SPS, &[3, 4, 5]), nal(PPS, &[6])]);
        assert_eq!(r.au, [&[0, 0, 0, 5][..], &nal(19, &[0xAA, 0xBB, 0x80])].concat());
        let k0 = kind0(&r.parameter_sets).unwrap();
        let ps = ParameterSets::decode(&k0).unwrap();
        assert_eq!(ps.nal_unit_header_length, 4);
        assert_eq!(ps.sets, r.parameter_sets);
    }

    #[test]
    fn a_delta_frame_has_no_parameter_sets_and_is_no_keyframe() {
        let d = annexb(&[(SC4, nal(AUD, &[0x50])), (SC4, nal(1, &[7, 7, 7]))]); // TRAIL_R
        let r = repack(&d);
        assert!(!r.keyframe);
        assert!(r.parameter_sets.is_empty());
        assert_eq!(r.au, [&[0, 0, 0, 5][..], &nal(1, &[7, 7, 7])].concat());
    }

    #[test]
    fn several_slices_stay_in_order_and_trailing_zeros_go() {
        let d = annexb(&[(SC4, nal(19, &[1])), (SC4, nal(19, &[2, 0, 0])), (SC3, nal(1, &[3]))]);
        let r = repack(&[&d[..], &[0, 0]].concat());
        assert_eq!(
            r.au,
            [&[0, 0, 0, 3][..], &nal(19, &[1]), &[0, 0, 0, 3], &nal(19, &[2]), &[0, 0, 0, 3], &nal(1, &[3])].concat()
        );
    }

    #[test]
    fn kind_0_needs_each_set_once() {
        assert!(kind0(&[nal(VPS, &[1]), nal(SPS, &[2])]).is_none());
        assert!(kind0(&[nal(VPS, &[1]), nal(SPS, &[2]), nal(PPS, &[3]), nal(PPS, &[4])]).is_none());
        // Out of order in, VPS SPS PPS out.
        let k = kind0(&[nal(PPS, &[3]), nal(VPS, &[1]), nal(SPS, &[2])]).unwrap();
        let types: Vec<u8> = ParameterSets::decode(&k).unwrap().sets.iter().map(|s| nal_type(s).unwrap()).collect();
        assert_eq!(types, vec![VPS, SPS, PPS]);
    }

    #[test]
    fn junk_yields_nothing() {
        assert_eq!(repack(&[]), Repacked::default());
        assert_eq!(repack(&[1, 2, 3, 4]), Repacked::default());
        assert_eq!(repack(&[0, 0, 1]), Repacked::default());
    }
}
