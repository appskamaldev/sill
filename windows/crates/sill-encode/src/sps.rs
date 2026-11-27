//! Reads an HEVC SPS (ITU-T H.265 §7.3.2.2) as far as its VUI's video signal type: whether the
//! picture is full range, and its colour primaries, transfer and matrix. The device's decoder takes
//! these as the truth, so the host checks what each encoder writes (a full-range picture signalled
//! as limited looks crushed, a limited one signalled as full looks washed out).
//!
//! `set_full_range` rewrites `video_full_range_flag` in place when the SPS carries a video signal
//! type: one bit of the RBSP, then emulation prevention again.

/// The VUI's video signal type; `None` fields: not signalled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vui {
    pub full_range: Option<bool>,
    pub colour_primaries: Option<u8>,
    pub transfer: Option<u8>,
    pub matrix: Option<u8>,
    /// Bit offset of `video_full_range_flag` in the RBSP (after the 2-byte NAL header).
    full_range_bit: Option<usize>,
}

impl Vui {
    /// "full range, BT.709 (1/1/1)" or "range not signalled".
    pub fn words(&self) -> String {
        let range = match self.full_range {
            Some(true) => "full range",
            Some(false) => "limited range",
            None => "range not signalled",
        };
        match (self.colour_primaries, self.transfer, self.matrix) {
            (Some(p), Some(t), Some(m)) => format!("{range}, primaries {p} transfer {t} matrix {m}"),
            _ => format!("{range}, no colour description"),
        }
    }
}

/// The RBSP of a NAL unit: emulation prevention bytes (the 03 of 00 00 03) removed.
pub fn rbsp(nal: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(nal.len());
    let mut zeros = 0;
    for &b in nal {
        if zeros >= 2 && b == 3 {
            zeros = 0;
            continue;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        out.push(b);
    }
    out
}

/// Emulation prevention added back (a 03 after any 00 00 followed by 00..03).
pub fn ebsp(rbsp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rbsp.len() + 8);
    let mut zeros = 0;
    for &b in rbsp {
        if zeros >= 2 && b <= 3 {
            out.push(3);
            zeros = 0;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        out.push(b);
    }
    out
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn u(&mut self, n: usize) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let byte = *self.d.get(self.pos / 8)?;
            let bit = (byte >> (7 - self.pos % 8)) & 1;
            v = (v << 1) | u32::from(bit);
            self.pos += 1;
        }
        Some(v)
    }
    fn flag(&mut self) -> Option<bool> {
        self.u(1).map(|b| b == 1)
    }
    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while !self.flag()? {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        Some((1u32 << zeros) - 1 + self.u(zeros)?)
    }
    fn se(&mut self) -> Option<i32> {
        let k = self.ue()?;
        Some(if k % 2 == 1 { k.div_ceil(2) as i32 } else { -((k / 2) as i32) })
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        self.u(0)?;
        if self.pos + n > self.d.len() * 8 {
            return None;
        }
        self.pos += n;
        Some(())
    }
}

fn profile_tier_level(b: &mut Bits, max_sub_layers_minus1: u32) -> Option<()> {
    b.skip(88)?; // general profile space .. general_inbld/reserved
    b.skip(8)?; // general_level_idc
    let mut present = Vec::new();
    for _ in 0..max_sub_layers_minus1 {
        present.push((b.flag()?, b.flag()?));
    }
    if max_sub_layers_minus1 > 0 {
        for _ in max_sub_layers_minus1..8 {
            b.skip(2)?;
        }
    }
    for (profile, level) in present {
        if profile {
            b.skip(88)?;
        }
        if level {
            b.skip(8)?;
        }
    }
    Some(())
}

fn scaling_list_data(b: &mut Bits) -> Option<()> {
    for size_id in 0..4u32 {
        let step = if size_id == 3 { 3 } else { 1 };
        let mut matrix_id = 0;
        while matrix_id < 6 {
            if !b.flag()? {
                b.ue()?; // scaling_list_pred_matrix_id_delta
            } else {
                let coef_num = 64.min(1 << (4 + (size_id << 1)));
                if size_id > 1 {
                    b.se()?;
                }
                for _ in 0..coef_num {
                    b.se()?;
                }
            }
            matrix_id += step;
        }
    }
    Some(())
}

/// One st_ref_pic_set; returns its NumDeltaPocs.
fn st_ref_pic_set(b: &mut Bits, idx: u32, num_delta_pocs: &[u32]) -> Option<u32> {
    let inter = if idx != 0 { b.flag()? } else { false };
    if inter {
        // In the SPS, idx < num_short_term_ref_pic_sets: the reference set is the one before.
        let ref_idx = idx.checked_sub(1)? as usize;
        b.flag()?; // delta_rps_sign
        b.ue()?; // abs_delta_rps_minus1
        let mut n = 0;
        for _ in 0..=*num_delta_pocs.get(ref_idx)? {
            let used = b.flag()?;
            let use_delta = if used { true } else { b.flag()? };
            if use_delta {
                n += 1;
            }
        }
        Some(n)
    } else {
        let neg = b.ue()?;
        let pos = b.ue()?;
        if neg > 64 || pos > 64 {
            return None;
        }
        for _ in 0..neg + pos {
            b.ue()?;
            b.flag()?;
        }
        Some(neg + pos)
    }
}

