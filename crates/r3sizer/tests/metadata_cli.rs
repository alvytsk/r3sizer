/// CLI regression tests for metadata-loss stderr warnings.
///
/// Metadata issues are stderr-only: they must never appear in `diag.json`,
/// stdout JSON, or sweep `summary.json`.
use std::path::PathBuf;

use assert_cmd::Command;
use tempfile::TempDir;

fn cmd() -> Command {
    Command::cargo_bin("r3sizer").expect("binary not found")
}

// ---------------------------------------------------------------------------
// Step 1: `process` reports metadata loss only on stderr.
// ---------------------------------------------------------------------------

#[test]
fn process_reports_metadata_loss_only_on_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("input.jpg");
    let dst = dir.path().join("output.png");
    let diag = dir.path().join("diag.json");
    std::fs::write(
        &src,
        include_bytes!("../../r3sizer-metadata/tests/fixtures/metadata-with-makernote.jpg"),
    )
    .unwrap();
    let output = assert_cmd::Command::cargo_bin("r3sizer")
        .unwrap()
        .args(["process", "-i"])
        .arg(&src)
        .arg("-o")
        .arg(&dst)
        .args(["--width", "16", "--height", "8", "--output-format", "json"])
        .arg("--diagnostics")
        .arg(&diag)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("maker_note"));
    assert!(stderr.contains(dst.to_str().unwrap()));
    let stdout: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(diag).unwrap()).unwrap();
    assert_eq!(stdout, saved);
    assert!(saved.get("metadata").is_none());
    assert!(saved.get("metadata_issues").is_none());
}

// ---------------------------------------------------------------------------
// Step 4: sweep with two inputs -- one warning-bearing, one clean.
// ---------------------------------------------------------------------------

/// Write a tiny plain (no-metadata) RGB PNG to `path`.
fn write_plain_png(path: &PathBuf) {
    let img = image::RgbImage::from_fn(32, 16, |x, y| {
        image::Rgb([(x * 4) as u8, (y * 8) as u8, 128])
    });
    img.save(path).expect("failed to write plain PNG fixture");
}

