//! Single-file resize + sharpen example.
//!
//! Usage:
//!   cargo run --example single_file --manifest-path crates/r3sizer-io/Cargo.toml \
//!       -- input.jpg output.png 800 600
//!
//! Loads `input.jpg`, downscales to 800×600 using the Photo preset,
//! applies automatic sharpening, and writes the result to `output.png`.
use std::path::Path;

use r3sizer_core::prelude::*;
use r3sizer_io::{load_as_linear, save_from_linear};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!("Usage: single_file <input> <output> <width> <height>");
        std::process::exit(1);
    }
    let (input_path, output_path) = (Path::new(&args[1]), Path::new(&args[2]));
    let width: u32 = args[3].parse()?;
    let height: u32 = args[4].parse()?;

    // 1. Load from file — decodes sRGB to linear-light f32 RGB.
    let src = load_as_linear(input_path)?;
    println!("Loaded {}×{} from {:?}", src.width(), src.height(), input_path);

    // 2. Build parameters from the Photo preset.
    //    .resolved() applies any pipeline_mode override before pipeline entry.
    let params = AutoSharpParams::photo(width, height).resolved();

    // 3. Run the full pipeline in a single call.
    let result = process_auto_sharp_downscale(&src, &params)?;
    let diag = &result.diagnostics;
    println!(
        "s* = {:.4}  target P0 = {:.4}  selection = {:?}",
        diag.selected_strength, diag.target_artifact_ratio, diag.selection_mode
    );

    // 4. Save — converts linear RGB back to sRGB before writing.
    save_from_linear(&result.image, output_path)?;
    println!("Saved {:?}", output_path);

    Ok(())
}