/// The VUI's video signal type of the SPS NAL unit `nal` (with its 2-byte header, emulation
/// prevention included); `None` if it does not parse, `Some(Vui::default())` if it has no VUI.
pub fn vui(nal: &[u8]) -> Option<Vui> {
    if nal.len() < 3 || (nal[0] >> 1) & 0x3F != 33 {
        return None;
    }
    let r = rbsp(&nal[2..]);
    let b = &mut Bits { d: &r, pos: 0 };
    b.u(4)?; // sps_video_parameter_set_id
    let max_sub = b.u(3)?;
    b.flag()?;
    profile_tier_level(b, max_sub)?;
    b.ue()?; // sps_seq_parameter_set_id
    if b.ue()? == 3 {
        b.flag()?;
    }
    b.ue()?;
    b.ue()?; // width, height
    if b.flag()? {
        for _ in 0..4 {
            b.ue()?;
        }
    }
    b.ue()?;
    b.ue()?; // bit depths
    let log2_max_poc_lsb = b.ue()? + 4;
    let all = b.flag()?;
    for _ in (if all { 0 } else { max_sub })..=max_sub {
        b.ue()?;
        b.ue()?;
        b.ue()?;
    }
    for _ in 0..6 {
        b.ue()?;
    }
    if b.flag()? && b.flag()? {
        scaling_list_data(b)?;
    }
    b.flag()?;
    b.flag()?; // amp, sao
    if b.flag()? {
        b.skip(8)?;
        b.ue()?;
        b.ue()?;
        b.flag()?;
    }
    let num_sets = b.ue()?;
    if num_sets > 64 {
        return None;
    }
    let mut num_delta_pocs = Vec::new();
    for i in 0..num_sets {
        num_delta_pocs.push(st_ref_pic_set(b, i, &num_delta_pocs)?);
    }
    if b.flag()? {
        let n = b.ue()?;
        if n > 32 {
            return None;
        }
        for _ in 0..n {
            b.skip(log2_max_poc_lsb as usize)?;
            b.flag()?;
        }
    }
    b.flag()?;
    b.flag()?; // temporal mvp, strong intra smoothing
    let mut v = Vui::default();
    if !b.flag()? {
        return Some(v);
    }
    if b.flag()? && b.u(8)? == 255 {
        b.skip(32)?;
    }
    if b.flag()? {
        b.flag()?;
    }
    if b.flag()? {
        b.u(3)?; // video_format
        v.full_range_bit = Some(b.pos);
        v.full_range = Some(b.flag()?);
        if b.flag()? {
            v.colour_primaries = Some(b.u(8)? as u8);
            v.transfer = Some(b.u(8)? as u8);
            v.matrix = Some(b.u(8)? as u8);
        }
    }
    Some(v)
}

/// `nal` (an SPS) with `video_full_range_flag` set to `full`; `None` if it has no video signal
/// type to set (or does not parse).
pub fn set_full_range(nal: &[u8], full: bool) -> Option<Vec<u8>> {
    let bit = vui(nal)?.full_range_bit?;
    let mut r = rbsp(&nal[2..]);
    let (i, m) = (bit / 8, 0x80u8 >> (bit % 8));
    if full {
        r[i] |= m;
    } else {
        r[i] &= !m;
    }
    let mut out = nal[..2].to_vec();
    out.extend(ebsp(&r));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emulation_prevention_round_trips() {
        let raw = [0u8, 0, 0, 1, 0, 0, 3, 7, 0, 0, 2];
        let e = ebsp(&raw);
        assert_eq!(e, vec![0, 0, 3, 0, 1, 0, 0, 3, 3, 7, 0, 0, 3, 2]);
        assert_eq!(rbsp(&e), raw);
    }

    #[test]
    fn exp_golomb() {
        // 1 | 010 | 011 | 00100 | 00101 → ue 0,1,2,3 and se(4) = -2.
        let d = [0b1010_0110, 0b0100_0010, 0b1000_0000];
        let b = &mut Bits { d: &d, pos: 0 };
        assert_eq!(b.ue(), Some(0));
        assert_eq!(b.ue(), Some(1));
        assert_eq!(b.ue(), Some(2));
        assert_eq!(b.ue(), Some(3));
        assert_eq!(b.se(), Some(-2));
    }

    fn clip_sps() -> Vec<u8> {
        // The test clip's SPS, from the Mac's VideoToolbox (windows/testdata/clip.bin).
        let d = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/clip.bin")).unwrap();
        let ps_len = u32::from_be_bytes(d[16..20].try_into().unwrap()) as usize;
        let ps = sill_wire::ParameterSets::decode(&d[20..20 + ps_len]).unwrap();
        ps.sets.into_iter().find(|s| (s[0] >> 1) & 0x3F == 33).unwrap()
    }

    #[test]
    fn the_test_clips_sps_parses() {
        let v = vui(&clip_sps()).expect("parses");
        // Whatever it says, it must say something definite and round-trip through a rewrite.
        if let Some(full) = v.full_range {
            let flipped = set_full_range(&clip_sps(), !full).unwrap();
            let w = vui(&flipped).unwrap();
            assert_eq!(w.full_range, Some(!full));
            assert_eq!((w.colour_primaries, w.transfer, w.matrix), (v.colour_primaries, v.transfer, v.matrix));
            let back = set_full_range(&flipped, full).unwrap();
            assert_eq!(back, clip_sps());
        }
    }

    #[test]
    fn junk_does_not_parse() {
        assert_eq!(vui(&[]), None);
        assert_eq!(vui(&[0x42, 0x01]), None);
        assert_eq!(vui(&[0x40, 0x01, 0xFF]), None, "a VPS is not an SPS");
        assert_eq!(vui(&[0x42, 0x01, 0x01]), None, "truncated");
    }
}
