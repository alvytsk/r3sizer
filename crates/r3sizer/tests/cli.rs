/// CLI integration tests using `assert_cmd`.
///
/// Each test spins up the `r3sizer` binary compiled from this workspace and
/// validates exit codes, stdout content, and produced files.
use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn cmd() -> Command {
    Command::cargo_bin("r3sizer").expect("binary not found")
}

/// Write a tiny 64×64 sRGB PNG to `path`.
fn write_test_png(path: &PathBuf) {
    // Build a minimal RGB image: simple gradient so the pipeline has something
    // to work with (flat images may be degenerate for some metrics).
    let w = 64u32;
    let h = 64u32;
    let mut img = image::RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let r = (x * 4).min(255) as u8;
            let g = (y * 4).min(255) as u8;
            let b = 128u8;
            img.put_pixel(x, y, image::Rgb([r, g, b]));
        }
    }
    img.save(path).expect("failed to write test fixture PNG");
}

// ---------------------------------------------------------------------------
// `process` subcommand
// ---------------------------------------------------------------------------

#[test]
fn process_basic_succeeds() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in.png");
    let output = dir.path().join("out.png");
    write_test_png(&input);

    cmd()
        .args([
            "process",
            "-i",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "-W",
            "32",
            "-H",
            "32",
        ])
        .assert()
        .success();

    assert!(output.exists(), "output image was not written");
}

#[test]
fn process_json_output_is_valid_json() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in.png");
    let output = dir.path().join("out.png");
    write_test_png(&input);

    let out = cmd()
        .args([
            "process",
            "-i",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "-W",
            "32",
            "-H",
            "32",
            "--output-format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(out).unwrap();
    let _parsed: serde_json::Value =
        serde_json::from_str(&text).expect("stdout was not valid JSON");
}

#[test]
fn process_missing_input_fails() {
    let dir = TempDir::new().unwrap();
    let output = dir.path().join("out.png");

    cmd()
        .args([
            "process",
            "-i",
            "/nonexistent/path/image.png",
            "-o",
            output.to_str().unwrap(),
            "-W",
            "32",
            "-H",
            "32",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
fn process_missing_dimensions_fails() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in.png");
    let output = dir.path().join("out.png");
    write_test_png(&input);

    // Neither --width nor --height provided.
    cmd()
        .args([
            "process",
            "-i",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .failure();
}

#[test]
fn process_preserve_aspect_ratio() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in.png");
    let output = dir.path().join("out.png");
    write_test_png(&input); // 64×64

    // Provide only width; aspect ratio should produce a matching height.
    cmd()
        .args([
            "process",
            "-i",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "-W",
            "32",
            "--preserve-aspect-ratio",
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// `presets` subcommand
// ---------------------------------------------------------------------------

#[test]
fn presets_list_contains_photo() {
    cmd()
        .args(["presets", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("photo"))
        .stdout(predicate::str::contains("precision"));
}

#[test]
fn presets_show_photo_is_valid_json() {
    let out = cmd()
        .args(["presets", "show", "photo"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(out).unwrap();
    // The first line is "Preset 'photo':", the JSON follows.
    let json_start = text.find('{').expect("no JSON object in output");
    let _parsed: serde_json::Value =
        serde_json::from_str(&text[json_start..]).expect("output is not valid JSON");
}

#[test]
fn presets_show_unknown_fails() {
    cmd()
        .args(["presets", "show", "not-a-preset"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

// ---------------------------------------------------------------------------
// `diff` subcommand
// ---------------------------------------------------------------------------

/// Write a minimal valid SweepSummary JSON to `path`.
fn write_minimal_summary(path: &PathBuf) {
    let summary = serde_json::json!({
        "aggregate": {
            "total_files": 0,
            "succeeded": 0,
            "failed": 0,
            "mean_selected_strength": 0.0,
            "median_selected_strength": 0.0,
            "mean_total_us": 0.0,
            "selection_mode_counts": {
                "polynomial_root": 0,
                "best_sample_within_budget": 0,
                "least_bad_sample": 0,
                "budget_unreachable": 0
            },
            "fit_success_rate": 0.0,
            "gamut_excursion": { "mean": 0.0, "median": 0.0, "p90": 0.0, "p95": 0.0 },
            "halo_ringing": { "mean": 0.0, "median": 0.0, "p90": 0.0, "p95": 0.0 },
            "edge_overshoot": { "mean": 0.0, "median": 0.0, "p90": 0.0, "p95": 0.0 },
            "texture_flattening": { "mean": 0.0, "median": 0.0, "p90": 0.0, "p95": 0.0 },
            "composite_score": { "mean": 0.0, "median": 0.0, "p90": 0.0, "p95": 0.0 },
            "ringing_score": { "mean": 0.0, "median": 0.0, "p90": 0.0, "p95": 0.0 },
            "envelope_scale": { "mean": 0.0, "median": 0.0, "p90": 0.0, "p95": 0.0 },
            "edge_retention": { "mean": 0.0, "median": 0.0, "p90": 0.0, "p95": 0.0 },
            "texture_retention": { "mean": 0.0, "median": 0.0, "p90": 0.0, "p95": 0.0 },
            "effective_target_artifact_ratio": { "mean": 0.0, "median": 0.0, "p90": 0.0, "p95": 0.0 },
            "chroma_clamped_fraction": { "mean": 0.0, "median": 0.0, "p90": 0.0, "p95": 0.0 }
        },
        "results": [],
        "errors": []
    });
    std::fs::write(path, serde_json::to_string_pretty(&summary).unwrap()).unwrap();
}

#[test]
fn diff_identical_summaries_succeeds() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("baseline.json");
    let b = dir.path().join("candidate.json");
    write_minimal_summary(&a);
    write_minimal_summary(&b);

    cmd()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("No significant changes"));
}

#[test]
fn diff_missing_file_fails() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("baseline.json");
    write_minimal_summary(&a);

    cmd()
        .args(["diff", a.to_str().unwrap(), "/nonexistent/candidate.json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}
