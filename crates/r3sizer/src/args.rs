use std::path::PathBuf;

/// `r3sizer` — downscale an image with automatic sharpness adjustment.
#[derive(clap::Parser, Debug)]
#[command(
    name = "r3sizer",
    about = "Downscale an image with automatic sharpness adjustment",
    long_about = "Downscales an image in linear RGB space and selects the sharpening \
                  strength automatically by fitting a cubic model P(s) of out-of-gamut \
                  artifact ratios, then solving for a target artifact threshold P0."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(clap::Subcommand, Debug)]
pub enum Commands {
    /// Process a single image file.
    Process(ProcessArgs),
    /// Process all images in a directory (batch mode).
    Sweep(SweepArgs),
    /// Compare two sweep summary JSON files.
    Diff(DiffArgs),
    /// Generate a synthetic benchmark corpus.
    Corpus(CorpusArgs),
    /// Show or list available presets.
    #[command(subcommand)]
    Presets(PresetsCommand),
}

// ---------------------------------------------------------------------------
// Shared pipeline parameters
// ---------------------------------------------------------------------------

/// Pipeline configuration flags shared by `process` and `sweep`.
#[derive(clap::Args, Debug)]
pub struct PipelineArgs {
    /// Target width in pixels.
    #[arg(long, short = 'W')]
    pub width: Option<u32>,

    /// Target height in pixels.
    #[arg(long, short = 'H')]
    pub height: Option<u32>,

    /// Preserve the input image's aspect ratio.  Only one of --width or --height is
    /// then required; the other is computed automatically.
    #[arg(long, short = 'p')]
    pub preserve_aspect_ratio: bool,

    /// Target artifact ratio P0 (fraction of channel values outside \[0,1\]).
    /// Default: 0.003 (= 0.3 %, "photo" preset).
    #[arg(long, default_value_t = 0.003)]
    pub target_artifact_ratio: f32,

    /// Explicit comma-separated probe sharpening strengths, e.g. "0.05,0.1,0.2,0.4".
    /// When omitted, the default non-uniform grid is used.
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

    /// Diagnostics verbosity: "summary" (default) or "full" (per-probe breakdowns).
    #[arg(long, default_value = "summary")]
    pub diagnostics_level: DiagnosticsLevelArg,

    /// Named pipeline preset. Overrides all pipeline settings.
    /// Stable: photo (default), precision.  Legacy: baseline, v3-adaptive, v5-full, v5-two-pass.
    #[arg(long, value_name = "NAME")]
    pub preset: Option<String>,

    /// Performance-quality tradeoff: fast, balanced (default), or quality.
    #[arg(long, value_name = "MODE")]
    pub mode: Option<PipelineModeArg>,

    /// Selection policy: "gamut-only" (default), "hybrid", or "composite-only".
    #[arg(long, default_value = "gamut-only")]
    pub selection_policy: SelectionPolicyArg,
}

// ---------------------------------------------------------------------------
// Subcommand arg structs
// ---------------------------------------------------------------------------

#[derive(clap::Args, Debug)]
pub struct ProcessArgs {
    /// Input image file (PNG, JPEG, BMP, TIFF, WebP, ...).
    #[arg(long, short = 'i', value_name = "FILE")]
    pub input: PathBuf,

    /// Output image file.  Format is inferred from the extension.
    #[arg(long, short = 'o', value_name = "FILE")]
    pub output: PathBuf,

    /// Path to write a JSON diagnostics file (optional).
    #[arg(long, value_name = "FILE")]
    pub diagnostics: Option<PathBuf>,

    /// Output format for the process summary printed to stdout.
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub output_format: OutputFormat,

    #[command(flatten)]
    pub pipeline: PipelineArgs,
}

#[derive(clap::Args, Debug)]
pub struct SweepArgs {
    /// Directory of images to process.
    #[arg(long = "in-dir", value_name = "DIR")]
    pub in_dir: PathBuf,

    /// Output directory for processed images.
    #[arg(long = "out-dir", value_name = "DIR")]
    pub out_dir: Option<PathBuf>,

    /// Path to write the sweep summary JSON file.
    #[arg(long, value_name = "FILE")]
    pub summary: Option<PathBuf>,

    #[command(flatten)]
    pub pipeline: PipelineArgs,
}

#[derive(clap::Args, Debug)]
pub struct DiffArgs {
    /// Baseline sweep summary JSON.
    pub baseline: PathBuf,
    /// Candidate sweep summary JSON.
    pub candidate: PathBuf,
}

#[derive(clap::Args, Debug)]
pub struct CorpusArgs {
    /// Directory to write generated images.
    pub output_dir: PathBuf,
}

#[derive(clap::Subcommand, Debug)]
pub enum PresetsCommand {
    /// List all available preset names.
    List,
    /// Show the configuration for a named preset.
    Show {
        /// Preset name (e.g. "photo", "precision").
        name: String,
    },
}

// ---------------------------------------------------------------------------
// Output format
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum OutputFormat {
    /// Human-readable text (default).
    Text,
    /// JSON object printed to stdout.
    Json,
}

// ---------------------------------------------------------------------------
// CLI-friendly wrappers for core enums
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
            ArtifactMetricArg::ChannelClipping => {
                r3sizer_core::ArtifactMetric::ChannelClippingRatio
            }
            ArtifactMetricArg::PixelOutOfGamut => {
                r3sizer_core::ArtifactMetric::PixelOutOfGamutRatio
            }
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

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SelectionPolicyArg {
    GamutOnly,
    Hybrid,
    CompositeOnly,
}

impl From<SelectionPolicyArg> for r3sizer_core::SelectionPolicy {
    fn from(val: SelectionPolicyArg) -> Self {
        match val {
            SelectionPolicyArg::GamutOnly => r3sizer_core::SelectionPolicy::GamutOnly,
            SelectionPolicyArg::Hybrid => r3sizer_core::SelectionPolicy::Hybrid,
            SelectionPolicyArg::CompositeOnly => r3sizer_core::SelectionPolicy::CompositeOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum PipelineModeArg {
    Fast,
    Balanced,
    Quality,
}

impl From<PipelineModeArg> for r3sizer_core::PipelineMode {
    fn from(val: PipelineModeArg) -> Self {
        match val {
            PipelineModeArg::Fast => r3sizer_core::PipelineMode::Fast,
            PipelineModeArg::Balanced => r3sizer_core::PipelineMode::Balanced,
            PipelineModeArg::Quality => r3sizer_core::PipelineMode::Quality,
        }
    }
}
