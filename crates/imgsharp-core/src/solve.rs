/// Cubic root finding and sharpening-strength selection.
///
/// Given a cubic polynomial P_hat(s) and a target P0, solves:
/// ```text
/// P_hat(s*) = P0   ⟺   a·s³ + b·s² + c·s + (d - P0) = 0
/// ```
///
/// **Method:** Cardano's formula via the depressed-cubic substitution, with
/// the trigonometric method when all three roots are real.
///
/// **Root selection policy (confirmed from paper intent):**
/// Choose the *largest* root within `[s_min, s_max]` — this maximises
/// sharpening strength while staying within the artifact budget.
///
/// **Fallback** (recorded in diagnostics, never a hard error):
/// 1. Among probe samples with `artifact_ratio ≤ P0`, pick the one with the
///    largest strength.
/// 2. If none qualify, pick the sample with the smallest `artifact_ratio`
///    (least-bad option).
use std::f64::consts::PI;

use crate::{CoreError, CubicPolynomial, ProbeSample};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Find the optimal sharpening strength.
///
/// Returns `(selected_strength, fallback_used, fallback_reason)`.
///
/// Never panics and never returns `Err` unless `probe_samples` is empty and
/// the polynomial path also fails (in which case there is genuinely nothing to
/// select from).
pub fn find_sharpness(
    poly: &CubicPolynomial,
    target_p0: f64,
    s_min: f64,
    s_max: f64,
    probe_samples: &[ProbeSample],
) -> Result<(f32, bool, Option<String>), CoreError> {
    // --- Attempt algebraic root finding ---
    match roots_in_range(poly, target_p0, s_min, s_max) {
        Ok(roots) if !roots.is_empty() => {
            // Pick the largest root (maximise sharpness within artifact budget).
            let s_star = roots.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            return Ok((s_star as f32, false, None));
        }
        Ok(_) => {
            // Roots found but none are in range.
        }
        Err(_) => {
            // Degenerate polynomial (linear/constant); fall through to sampling.
        }
    }

    // --- Fallback: best result from probe samples ---
    fallback_from_samples(probe_samples, target_p0 as f32, s_min as f32, s_max as f32)
}

// ---------------------------------------------------------------------------
// Algebraic root finding
// ---------------------------------------------------------------------------

/// Find all real roots of `P_hat(s) = target_p0` that lie in `[s_min, s_max]`.
///
/// Returns an empty `Vec` (not an error) when no in-range roots exist.
/// Returns `Err` only when the polynomial is too degenerate to solve.
fn roots_in_range(
    poly: &CubicPolynomial,
    p0: f64,
    s_min: f64,
    s_max: f64,
) -> Result<Vec<f64>, CoreError> {
    let a = poly.a;
    let b = poly.b;
    let c = poly.c;
    let d = poly.d - p0; // shift constant term

    let all_roots = if a.abs() < 1e-12 {
        // Degenerate: treat as quadratic or linear.
        solve_quadratic_or_lower(b, c, d)?
    } else {
        solve_cubic(a, b, c, d)
    };

    // Filter to range and remove any NaN/infinity that might slip through.
    let in_range = all_roots
        .into_iter()
        .filter(|r| r.is_finite() && *r >= s_min && *r <= s_max)
        .collect();

    Ok(in_range)
}

