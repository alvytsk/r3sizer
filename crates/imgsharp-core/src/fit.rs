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
use crate::{CoreError, CubicPolynomial};

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

/// Solve a 4x4 linear system `A * x = b` via Gaussian elimination with partial
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
}
