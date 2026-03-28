//! Edge overshoot metric: measure how much sharpening exceeds the local
//! edge-strength proxy (Sobel gradient magnitude).
//!
//! Uses cross-edge profiles from `edges.rs`. For each profile, finds the
//! peak excursion and compares it to the gradient magnitude.

use super::edges::EdgeProfile;

/// Compute the edge overshoot score from pre-extracted edge profiles.
///
/// Score = mean of `max(0, peak_excursion / gradient_magnitude - 1.0)`.
/// Returns 0.0 if `profiles` is empty or no overshoot is found.
pub fn edge_overshoot_score(profiles: &[EdgeProfile]) -> f32 {
    if profiles.is_empty() {
        return 0.0;
    }

    let total_overshoot: f32 = profiles
        .iter()
        .map(|p| {
            let peak = p
                .diff_samples
                .iter()
                .map(|v| v.abs())
                .fold(0.0_f32, f32::max);
            let ratio = peak / p.gradient_magnitude;
            (ratio - 1.0).max(0.0)
        })
        .sum();

    total_overshoot / profiles.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::edges::EdgeProfile;

    fn profile(grad_mag: f32, diff: [f32; 5]) -> EdgeProfile {
        EdgeProfile {
            gradient_magnitude: grad_mag,
            diff_samples: diff,
        }
    }

    #[test]
    fn empty_profiles_returns_zero() {
        assert_eq!(edge_overshoot_score(&[]), 0.0);
    }

    #[test]
    fn no_overshoot_when_diff_below_gradient() {
        let p = profile(0.1, [0.01, 0.02, 0.04, 0.02, 0.01]);
        assert_eq!(edge_overshoot_score(&[p]), 0.0);
    }

    #[test]
    fn exact_match_no_overshoot() {
        let p = profile(0.1, [0.01, 0.05, 0.1, 0.05, 0.01]);
        assert_eq!(edge_overshoot_score(&[p]), 0.0);
    }

    #[test]
    fn overshoot_detected() {
        let p = profile(0.1, [0.01, 0.05, 0.2, 0.05, 0.01]);
        assert!((edge_overshoot_score(&[p]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn negative_diff_counts_as_overshoot() {
        let p = profile(0.1, [0.01, -0.15, 0.05, 0.02, 0.01]);
        assert!((edge_overshoot_score(&[p]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mixed_profiles_averages_overshoot() {
        let p1 = profile(0.1, [0.01, 0.05, 0.2, 0.05, 0.01]);
        let p2 = profile(0.1, [0.01, 0.02, 0.04, 0.02, 0.01]);
        let score = edge_overshoot_score(&[p1, p2]);
        assert!((score - 0.5).abs() < 1e-6);
    }
}
