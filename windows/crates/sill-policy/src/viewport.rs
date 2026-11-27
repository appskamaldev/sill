//! What a host takes from a device's viewport (upstream `ViewportBounds`), and the number
//! formatting its log lines use.

use sill_wire::Viewport;

pub const SIDE_MIN: f64 = 1.0;
pub const SIDE_MAX: f64 = 16384.0;
pub const SCALE_MIN: f64 = 0.25;
pub const SCALE_MAX: f64 = 4.0;

fn in_range(x: f64, lo: f64, hi: f64) -> bool {
    lo <= x && x <= hi
}

/// Why a host refuses `v` ("width 1e+300 (1 to 16384 points)", joined by ", "); `None` when it
/// takes it. A NaN is in no range. A refused viewport is ignored whole.
pub fn refusal(v: &Viewport) -> Option<String> {
    let points = format!("({} to {} points)", number(SIDE_MIN), number(SIDE_MAX));
    let mut why = Vec::new();
    if !in_range(v.width, SIDE_MIN, SIDE_MAX) {
        why.push(format!("width {} {points}", number(v.width)));
    }
    if !in_range(v.height, SIDE_MIN, SIDE_MAX) {
        why.push(format!("height {} {points}", number(v.height)));
    }
    if let Some(s) = v.scale {
        if !in_range(s, SCALE_MIN, SCALE_MAX) {
            why.push(format!("scale {} ({} to {})", number(s), number(SCALE_MIN), number(SCALE_MAX)));
        }
    }
    (!why.is_empty()).then(|| why.join(", "))
}

/// What a device sends for `v`: sides and scale held to the ranges; a scale that is not a
/// positive number becomes none; `None` while a side is not a positive number.
pub fn held(v: &Viewport) -> Option<Viewport> {
    if !(v.width.is_finite() && v.height.is_finite() && v.width > 0.0 && v.height > 0.0) {
        return None;
    }
    let mut h = v.clone();
    h.width = v.width.clamp(SIDE_MIN, SIDE_MAX);
    h.height = v.height.clamp(SIDE_MIN, SIDE_MAX);
    h.scale = v.scale.filter(|s| s.is_finite() && *s > 0.0).map(|s| s.clamp(SCALE_MIN, SCALE_MAX));
    Some(h)
}

/// `Int(exactly:)`.
fn exact_int(x: f64) -> Option<i64> {
    (x.is_finite() && x.trunc() == x && (-(2f64.powi(63))..2f64.powi(63)).contains(&x)).then_some(x as i64)
}

/// A number as `Int(x)` prints it, toward zero, or as Swift prints the Double when no Int holds it.
pub fn whole(x: f64) -> String {
    exact_int(x.trunc()).map_or_else(|| swift_double(x), |i| i.to_string())
}

/// A whole number without ".0", any other as Swift prints the Double.
pub fn number(x: f64) -> String {
    exact_int(x).map_or_else(|| swift_double(x), |i| i.to_string())
}

/// Swift's `Double.description`: shortest round-trip digits; fixed notation for 1e-4 ≤ |x| ≤ 2^53
/// (and zero) with ".0" for whole numbers, else exponential with a signed, two-digit-minimum
/// exponent ("1e+16", "1e-05"); "nan", "inf", "-inf".
pub fn swift_double(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let a = x.abs();
    if a == 0.0 || (1e-4..=9_007_199_254_740_992.0).contains(&a) {
        let s = format!("{x}");
        return if s.contains('.') { s } else { format!("{s}.0") };
    }
    let e = format!("{x:e}");
    let (mantissa, exp) = e.split_once('e').expect("LowerExp has an e");
    let exp: i32 = exp.parse().expect("an exponent");
    format!("{mantissa}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
}
