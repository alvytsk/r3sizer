//! Two-phase pipeline example: prepare once, process multiple times.
//!
//! Usage:
//!   cargo run --example two_phase --manifest-path crates/r3sizer-io/Cargo.toml \
//!       -- input.jpg out_fast.png out_balanced.png out_quality.png 800 600
//!
//! Demonstrates the two-phase API: `prepare_base` runs the expensive resize +
//! classify + baseline stage once; `process_from_prepared` runs the faster
//! probing + fit + sharpen stage with different `PipelineMode` settings.
use std::path::Path;

use r3sizer_core::prelude::*;
use r3sizer_io::{load_as_linear, save_from_linear};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 7 {
        eprintln!(
            "Usage: two_phase <input> <out_fast> <out_balanced> <out_quality> <width> <height>"
        );
        std::process::exit(1);
    }
    let width: u32 = args[5].parse()?;
    let height: u32 = args[6].parse()?;

    // 1. Load image once.
    let src = load_as_linear(Path::new(&args[1]))?;
    println!("Loaded {}×{} from {:?}", src.width(), src.height(), &args[1]);

    // 2. Prepare the base once — resize, classify, baseline measurement.
    //    This is the expensive step (~1.5 s on a 24 MP image).
    let base_params = AutoSharpParams::photo(width, height);
    let prepared = prepare_base(&src, &base_params, &|stage| {
        eprint!("  [{stage}] ");
    })?;
    eprintln!();
    println!(
        "Base prepared: {}×{}  baseline P = {:.4}",
        prepared.base_width(),
        prepared.base_height(),
        prepared.baseline_artifact_ratio()
    );

    // 3. Process with each PipelineMode, reusing the cached base.
    let modes: [(PipelineMode, &str, &str); 3] = [
        (PipelineMode::Fast, "Fast", &args[2]),
        (PipelineMode::Balanced, "Balanced", &args[3]),
        (PipelineMode::Quality, "Quality", &args[4]),
    ];

    for (mode, label, out_path) in &modes {
        // .resolved() applies the mode's probe-count and strategy overrides.
        let params = AutoSharpParams {
            pipeline_mode: Some(*mode),
            ..AutoSharpParams::photo(width, height)
        }
        .resolved();

        // Verify the prepared base is still valid for these params.
        assert!(
            prepared.matches_params(&params),
            "Base params changed — call prepare_base again"
        );

        let result = process_from_prepared(&prepared, &params, &|_| {})?;
        let diag = &result.diagnostics;
        println!(
            "{label:10} s* = {:.4}  selection = {:?}",
            diag.selected_strength, diag.selection_mode
        );
        save_from_linear(&result.image, Path::new(out_path))?;
        println!("  → {out_path}");
    }

    Ok(())
}
