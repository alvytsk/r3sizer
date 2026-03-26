use std::path::PathBuf;

/// Command-line arguments for `imgsharp`.
#[derive(clap::Parser, Debug)]
#[command(
    name = "imgsharp",
    about = "Downscale an image with automatic sharpness adjustment",
    long_about = "Downscales an image in linear RGB space and selects the sharpening \
                  strength automatically by fitting a cubic model P(s) of out-of-gamut \
                  artifact ratios, then solving for a target artifact threshold P0."
)]
pub struct Cli {
    /// Input image file (PNG, JPEG, BMP, …).
    #[arg(long, short = 'i', value_name = "FILE")]
    pub input: PathBuf,

    /// Output image file.  Format is inferred from the extension.
    #[arg(long, short = 'o', value_name = "FILE")]
    pub output: PathBuf,

    /// Target width in pixels.
    #[arg(long, short = 'W')]
    pub width: u32,

    /// Target height in pixels.
    #[arg(long, short = 'H')]
    pub height: u32,

    /// Target artifact ratio P0 (fraction of channel values outside [0,1]).
    /// Default: 0.001 (= 0.1 %).
    #[arg(long, default_value_t = 0.001)]
    pub target_artifact_ratio: f32,

    /// Path to write a JSON diagnostics file (optional).
    #[arg(long, value_name = "FILE")]
    pub diagnostics: Option<PathBuf>,

    /// Explicit comma-separated probe sharpening strengths, e.g. "0.5,1.0,2.0,3.0".
    /// When omitted, 9 evenly spaced values from 0.5 to 4.0 are used.
    #[arg(long, value_delimiter = ',', value_name = "S1,S2,...")]
    pub probe_strengths: Option<Vec<f32>>,

    /// Enable the contrast-leveling post-process stage (placeholder implementation).
    #[arg(long, default_value_t = false)]
    pub enable_contrast_leveling: bool,

    /// Gaussian sigma for the unsharp-mask sharpening filter.
    #[arg(long, default_value_t = 1.0)]
    pub sharpen_sigma: f32,
}