/// Solve `a·s³ + b·s² + c·s + d = 0` (|a| > 0).
/// Returns up to 3 real roots.
fn solve_cubic(a: f64, b: f64, c: f64, d: f64) -> Vec<f64> {
    // Depress the cubic: substitute s = t - b/(3a) to get t³ + p·t + q = 0.
    let p = (3.0 * a * c - b * b) / (3.0 * a * a);
    let q = (2.0 * b * b * b - 9.0 * a * b * c + 27.0 * a * a * d) / (27.0 * a * a * a);
    let shift = b / (3.0 * a);

    // Discriminant of the depressed cubic.
    let discriminant = -(4.0 * p * p * p + 27.0 * q * q);

    if discriminant > 0.0 {
        // Three distinct real roots — trigonometric method.
        let m = 2.0 * (-p / 3.0_f64).sqrt();
        let inner = 3.0 * q / (p * m);
        // Clamp to [-1, 1] to guard against floating-point noise at boundary.
        let inner = inner.clamp(-1.0, 1.0);
        let theta = (1.0 / 3.0) * inner.acos();
        vec![
            m * (theta).cos() - shift,
            m * (theta - 2.0 * PI / 3.0).cos() - shift,
            m * (theta - 4.0 * PI / 3.0).cos() - shift,
        ]
    } else if discriminant.abs() < 1e-14 {
        // Repeated root(s).
        if p.abs() < 1e-14 && q.abs() < 1e-14 {
            // Triple root at t = 0.
            vec![-shift]
        } else {
            // Double root and one simple root.
            // For t³ + p·t + q = 0: double root at t = -3q/(2p), simple at t = 3q/p.
            let t_double = -3.0 * q / (2.0 * p);
            let t_simple = 3.0 * q / p;
            vec![t_double - shift, t_simple - shift]
        }
    } else {
        // One real root — Cardano's formula.
        let sqrt_inner = (q * q / 4.0 + p * p * p / 27.0).sqrt();
        let u_arg = -q / 2.0 + sqrt_inner;
        let v_arg = -q / 2.0 - sqrt_inner;
        let u = cbrt(u_arg);
        let v = cbrt(v_arg);
        vec![u + v - shift]
    }
}

/// Solve `b·s² + c·s + d = 0` (b may be zero, falling back further).
fn solve_quadratic_or_lower(b: f64, c: f64, d: f64) -> Result<Vec<f64>, CoreError> {
    if b.abs() < 1e-12 {
        // Linear: c·s + d = 0
        if c.abs() < 1e-12 {
            return Err(CoreError::FitFailed("polynomial is effectively constant".into()));
        }
        return Ok(vec![-d / c]);
    }
    // Quadratic.
    let disc = c * c - 4.0 * b * d;
    if disc < 0.0 {
        return Ok(vec![]);
    }
    let sq = disc.sqrt();
    Ok(vec![(-c + sq) / (2.0 * b), (-c - sq) / (2.0 * b)])
}

/// Real cube root (handles negative radicand).
#[inline]
fn cbrt(x: f64) -> f64 {
    if x < 0.0 { -(-x).cbrt() } else { x.cbrt() }
}

// ---------------------------------------------------------------------------
// Fallback: direct selection from probe samples
// ---------------------------------------------------------------------------

