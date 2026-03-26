/// Formatted stdout output.
use imgsharp_core::AutoSharpDiagnostics;

/// Print a human-readable summary of the pipeline diagnostics to stdout.
pub fn print_summary(diag: &AutoSharpDiagnostics) {
    println!(
        "Selected sharpness strength : {:.4}",
        diag.selected_strength
    );
    println!(
        "Measured artifact ratio     : {:.6}  (target: {:.6})",
        diag.measured_artifact_ratio, diag.target_artifact_ratio
    );
    println!(
        "Polynomial fit              : {}",
        if diag.fit_coefficients.is_some() { "success" } else { "skipped / failed" }
    );
    println!(
        "Fallback used               : {}",
        if diag.fallback_used { "yes" } else { "no" }
    );
    if let Some(ref reason) = diag.fallback_reason {
        println!("Fallback reason             : {reason}");
    }
    println!(
        "Output size                 : {}×{}",
        diag.output_size.width, diag.output_size.height
    );
}
