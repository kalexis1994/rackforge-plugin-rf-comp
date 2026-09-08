//! The static transfer curve: how many decibels come off a level, once the
//! ballistics have settled.
//!
//! Above the knee the curve is the familiar line of slope `1/ratio` through
//! the threshold; below it, unity. Across the knee the two are joined by the
//! quadratic that meets each with the same slope, so the curve is continuous
//! in value and in slope and the reduction it asks for grows smoothly. This
//! is the textbook soft knee; at the threshold itself it takes away
//! `(1 - 1/ratio) * knee / 8`.

/// The reduction, in decibels at or above zero, for a level `level_db`.
///
/// `slope` is `1/ratio` — zero for a limiter — and `knee_db` the full width
/// of the knee, centred on the threshold.
#[inline]
pub fn reduction_db(level_db: f32, threshold_db: f32, slope: f32, knee_db: f32) -> f32 {
    let over = level_db - threshold_db;
    let half = 0.5 * knee_db;
    if over <= -half {
        0.0
    } else if over >= half {
        // Above the knee: the line of slope 1/ratio through the threshold.
        (1.0 - slope) * over
    } else {
        // Across the knee: the parabola that meets both lines tangentially.
        let into = over + half;
        (1.0 - slope) * into * into / (2.0 * knee_db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hard_knee_is_a_line_through_the_threshold() {
        assert_eq!(reduction_db(-40.0, -18.0, 0.25, 0.0), 0.0);
        assert_eq!(reduction_db(-18.0, -18.0, 0.25, 0.0), 0.0);
        assert!((reduction_db(-6.0, -18.0, 0.25, 0.0) - 9.0).abs() < 1.0e-5);
        assert!((reduction_db(0.0, -18.0, 0.0, 0.0) - 18.0).abs() < 1.0e-5);
    }

    #[test]
    fn a_ratio_of_one_takes_nothing() {
        for level in [-60.0, -18.0, -12.0, 0.0] {
            assert_eq!(reduction_db(level, -18.0, 1.0, 12.0), 0.0);
        }
    }

    #[test]
    fn the_soft_knee_is_continuous_monotonic_and_the_hard_curve_outside() {
        let (threshold, slope, knee) = (-18.0, 0.25, 12.0);
        let mut previous_output = f32::NEG_INFINITY;
        let mut previous_reduction = 0.0;
        let mut level = -60.0;
        while level <= 0.0 {
            let reduction = reduction_db(level, threshold, slope, knee);
            let output = level - reduction;
            assert!(output >= previous_output - 1.0e-4, "at {level}");
            assert!(reduction >= previous_reduction - 1.0e-4, "at {level}");
            assert!(
                (reduction - previous_reduction).abs() < 0.02,
                "a jump at {level}"
            );
            if (level - threshold).abs() >= knee / 2.0 {
                let hard = reduction_db(level, threshold, slope, 0.0);
                assert!((reduction - hard).abs() < 1.0e-4, "at {level}");
            }
            previous_output = output;
            previous_reduction = reduction;
            level += 0.01;
        }
        // At the threshold the parabola takes (1 - 1/R) * knee / 8.
        let at_threshold = reduction_db(threshold, threshold, slope, knee);
        assert!((at_threshold - (1.0 - slope) * knee / 8.0).abs() < 0.1);
    }
}
