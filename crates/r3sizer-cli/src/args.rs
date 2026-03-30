use std::path::PathBuf;

/// Command-line arguments for `r3sizer`.
#[derive(clap::Parser, Debug)]
#[command(
    name = "r3sizer",
    about = "Downscale an image with automatic sharpness adjustment",
    long_about = "Downscales an image in linear RGB space and selects the sharpening \
                  strength automatically by fitting a cubic model P(s) of out-of-gamut \
                  artifact ratios, then solving for a target artifact threshold P0."
)]
pub struct Cli {
    /// Input image file (PNG, JPEG, BMP, ...).
    /// Not required when --sweep-dir or --generate-corpus is used.
    #[arg(long, short = 'i', value_name = "FILE", required_unless_present_any = ["sweep_dir", "generate_corpus", "sweep_diff"])]
    pub input: Option<PathBuf>,

    /// Output image file.  Format is inferred from the extension.
    /// Not required when --sweep-dir or --generate-corpus is used.
    #[arg(long, short = 'o', value_name = "FILE", required_unless_present_any = ["sweep_dir", "generate_corpus", "sweep_diff"])]
    pub output: Option<PathBuf>,

    /// Target width in pixels.
    #[arg(long, short = 'W')]
    pub width: Option<u32>,

    /// Target height in pixels.
    #[arg(long, short = 'H')]
    pub height: Option<u32>,

    /// Preserve the input image's aspect ratio. When set, only one of
    /// --width or --height is required; the other is computed automatically.
    #[arg(long, short = 'p')]
    pub preserve_aspect_ratio: bool,

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

    /// Artifact metric: "channel-clipping" (default) or "pixel-out-of-gamut".
    #[arg(long, default_value = "channel-clipping")]
    pub artifact_metric: ArtifactMetricArg,

    /// Composite metric weights as W1,W2,W3,W4.
    /// Order: gamut_excursion, halo_ringing, edge_overshoot, texture_flattening.
    #[arg(long, value_delimiter = ',', value_name = "W1,W2,W3,W4")]
    pub metric_weights: Option<Vec<f32>>,

    /// Diagnostics verbosity: "summary" (final breakdown only) or "full" (per-probe breakdowns).
    #[arg(long, default_value = "summary")]
    pub diagnostics_level: DiagnosticsLevelArg,

    /// Named pipeline preset. Overrides sharpen strategy, chroma guard, and evaluator settings.
    /// Available: baseline, v3-adaptive, v5-full, v5-two-pass.
    #[arg(long, value_name = "NAME")]
    pub preset: Option<String>,

    // --- Sweep mode ---

    /// Directory of images to process in batch mode. Mutually exclusive with --input/--output.
    #[arg(long, value_name = "DIR")]
    pub sweep_dir: Option<PathBuf>,

    /// Output directory for processed images in sweep mode.
    #[arg(long, value_name = "DIR", requires = "sweep_dir")]
    pub sweep_output_dir: Option<PathBuf>,

    /// Path to write the sweep summary JSON file.
    #[arg(long, value_name = "FILE", requires = "sweep_dir")]
    pub sweep_summary: Option<PathBuf>,

    // --- Corpus generation ---

    /// Generate a synthetic benchmark corpus in the given directory and exit.
    #[arg(long, value_name = "DIR")]
    pub generate_corpus: Option<PathBuf>,

    // --- Sweep comparison ---

    /// Compare two sweep summary JSON files: baseline,candidate.
    /// Produces a diff report showing per-file and aggregate changes.
    #[arg(long, value_delimiter = ',', num_args = 2, value_name = "BASE,CANDIDATE")]
    pub sweep_diff: Option<Vec<PathBuf>>,
}

// ---------------------------------------------------------------------------
// CLI-friendly wrappers for core enums (avoids adding clap dep to core)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SharpenModeArg {
    Rgb,
    Lightness,
}

impl From<SharpenModeArg> for r3sizer_core::SharpenMode {
    fn from(val: SharpenModeArg) -> Self {
        match val {
            SharpenModeArg::Rgb => r3sizer_core::SharpenMode::Rgb,
            SharpenModeArg::Lightness => r3sizer_core::SharpenMode::Lightness,
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum MetricModeArg {
    Absolute,
    Relative,
}

impl From<MetricModeArg> for r3sizer_core::MetricMode {
    fn from(val: MetricModeArg) -> Self {
        match val {
            MetricModeArg::Absolute => r3sizer_core::MetricMode::AbsoluteTotal,
            MetricModeArg::Relative => r3sizer_core::MetricMode::RelativeToBase,
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ArtifactMetricArg {
    ChannelClipping,
    PixelOutOfGamut,
}

impl From<ArtifactMetricArg> for r3sizer_core::ArtifactMetric {
    fn from(val: ArtifactMetricArg) -> Self {
        match val {
            ArtifactMetricArg::ChannelClipping => r3sizer_core::ArtifactMetric::ChannelClippingRatio,
            ArtifactMetricArg::PixelOutOfGamut => r3sizer_core::ArtifactMetric::PixelOutOfGamutRatio,
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum DiagnosticsLevelArg {
    Summary,
    Full,
}

impl From<DiagnosticsLevelArg> for r3sizer_core::DiagnosticsLevel {
    fn from(val: DiagnosticsLevelArg) -> Self {
        match val {
            DiagnosticsLevelArg::Summary => r3sizer_core::DiagnosticsLevel::Summary,
            DiagnosticsLevelArg::Full => r3sizer_core::DiagnosticsLevel::Full,
        }
    }
}
