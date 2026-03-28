//! Halo/ringing metric: detect sign-alternating oscillations near strong edges.
//!
//! Uses cross-edge profiles from `edges.rs`. A profile exhibits ringing if it
//! contains 2+ sign changes (ignoring values below a noise floor epsilon).

use super::edges::EdgeProfile;

/// Epsilon floor: diff samples with |value| below this are treated as zero.
const NOISE_FLOOR: f32 = 1e-5;

/// Minimum sign changes in a 5-sample profile to classify as ringing.
const RINGING_THRESHOLD: usize = 2;

/// Compute the halo ringing score from pre-extracted edge profiles.
///
/// Score = (profiles with ringing) / (total profiles).
/// Returns 0.0 if `profiles` is empty.
pub fn halo_ringing_score(profiles: &[EdgeProfile]) -> f32 {
    if profiles.is_empty() {
        return 0.0;
    }

    let ringing_count = profiles
        .iter()
        .filter(|p| has_ringing(&p.diff_samples))
        .count();

    ringing_count as f32 / profiles.len() as f32
}

/// Check if a diff profile has sign-alternating oscillations.
fn has_ringing(samples: &[f32; 5]) -> bool {
    let mut sign_changes = 0usize;
    let mut prev_sign: Option<bool> = None;

    for &v in samples {
        if v.abs() < NOISE_FLOOR {
            continue;
        }
        let positive = v > 0.0;
        if let Some(prev) = prev_sign {
            if positive != prev {
                sign_changes += 1;
            }
        }
        prev_sign = Some(positive);
    }

    sign_changes >= RINGING_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::edges::EdgeProfile;

    fn profile(diff: [f32; 5]) -> EdgeProfile {
        EdgeProfile {
            gradient_magnitude: 0.5,
            diff_samples: diff,
        }
    }

    #[test]
    fn empty_profiles_returns_zero() {
        assert_eq!(halo_ringing_score(&[]), 0.0);
    }

    #[test]
    fn no_ringing_monotonic_diff() {
        let p = profile([0.01, 0.02, 0.05, 0.08, 0.1]);
        assert_eq!(halo_ringing_score(&[p]), 0.0);
    }

    #[test]
    fn one_sign_change_not_ringing() {
        let p = profile([-0.05, -0.02, 0.01, 0.03, 0.05]);
        assert_eq!(halo_ringing_score(&[p]), 0.0);
    }

    #[test]
    fn two_sign_changes_is_ringing() {
        let p = profile([0.05, -0.03, 0.02, -0.01, 0.005]);
        assert_eq!(halo_ringing_score(&[p]), 1.0);
    }

    #[test]
    fn noise_floor_suppresses_false_positives() {
        let p = profile([0.05, 1e-6, -1e-6, 1e-6, 0.03]);
        assert_eq!(halo_ringing_score(&[p]), 0.0);
    }

    #[test]
    fn mixed_profiles_correct_ratio() {
        let ringing = profile([0.05, -0.03, 0.02, -0.01, 0.005]);
        let clean = profile([0.01, 0.02, 0.05, 0.08, 0.1]);
        let score = halo_ringing_score(&[ringing, clean]);
        assert!((score - 0.5).abs() < 1e-6);
    }

    #[test]
    fn all_below_noise_floor_not_ringing() {
        let p = profile([1e-6, -1e-7, 1e-8, -1e-9, 1e-6]);
        assert_eq!(halo_ringing_score(&[p]), 0.0);
    }
}
