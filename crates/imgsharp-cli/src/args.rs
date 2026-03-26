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
    /// Input image file (PNG, JPEG, BMP, ...).
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

    /// Explicit comma-separated probe sharpening strengths, e.g. "0.05,0.1,0.2,0.4,0.8,1.5,3.0".
    /// When omitted, a non-uniform default list dense near zero is used.
    #[arg(long, value_delimiter = ',', value_name = "S1,S2,...")]
    pub probe_strengths: Option<Vec<f32>>,

    /// Enable the contrast-leveling post-process stage (placeholder implementation).
    #[arg(long, default_value_t = false)]
    pub enable_contrast_leveling: bool,

    /// Gaussian sigma for the unsharp-mask sharpening filter.
    #[arg(long, default_value_t = 1.0)]
    pub sharpen_sigma: f32,

    /// Sharpening mode: "rgb" (sharpen all channels) or "lightness" (sharpen CIE Y,
    /// reconstruct RGB via multiplicative ratio). Default: lightness.
    #[arg(long, default_value = "lightness")]
    pub sharpen_mode: SharpenModeArg,

    /// Metric mode: "absolute" (total artifact ratio) or "relative" (artifacts added
    /// by sharpening, subtracting baseline from resize). Default: relative.
    #[arg(long, default_value = "relative")]
    pub metric_mode: MetricModeArg,
}

// ---------------------------------------------------------------------------
// CLI-friendly wrappers for core enums (avoids adding clap dep to core)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SharpenModeArg {
    Rgb,
    Lightness,
}

impl From<SharpenModeArg> for imgsharp_core::SharpenMode {
    fn from(val: SharpenModeArg) -> Self {
        match val {
            SharpenModeArg::Rgb => imgsharp_core::SharpenMode::Rgb,
            SharpenModeArg::Lightness => imgsharp_core::SharpenMode::Lightness,
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum MetricModeArg {
    Absolute,
    Relative,
}

impl From<MetricModeArg> for imgsharp_core::MetricMode {
    fn from(val: MetricModeArg) -> Self {
        match val {
            MetricModeArg::Absolute => imgsharp_core::MetricMode::AbsoluteTotal,
            MetricModeArg::Relative => imgsharp_core::MetricMode::RelativeToBase,
        }
    }
}