#[test]
fn sweep_with_out_dir_warns_per_file_without_affecting_aggregate() {
    let dir = TempDir::new().unwrap();
    let in_dir = dir.path().join("in");
    let out_dir = dir.path().join("out");
    std::fs::create_dir_all(&in_dir).unwrap();
    let summary_path = dir.path().join("summary.json");

    let makernote_src = in_dir.join("with_makernote.jpg");
    std::fs::write(
        &makernote_src,
        include_bytes!("../../r3sizer-metadata/tests/fixtures/metadata-with-makernote.jpg"),
    )
    .unwrap();
    let plain_src = in_dir.join("plain.png");
    write_plain_png(&plain_src);

    let output = cmd()
        .args(["sweep", "--in-dir"])
        .arg(&in_dir)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--summary")
        .arg(&summary_path)
        .args(["--width", "16", "--height", "8"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sweep failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("maker_note"));
    let expected_output_path = out_dir.join("with_makernote.png");
    assert!(stderr.contains(expected_output_path.to_str().unwrap()));

    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
    assert_eq!(summary["aggregate"]["succeeded"], 2);
    assert_eq!(summary["aggregate"]["failed"], 0);
    assert_eq!(summary["errors"].as_array().unwrap().len(), 0);

    // No new top-level keys were introduced by metadata handling: exactly
    // the pre-existing summary/result JSON key sets.
    let mut summary_keys: Vec<&str> = summary
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    summary_keys.sort_unstable();
    assert_eq!(summary_keys, vec!["aggregate", "errors", "results"]);

    let results = summary["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    let mut result_keys: Vec<&str> = results[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    result_keys.sort_unstable();
    let mut expected_result_keys = vec![
        "input",
        "output",
        "selected_strength",
        "selection_mode",
        "fallback_reason",
        "measured_artifact_ratio",
        "measured_metric_value",
        "fit_r_squared",
        "monotonic",
        "total_us",
        "gamut_excursion",
        "halo_ringing",
        "edge_overshoot",
        "texture_flattening",
        "composite_score",
        "ringing_score",
        "envelope_scale",
        "edge_retention",
        "texture_retention",
        "effective_target_artifact_ratio",
        "chroma_clamped_fraction",
        "chroma_effective_threshold_mean",
    ];
    expected_result_keys.sort_unstable();
    assert_eq!(result_keys, expected_result_keys);

    let mut aggregate_keys: Vec<&str> = summary["aggregate"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    aggregate_keys.sort_unstable();
    let mut expected_aggregate_keys = vec![
        "total_files",
        "succeeded",
        "failed",
        "mean_selected_strength",
        "median_selected_strength",
        "mean_total_us",
        "selection_mode_counts",
        "fit_success_rate",
        "gamut_excursion",
        "halo_ringing",
        "edge_overshoot",
        "texture_flattening",
        "composite_score",
        "ringing_score",
        "envelope_scale",
        "edge_retention",
        "texture_retention",
        "effective_target_artifact_ratio",
        "chroma_clamped_fraction",
    ];
    expected_aggregate_keys.sort_unstable();
    assert_eq!(aggregate_keys, expected_aggregate_keys);

    assert!(out_dir.join("with_makernote.png").exists());
    assert!(out_dir.join("plain.png").exists());
}

#[test]
fn sweep_without_out_dir_emits_no_metadata_warning_and_no_images() {
    let dir = TempDir::new().unwrap();
    let in_dir = dir.path().join("in");
    std::fs::create_dir_all(&in_dir).unwrap();
    let summary_path = dir.path().join("summary.json");

    let makernote_src = in_dir.join("with_makernote.jpg");
    std::fs::write(
        &makernote_src,
        include_bytes!("../../r3sizer-metadata/tests/fixtures/metadata-with-makernote.jpg"),
    )
    .unwrap();
    let plain_src = in_dir.join("plain.png");
    write_plain_png(&plain_src);

    let output = cmd()
        .args(["sweep", "--in-dir"])
        .arg(&in_dir)
        .arg("--summary")
        .arg(&summary_path)
        .args(["--width", "16", "--height", "8"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sweep failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains("metadata"));

    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
    assert_eq!(summary["aggregate"]["succeeded"], 2);
    assert_eq!(summary["aggregate"]["failed"], 0);
    assert_eq!(summary["errors"].as_array().unwrap().len(), 0);

    for entry in summary["results"].as_array().unwrap() {
        assert!(entry["output"].is_null());
    }
    // No output directory was created, so no image files exist anywhere
    // under the sweep's temp dir besides the sources we wrote ourselves.
    let png_and_jpg_outside_in_dir = walk_images(dir.path())
        .into_iter()
        .filter(|p| !p.starts_with(&in_dir))
        .count();
    assert_eq!(png_and_jpg_outside_in_dir, 0);
}

/// A real decode failure is still recorded as a per-file failure (in
/// `errors`/`aggregate.failed`), not silently swallowed or misclassified as
/// a metadata issue -- sweep mode's overall exit code stays success as long
/// as the run itself completes, matching pre-existing behavior.
#[test]
fn sweep_real_decode_failure_still_fails() {
    let dir = TempDir::new().unwrap();
    let in_dir = dir.path().join("in");
    std::fs::create_dir_all(&in_dir).unwrap();
    let summary_path = dir.path().join("summary.json");
    // A file with an image extension but garbage bytes: real decode failure.
    std::fs::write(in_dir.join("broken.png"), b"not a real png").unwrap();

    cmd()
        .args(["sweep", "--in-dir"])
        .arg(&in_dir)
        .arg("--summary")
        .arg(&summary_path)
        .args(["--width", "16", "--height", "8"])
        .assert()
        .success();

    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
    assert_eq!(summary["aggregate"]["succeeded"], 0);
    assert_eq!(summary["aggregate"]["failed"], 1);
    assert_eq!(summary["errors"].as_array().unwrap().len(), 1);
}

fn walk_images(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk_images(&path));
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("png") | Some("jpg") | Some("jpeg")
            ) {
                out.push(path);
            }
        }
    }
    out
}
