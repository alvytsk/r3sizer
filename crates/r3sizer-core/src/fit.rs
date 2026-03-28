/// Least-squares cubic polynomial fitting.
///
/// Given N >= 4 data points `(x_i, y_i)`, fits:
/// ```text
/// P_hat(x) = a*x^3 + b*x^2 + c*x + d
/// ```
///
/// **Method:** Vandermonde normal equations with Gaussian elimination (partial
/// pivoting, f64 arithmetic for numerical stability).
///
/// The 4x4 normal equations are:
/// ```text
/// (V^T V) * [a, b, c, d]^T  =  V^T * y
/// ```
/// where V is the N x 4 Vandermonde matrix `V[k][j] = x_k^(3-j)`.
///
/// All arithmetic uses `f64` because the normal-equation matrix contains
/// terms up to `x^6`.  For x = 4.0, `x^6 = 4096`, which accumulates in
/// double-precision with acceptable numerical error.  Using `f32` here
/// would cause catastrophic cancellation in the Gaussian elimination.
use crate::{CoreError, CubicPolynomial, FitQuality};

/// Fit a cubic polynomial to the given (x, y) data points.
///
/// Returns `Err(FitFailed)` if:
/// - fewer than 4 points are provided,
/// - all x values are identical (degenerate data),
/// - the normal-equation matrix is numerically singular (pivot < 1e-14).
pub fn fit_cubic(data: &[(f64, f64)]) -> Result<CubicPolynomial, CoreError> {
    if data.len() < 4 {
        return Err(CoreError::FitFailed(format!(
            "need at least 4 data points for cubic fit, got {}",
            data.len()
        )));
    }

    // Build 4x4 normal equations in f64.
    // Coefficient order: [a (x^3), b (x^2), c (x^1), d (x^0)]
    // AtA[i][j] = sum_k  x_k^(i+j)   for i,j in 0..4 (powers 0..=6)
    // Atb[i]    = sum_k  x_k^i * y_k  for i in 0..4

    let mut ata = [[0.0f64; 4]; 4];
    let mut atb = [0.0f64; 4];

    for &(x, y) in data {
        // Precompute powers x^0 .. x^6
        let mut xp = [1.0f64; 7];
        for k in 1..7 {
            xp[k] = xp[k - 1] * x;
        }

        // AtA is indexed by polynomial-degree order (0 = x^0 = constant).
        // We want the coefficient vector [d, c, b, a] (ascending power).
        for i in 0..4 {
            atb[i] += xp[i] * y;
            for j in 0..4 {
                ata[i][j] += xp[i + j];
            }
        }
    }

    // Solve AtA * x = Atb using Gaussian elimination with partial pivoting.
    let coeffs = gauss_solve(ata, atb)?;

    // coeffs order: [d, c, b, a] (ascending power of x).
    Ok(CubicPolynomial {
        a: coeffs[3],
        b: coeffs[2],
        c: coeffs[1],
        d: coeffs[0],
    })
}

/// Fit a cubic polynomial and return both the polynomial and quality metrics.
///
/// Same fitting algorithm as [`fit_cubic`], but additionally computes:
/// - residual sum of squares
/// - R² (coefficient of determination)
/// - maximum absolute residual
/// - minimum pivot encountered during Gaussian elimination
pub fn fit_cubic_with_quality(data: &[(f64, f64)]) -> Result<(CubicPolynomial, FitQuality), CoreError> {
    if data.len() < 4 {
        return Err(CoreError::FitFailed(format!(
            "need at least 4 data points for cubic fit, got {}",
            data.len()
        )));
    }

    let mut ata = [[0.0f64; 4]; 4];
    let mut atb = [0.0f64; 4];

    for &(x, y) in data {
        let mut xp = [1.0f64; 7];
        for k in 1..7 {
            xp[k] = xp[k - 1] * x;
        }
        for i in 0..4 {
            atb[i] += xp[i] * y;
            for j in 0..4 {
                ata[i][j] += xp[i + j];
            }
        }
    }

    let (coeffs, min_pivot) = gauss_solve_with_pivots(ata, atb)?;

    let poly = CubicPolynomial {
        a: coeffs[3],
        b: coeffs[2],
        c: coeffs[1],
        d: coeffs[0],
    };

    // Compute residuals.
    let n = data.len() as f64;
    let y_mean = data.iter().map(|&(_, y)| y).sum::<f64>() / n;

    let mut ss_res = 0.0f64;
    let mut ss_tot = 0.0f64;
    let mut max_residual = 0.0f64;

    for &(x, y) in data {
        let y_hat = poly.evaluate(x);
        let residual = (y - y_hat).abs();
        ss_res += (y - y_hat) * (y - y_hat);
        ss_tot += (y - y_mean) * (y - y_mean);
        if residual > max_residual {
            max_residual = residual;
        }
    }

    let r_squared = if ss_tot > 1e-30 { 1.0 - ss_res / ss_tot } else { 1.0 };

    let quality = FitQuality {
        residual_sum_of_squares: ss_res,
        r_squared,
        max_residual,
        min_pivot,
    };

    Ok((poly, quality))
}