fn fallback_from_samples(
    samples: &[ProbeSample],
    p0: f32,
    _s_min: f32,
    _s_max: f32,
) -> Result<(f32, bool, Option<String>), CoreError> {
    if samples.is_empty() {
        return Err(CoreError::NoValidRoot {
            reason: "no probe samples available for fallback selection".into(),
        });
    }

    // Best case: pick largest strength with P(s) ≤ P0.
    let qualifying: Vec<&ProbeSample> =
        samples.iter().filter(|s| s.artifact_ratio <= p0).collect();

    if let Some(best) = qualifying.iter().max_by(|a, b| {
        a.strength.partial_cmp(&b.strength).unwrap()
    }) {
        return Ok((
            best.strength,
            true,
            Some("no cubic root in probe range; selected largest probe sample within artifact budget".into()),
        ));
    }

    // Worst case: all samples exceed P0 — pick the one with the smallest ratio.
    let least_bad = samples
        .iter()
        .min_by(|a, b| a.artifact_ratio.partial_cmp(&b.artifact_ratio).unwrap())
        .unwrap(); // safe: samples non-empty

    Ok((
        least_bad.strength,
        true,
        Some(format!(
            "all probe samples exceed artifact budget (min ratio = {:.4}); \
             selected sample with smallest artifact ratio",
            least_bad.artifact_ratio
        )),
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn poly(a: f64, b: f64, c: f64, d: f64) -> CubicPolynomial {
        CubicPolynomial { a, b, c, d }
    }

    fn make_samples(strengths: &[f32], ratios: &[f32]) -> Vec<ProbeSample> {
        strengths
            .iter()
            .zip(ratios.iter())
            .map(|(&strength, &artifact_ratio)| ProbeSample { strength, artifact_ratio })
            .collect()
    }

    #[test]
    fn single_root_found_in_range() {
        // (s - 2)^3 = 0 → root at s=2
        // Expanded: s³ - 6s² + 12s - 8
        let p = poly(1.0, -6.0, 12.0, -8.0);
        let samples = make_samples(&[0.5, 1.0, 2.0, 3.0], &[0.0, 0.0, 0.0, 0.0]);
        let (s, fallback, _) = find_sharpness(&p, 0.0, 0.5, 4.0, &samples).unwrap();
        assert_abs_diff_eq!(s as f64, 2.0, epsilon = 1e-4);
        assert!(!fallback);
    }

    #[test]
    fn largest_root_selected_when_multiple() {
        // (s-1)(s-2)(s-3) = s³ - 6s² + 11s - 6
        // P_hat(s) = 0 has roots at 1, 2, 3.
        // We want P_hat(s) = 0.0, so target_p0 = 0.0.
        let p = poly(1.0, -6.0, 11.0, -6.0);
        let samples = make_samples(&[0.5, 1.5, 2.5, 3.5], &[0.0; 4]);
        let (s, fallback, _) = find_sharpness(&p, 0.0, 0.5, 4.0, &samples).unwrap();
        // Should select root 3 (largest in [0.5, 4.0]).
        assert_abs_diff_eq!(s as f64, 3.0, epsilon = 1e-3);
        assert!(!fallback);
    }

    #[test]
    fn no_root_in_range_triggers_fallback() {
        // P_hat(s) = 0 roots are at 1, 2, 3 (outside [3.5, 4.0]).
        let p = poly(1.0, -6.0, 11.0, -6.0);
        // Probe samples all below P0 = 0.001.
        let samples = make_samples(
            &[3.5, 3.7, 3.9, 4.0],
            &[0.0001, 0.0002, 0.0003, 0.0004],
        );
        let (_, fallback, reason) = find_sharpness(&p, 0.001, 3.5, 4.0, &samples).unwrap();
        assert!(fallback);
        assert!(reason.is_some());
    }

    #[test]
    fn fallback_picks_largest_qualifying_sample() {
        let samples = make_samples(
            &[1.0, 2.0, 3.0, 4.0],
            &[0.0002, 0.0005, 0.001, 0.005], // only first three ≤ 0.001
        );
        // Use a degenerate poly (constant) to force fallback.
        let p = poly(0.0, 0.0, 0.0, 0.5); // P_hat(s) = 0.5 for all s → no root near 0.001
        let (s, fallback, _) = find_sharpness(&p, 0.001, 1.0, 4.0, &samples).unwrap();
        assert!(fallback);
        // Should pick strength 3.0 (largest with ratio ≤ 0.001).
        assert_abs_diff_eq!(s, 3.0_f32, epsilon = 1e-5);
    }

    #[test]
    fn all_samples_exceed_budget_picks_least_bad() {
        let samples = make_samples(
            &[1.0, 2.0, 3.0, 4.0],
            &[0.01, 0.02, 0.03, 0.04],
        );
        let p = poly(0.0, 0.0, 0.0, 0.5);
        let (s, fallback, _) = find_sharpness(&p, 0.001, 1.0, 4.0, &samples).unwrap();
        assert!(fallback);
        // Should pick strength 1.0 (smallest artifact ratio = 0.01).
        assert_abs_diff_eq!(s, 1.0_f32, epsilon = 1e-5);
    }
}
