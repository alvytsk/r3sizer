/// Least-squares cubic polynomial fitting.
///
/// Given N ≥ 4 probe samples `(s_i, P_i)`, fits:
/// ```text
/// P_hat(s) = a·s³ + b·s² + c·s + d
/// ```
///
/// **Method:** Vandermonde normal equations with Gaussian elimination (partial
/// pivoting, f64 arithmetic for numerical stability).
///
/// The 4×4 normal equations are:
/// ```text
/// (Vᵀ V) · [a, b, c, d]ᵀ  =  Vᵀ · P
/// ```
/// where V is the N×4 Vandermonde matrix `V[k][j] = s_k^(3-j)`.
///
/// All arithmetic uses `f64` because the normal-equation matrix contains
/// terms up to `s^6`.  For s = 4.0, `s^6 = 4096`, which accumulates in
/// double-precision with acceptable numerical error.  Using `f32` here
/// would cause catastrophic cancellation in the Gaussian elimination.
use crate::{CoreError, CubicPolynomial, ProbeSample};

/// Fit a cubic polynomial to the given probe samples.
///
/// Returns `Err(FitFailed)` if:
/// - fewer than 4 samples are provided,
/// - all strength values are identical (degenerate data),
/// - the normal-equation matrix is numerically singular (pivot < 1e-14).
pub fn fit_cubic(samples: &[ProbeSample]) -> Result<CubicPolynomial, CoreError> {
    if samples.len() < 4 {
        return Err(CoreError::FitFailed(format!(
            "need at least 4 probe samples for cubic fit, got {}",
            samples.len()
        )));
    }

    // Build 4×4 normal equations in f64.
    // Coefficient order: [a (s^3), b (s^2), c (s^1), d (s^0)]
    // AtA[i][j] = Σ_k  s_k^(i+j)   for i,j in 0..4 (powers 0..=6)
    // Atb[i]    = Σ_k  s_k^i * P_k  for i in 0..4

    let mut ata = [[0.0f64; 4]; 4];
    let mut atb = [0.0f64; 4];

    for s in samples {
        let x = s.strength as f64;
        let p = s.artifact_ratio as f64;

        // Precompute powers x^0 .. x^6
        let mut xp = [1.0f64; 7];
        for k in 1..7 {
            xp[k] = xp[k - 1] * x;
        }

        // AtA is indexed by polynomial-degree order (0 = s^0 = constant).
        // We want the coefficient vector [d, c, b, a] (ascending power).
        for i in 0..4 {
            atb[i] += xp[i] * p;
            for j in 0..4 {
                ata[i][j] += xp[i + j];
            }
        }
    }

    // Solve AtA · x = Atb using Gaussian elimination with partial pivoting.
    let coeffs = gauss_solve(ata, atb)?;

    // coeffs order: [d, c, b, a] (ascending power of s).
    Ok(CubicPolynomial {
        a: coeffs[3],
        b: coeffs[2],
        c: coeffs[1],
        d: coeffs[0],
    })
}

/// Solve a 4×4 linear system `A · x = b` via Gaussian elimination with partial
/// pivoting.  Returns `Err(FitFailed)` if the matrix is (near-)singular.
#[allow(clippy::needless_range_loop)]
fn gauss_solve(mut a: [[f64; 4]; 4], mut b: [f64; 4]) -> Result<[f64; 4], CoreError> {
    const N: usize = 4;
    const PIVOT_EPSILON: f64 = 1e-14;

    for col in 0..N {
        // --- Partial pivot: find the row with the largest absolute value in
        //     column `col` at or below the current row. ---
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
                "normal equations are numerically singular; try more varied probe strengths".into(),
            ));
        }

        // Swap rows.
        if max_row != col {
            a.swap(col, max_row);
            b.swap(col, max_row);
        }

        // Eliminate below.
        for row in (col + 1)..N {
            let factor = a[row][col] / a[col][col];
            for k in col..N {
                a[row][k] -= factor * a[col][k];
            }
            b[row] -= factor * b[col];
        }
    }

    // Back substitution.
    let mut x = [0.0f64; N];
    for i in (0..N).rev() {
        let mut s = b[i];
        for j in (i + 1)..N {
            s -= a[i][j] * x[j];
        }
        x[i] = s / a[i][i];
    }

    Ok(x)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    /// Build probe samples from a known cubic at the given strengths.
    fn samples_from_cubic(
        a: f64, b: f64, c: f64, d: f64,
        strengths: &[f32],
    ) -> Vec<ProbeSample> {
        strengths
            .iter()
            .map(|&s| {
                let x = s as f64;
                let p = a * x.powi(3) + b * x.powi(2) + c * x + d;
                ProbeSample { strength: s, artifact_ratio: p as f32 }
            })
            .collect()
    }

    #[test]
    fn recovers_known_cubic_exactly() {
        // P_hat(s) = 0.002·s³ - 0.001·s² + 0.0005·s + 0.0001
        let (a, b, c, d) = (0.002, -0.001, 0.0005, 0.0001);
        let strengths = [0.5f32, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5];
        let samples = samples_from_cubic(a, b, c, d, &strengths);
        let poly = fit_cubic(&samples).unwrap();

        // Coefficients should match to high precision (within f32→f64 casting noise).
        assert_abs_diff_eq!(poly.a, a, epsilon = 1e-6);
        assert_abs_diff_eq!(poly.b, b, epsilon = 1e-6);
        assert_abs_diff_eq!(poly.c, c, epsilon = 1e-6);
        assert_abs_diff_eq!(poly.d, d, epsilon = 1e-6);
    }

    #[test]
    fn minimum_four_samples() {
        let strengths = [0.5f32, 1.0, 2.0, 3.0];
        let samples = samples_from_cubic(0.001, 0.0, 0.0, 0.0, &strengths);
        assert!(fit_cubic(&samples).is_ok());
    }

    #[test]
    fn fewer_than_four_samples_returns_error() {
        let samples = vec![
            ProbeSample { strength: 1.0, artifact_ratio: 0.0 },
            ProbeSample { strength: 2.0, artifact_ratio: 0.001 },
            ProbeSample { strength: 3.0, artifact_ratio: 0.005 },
        ];
        assert!(matches!(fit_cubic(&samples), Err(CoreError::FitFailed(_))));
    }

    #[test]
    fn evaluates_correctly_at_sample_points() {
        let (a, b, c, d) = (0.0015, -0.0005, 0.0002, 0.00005);
        let strengths = [0.5f32, 1.0, 2.0, 3.0, 4.0];
        let samples = samples_from_cubic(a, b, c, d, &strengths);
        let poly = fit_cubic(&samples).unwrap();

        for s in &strengths {
            let expected = a * (*s as f64).powi(3)
                + b * (*s as f64).powi(2)
                + c * (*s as f64)
                + d;
            let got = poly.evaluate(*s as f64);
            assert_abs_diff_eq!(got, expected, epsilon = 1e-5);
        }
    }
}