/// Check whether probe metric values are monotonically non-decreasing.
///
/// Returns `(monotonic, quasi_monotonic)`:
/// - `monotonic`: true if no inversions (each value >= previous).
/// - `quasi_monotonic`: true if at most 1 inversion.
///
/// Assumes `samples` are sorted by strength (ascending).
pub fn check_monotonicity(samples: &[crate::ProbeSample]) -> (bool, bool) {
    let mut inversions = 0usize;
    for w in samples.windows(2) {
        if w[1].metric_value < w[0].metric_value {
            inversions += 1;
        }
    }
    (inversions == 0, inversions <= 1)
}

/// Solve a 4x4 linear system `A * x = b` via Gaussian elimination with partial
/// pivoting.  Returns `Err(FitFailed)` if the matrix is (near-)singular.
fn gauss_solve(a: [[f64; 4]; 4], b: [f64; 4]) -> Result<[f64; 4], CoreError> {
    gauss_solve_with_pivots(a, b).map(|(x, _)| x)
}

/// Like `gauss_solve`, but also returns the minimum pivot magnitude encountered.
#[allow(clippy::needless_range_loop)]
fn gauss_solve_with_pivots(
    mut a: [[f64; 4]; 4],
    mut b: [f64; 4],
) -> Result<([f64; 4], f64), CoreError> {
    const N: usize = 4;
    const PIVOT_EPSILON: f64 = 1e-14;

    let mut min_pivot = f64::INFINITY;

    for col in 0..N {
        let mut max_row = col;
        let mut max_val = a[col][col].abs();
        for row in (col + 1)..N {
            if a[row][col].abs() > max_val {
                max_val = a[row][col].abs();
                max_row = row;
            }
        }

        if max_val < PIVOT_EPSILON {
            return Err(CoreError::FitFailed(
                "normal equations are numerically singular; try more varied probe strengths"
                    .into(),
            ));
        }

        if max_val < min_pivot {
            min_pivot = max_val;
        }

        if max_row != col {
            a.swap(col, max_row);
            b.swap(col, max_row);
        }

        for row in (col + 1)..N {
            let factor = a[row][col] / a[col][col];
            for k in col..N {
                a[row][k] -= factor * a[col][k];
            }
            b[row] -= factor * b[col];
        }
    }

    let mut x = [0.0f64; N];
    for i in (0..N).rev() {
        let mut s = b[i];
        for j in (i + 1)..N {
            s -= a[i][j] * x[j];
        }
        x[i] = s / a[i][i];
    }

    Ok((x, min_pivot))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    /// Build (x, y) data points from a known cubic at the given x values.
    fn data_from_cubic(a: f64, b: f64, c: f64, d: f64, xs: &[f64]) -> Vec<(f64, f64)> {
        xs.iter()
            .map(|&x| {
                let y = a * x.powi(3) + b * x.powi(2) + c * x + d;
                (x, y)
            })
            .collect()
    }

    #[test]
    fn recovers_known_cubic_exactly() {
        // P_hat(s) = 0.002*s^3 - 0.001*s^2 + 0.0005*s + 0.0001
        let (a, b, c, d) = (0.002, -0.001, 0.0005, 0.0001);
        let xs = [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5];
        let data = data_from_cubic(a, b, c, d, &xs);
        let poly = fit_cubic(&data).unwrap();

        assert_abs_diff_eq!(poly.a, a, epsilon = 1e-6);
        assert_abs_diff_eq!(poly.b, b, epsilon = 1e-6);
        assert_abs_diff_eq!(poly.c, c, epsilon = 1e-6);
        assert_abs_diff_eq!(poly.d, d, epsilon = 1e-6);
    }

    #[test]
    fn minimum_four_samples() {
        let xs = [0.5, 1.0, 2.0, 3.0];
        let data = data_from_cubic(0.001, 0.0, 0.0, 0.0, &xs);
        assert!(fit_cubic(&data).is_ok());
    }

    #[test]
    fn fewer_than_four_samples_returns_error() {
        let data = vec![(1.0, 0.0), (2.0, 0.001), (3.0, 0.005)];
        assert!(matches!(fit_cubic(&data), Err(CoreError::FitFailed(_))));
    }

    #[test]
    fn evaluates_correctly_at_sample_points() {
        let (a, b, c, d) = (0.0015, -0.0005, 0.0002, 0.00005);
        let xs = [0.5, 1.0, 2.0, 3.0, 4.0];
        let data = data_from_cubic(a, b, c, d, &xs);
        let poly = fit_cubic(&data).unwrap();

        for &x in &xs {
            let expected = a * x.powi(3) + b * x.powi(2) + c * x + d;
            let got = poly.evaluate(x);
            assert_abs_diff_eq!(got, expected, epsilon = 1e-5);
        }
    }

    #[test]
    fn fits_with_zero_anchor() {
        // Simulate RelativeToBase mode: (0,0) anchor + probe points.
        let (a, b, c, d) = (0.001, 0.002, 0.0005, 0.0);
        let xs = [0.0, 0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0];
        let data = data_from_cubic(a, b, c, d, &xs);
        let poly = fit_cubic(&data).unwrap();

        // d should be near zero since the curve passes through origin.
        assert_abs_diff_eq!(poly.d, 0.0, epsilon = 1e-6);
        assert_abs_diff_eq!(poly.a, a, epsilon = 1e-4);
    }

    // --- fit_cubic_with_quality tests ---

    #[test]
    fn quality_perfect_fit_has_zero_residuals() {
        let (a, b, c, d) = (0.002, -0.001, 0.0005, 0.0001);
        let xs = [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0];
        let data = data_from_cubic(a, b, c, d, &xs);
        let (poly, quality) = fit_cubic_with_quality(&data).unwrap();

        assert_abs_diff_eq!(poly.a, a, epsilon = 1e-6);
        assert!(quality.residual_sum_of_squares < 1e-20);
        assert!(quality.r_squared > 0.9999);
        assert!(quality.max_residual < 1e-10);
        assert!(quality.min_pivot > 0.0);
    }

    #[test]
    fn quality_noisy_data_has_lower_r_squared() {
        // Cubic + noise: R² should be < 1 but still reasonable.
        let xs: [f64; 8] = [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0];
        let noise: [f64; 8] = [0.001, -0.002, 0.003, -0.001, 0.002, -0.003, 0.001, -0.002];
        let data: Vec<(f64, f64)> = xs
            .iter()
            .zip(noise.iter())
            .map(|(&x, &n)| {
                let y = 0.002 * x.powi(3) - 0.001 * x.powi(2) + 0.0005 * x + 0.0001 + n;
                (x, y)
            })
            .collect();
        let (_, quality) = fit_cubic_with_quality(&data).unwrap();

        assert!(quality.r_squared < 1.0);
        assert!(quality.residual_sum_of_squares > 0.0);
        assert!(quality.max_residual > 0.0);
    }

    #[test]
    fn quality_min_pivot_is_positive() {
        let xs = [0.5, 1.0, 2.0, 3.0];
        let data = data_from_cubic(0.001, 0.0, 0.0, 0.0, &xs);
        let (_, quality) = fit_cubic_with_quality(&data).unwrap();
        assert!(quality.min_pivot > 0.0);
    }

    // --- check_monotonicity tests ---

    #[test]
    fn monotonic_samples_detected() {
        let samples = vec![
            crate::ProbeSample { strength: 0.5, artifact_ratio: 0.001, metric_value: 0.001, breakdown: None },
            crate::ProbeSample { strength: 1.0, artifact_ratio: 0.002, metric_value: 0.002, breakdown: None },
            crate::ProbeSample { strength: 2.0, artifact_ratio: 0.005, metric_value: 0.005, breakdown: None },
            crate::ProbeSample { strength: 3.0, artifact_ratio: 0.010, metric_value: 0.010, breakdown: None },
        ];
        let (mono, quasi) = check_monotonicity(&samples);
        assert!(mono);
        assert!(quasi);
    }

    #[test]
    fn non_monotonic_samples_detected() {
        let samples = vec![
            crate::ProbeSample { strength: 0.5, artifact_ratio: 0.005, metric_value: 0.005, breakdown: None },
            crate::ProbeSample { strength: 1.0, artifact_ratio: 0.002, metric_value: 0.002, breakdown: None },
            crate::ProbeSample { strength: 2.0, artifact_ratio: 0.008, metric_value: 0.008, breakdown: None },
            crate::ProbeSample { strength: 3.0, artifact_ratio: 0.003, metric_value: 0.003, breakdown: None },
        ];
        let (mono, quasi) = check_monotonicity(&samples);
        assert!(!mono);
        assert!(!quasi); // 2 inversions
    }

    #[test]
    fn quasi_monotonic_one_inversion() {
        let samples = vec![
            crate::ProbeSample { strength: 0.5, artifact_ratio: 0.001, metric_value: 0.001, breakdown: None },
            crate::ProbeSample { strength: 1.0, artifact_ratio: 0.003, metric_value: 0.003, breakdown: None },
            crate::ProbeSample { strength: 2.0, artifact_ratio: 0.002, metric_value: 0.002, breakdown: None },
            crate::ProbeSample { strength: 3.0, artifact_ratio: 0.010, metric_value: 0.010, breakdown: None },
        ];
        let (mono, quasi) = check_monotonicity(&samples);
        assert!(!mono);
        assert!(quasi); // exactly 1 inversion
    }
}
