//! The 12-digit typed code: 11 random digits and a Damm check digit (upstream `PairingCode`).

/// Damm's quasigroup of order 10.
const TABLE: [[u8; 10]; 10] = [
    [0, 3, 1, 7, 5, 9, 8, 6, 4, 2],
    [7, 0, 9, 2, 1, 5, 4, 8, 6, 3],
    [4, 2, 0, 6, 8, 7, 1, 3, 5, 9],
    [1, 7, 5, 0, 9, 8, 3, 4, 2, 6],
    [6, 1, 2, 3, 0, 4, 5, 9, 7, 8],
    [3, 6, 7, 4, 2, 0, 9, 5, 8, 1],
    [5, 8, 6, 9, 7, 2, 0, 1, 3, 4],
    [8, 9, 4, 5, 3, 6, 2, 0, 1, 7],
    [9, 4, 3, 8, 6, 1, 7, 2, 0, 5],
    [2, 5, 8, 1, 4, 3, 6, 7, 9, 0],
];

pub const LENGTH: usize = 12;

/// The check digit for `digits` (each 0–9; panics otherwise). A valid code ends at 0.
pub fn check_digit(digits: &[u8]) -> u8 {
    digits.iter().fold(0, |interim, &d| TABLE[interim as usize][d as usize])
}

/// A fresh code from `random`, which fills a buffer with secure random bytes and returns false
/// if it cannot. Rejection sampling (bytes of 250 or more are drawn again) keeps every digit
/// equally likely. `None` only if `random` fails.
pub fn generate(mut random: impl FnMut(&mut [u8]) -> bool) -> Option<String> {
    let mut digits: Vec<u8> = Vec::with_capacity(LENGTH);
    while digits.len() < LENGTH - 1 {
        let mut bytes = [0u8; 16];
        if !random(&mut bytes) {
            return None;
        }
        for b in bytes {
            if b < 250 && digits.len() < LENGTH - 1 {
                digits.push(b % 10);
            }
        }
    }
    digits.push(check_digit(&digits));
    Some(digits.iter().map(|d| char::from(b'0' + d)).collect())
}

/// Why typed input is not a code yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeProblem {
    /// "A code has 12 digits."
    Length,
    /// "That code has a typo."
    Typo,
}

/// `input` as a code: spaces, hyphens and no-break spaces removed, exactly 12 ASCII digits, the
/// check digit valid.
pub fn check(input: &str) -> Result<String, CodeProblem> {
    let kept: String = input.chars().filter(|&c| c != ' ' && c != '-' && c != '\u{00A0}').collect();
    if kept.chars().count() != LENGTH || !kept.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CodeProblem::Length);
    }
    let digits: Vec<u8> = kept.bytes().map(|b| b - b'0').collect();
    if check_digit(&digits) != 0 {
        return Err(CodeProblem::Typo);
    }
    Ok(kept)
}

/// "4829 1355 7208".
pub fn grouped(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    chars.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_uses_bytes_below_250_only() {
        let mut calls = 0;
        let code = generate(|buf| {
            calls += 1;
            // 250..=255 must be skipped; 0..=249 map to b % 10.
            for (i, b) in buf.iter_mut().enumerate() {
                *b = if i % 2 == 0 { 250 + (i as u8 % 6) } else { (i as u8) * 7 };
            }
            true
        })
        .unwrap();
        assert_eq!(code.len(), 12);
        assert!(check(&code).is_ok());
        assert_eq!(calls, 2, "8 usable bytes per buffer, 11 digits needed");
        assert_eq!(&code[..11], "71593715715");
        assert!(generate(|_| false).is_none());
    }

    #[test]
    fn damm_catches_every_substitution_and_adjacent_transposition() {
        let base = "48291355720";
        let digits: Vec<u8> = base.bytes().map(|b| b - b'0').collect();
        let mut code = digits.clone();
        code.push(check_digit(&digits));
        for i in 0..12 {
            for d in 0..10u8 {
                if d != code[i] {
                    let mut c = code.clone();
                    c[i] = d;
                    assert_ne!(check_digit(&c), 0);
                }
            }
            if i < 11 && code[i] != code[i + 1] {
                let mut c = code.clone();
                c.swap(i, i + 1);
                assert_ne!(check_digit(&c), 0);
            }
        }
    }
}
