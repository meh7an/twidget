//! Sine and cosine of an angle in degrees, the same bits on every platform: exact at multiples of
//! 90°, and otherwise reduced to 0°–45° and evaluated by fixed Taylor polynomials, whose error
//! there is far below the last bit.

use core::f64::consts::PI;

/// Radians per degree.
const RADIANS: f64 = PI / 180.0;

/// `sin x = x (1 + x² (S₀ + x² (S₁ + …)))`, through the x¹⁷ term.
const SIN: [f64; 8] = [
    -1.0 / 6.0,
    1.0 / 120.0,
    -1.0 / 5_040.0,
    1.0 / 362_880.0,
    -1.0 / 39_916_800.0,
    1.0 / 6_227_020_800.0,
    -1.0 / 1_307_674_368_000.0,
    1.0 / 355_687_428_096_000.0,
];

/// `cos x = 1 + x² (C₀ + x² (C₁ + …))`, through the x¹⁸ term.
const COS: [f64; 9] = [
    -1.0 / 2.0,
    1.0 / 24.0,
    -1.0 / 720.0,
    1.0 / 40_320.0,
    -1.0 / 3_628_800.0,
    1.0 / 479_001_600.0,
    -1.0 / 87_178_291_200.0,
    1.0 / 20_922_789_888_000.0,
    -1.0 / 6_402_373_705_728_000.0,
];

/// The sine and cosine of `degrees`.
pub(super) fn sin_cos(degrees: f64) -> (f64, f64) {
    let mut turned = degrees % 360.0;
    if turned < 0.0 {
        turned += 360.0;
    }
    // Rounding can land a tiny negative angle on 360, and a non-finite one reads as 0.
    if !(0.0..360.0).contains(&turned) {
        turned = 0.0;
    }
    // Subtracting a whole number of quarter turns here is exact, by Sterbenz's lemma.
    let (quarter, angle) = if turned < 90.0 {
        (0, turned)
    } else if turned < 180.0 {
        (1, turned - 90.0)
    } else if turned < 270.0 {
        (2, turned - 180.0)
    } else {
        (3, turned - 270.0)
    };
    let (sin, cos) = if angle == 0.0 {
        (0.0, 1.0)
    } else if angle <= 45.0 {
        octant(angle)
    } else {
        let (sin, cos) = octant(90.0 - angle);
        (cos, sin)
    };
    // `0.0 - x` rather than `-x`, so an exact zero stays positive.
    match quarter {
        0 => (sin, cos),
        1 => (cos, 0.0 - sin),
        2 => (0.0 - sin, 0.0 - cos),
        _ => (0.0 - cos, sin),
    }
}

/// The sine and cosine of an angle from 0° to 45°.
fn octant(degrees: f64) -> (f64, f64) {
    let x = degrees * RADIANS;
    let x2 = x * x;
    (x * series(&SIN, x2), series(&COS, x2))
}

/// `1 + x² (c₀ + x² (c₁ + …))`, by Horner's rule.
fn series(coefficients: &[f64], x2: f64) -> f64 {
    coefficients
        .iter()
        .rev()
        .fold(0.0, |sum, coefficient| (sum + coefficient) * x2)
        + 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sin_cos_exact_at_quarter_turns() {
        for (degrees, expected) in [
            (0.0, (0.0, 1.0)),
            (90.0, (1.0, 0.0)),
            (180.0, (0.0, -1.0)),
            (270.0, (-1.0, 0.0)),
            (360.0, (0.0, 1.0)),
            (-90.0, (-1.0, 0.0)),
            (450.0, (1.0, 0.0)),
            (-720.0, (0.0, 1.0)),
        ] {
            let (sin, cos) = sin_cos(degrees);
            assert_eq!((sin, cos), expected, "{degrees}");
            assert!(sin.is_sign_positive() || sin != 0.0, "{degrees}: sin is -0");
            assert!(cos.is_sign_positive() || cos != 0.0, "{degrees}: cos is -0");
        }
    }

    #[test]
    fn test_sin_cos_match_the_platform_within_rounding() {
        let mut degrees = -721.0;
        while degrees < 721.0 {
            let (sin, cos) = sin_cos(degrees);
            // Turning degrees into radians rounds the reference by up to about 1e-15 at 180°, and
            // by more at larger angles, so it starts from the angle reduced to ±180°, which `%`
            // and the subtraction do exactly, and the tolerance allows for the rest.
            let mut reduced = degrees % 360.0;
            if reduced > 180.0 {
                reduced -= 360.0;
            } else if reduced < -180.0 {
                reduced += 360.0;
            }
            let radians = reduced.to_radians();
            assert!((sin - radians.sin()).abs() < 2e-15, "sin {degrees}");
            assert!((cos - radians.cos()).abs() < 2e-15, "cos {degrees}");
            degrees += 0.37;
        }
    }
}
