//! Artifact metrics for the auto-sharpness pipeline.
//!
//! v0.2: four components (GamutExcursion, HaloRinging, EdgeOvershoot, TextureFlattening)
//! computed per-probe and at final measurement. The solver uses GamutExcursion for
//! selection; the composite score is diagnostic only.

mod composite;
mod edges;
mod gamut;
mod halo;
mod overshoot;
mod texture;

pub use gamut::{channel_clipping_ratio, pixel_out_of_gamut_ratio};

use std::collections::BTreeMap;

use crate::{ArtifactMetric, MetricBreakdown, MetricComponent, MetricWeights};
use crate::types::LinearRgbImage;

/// Compute the full per-component metric breakdown.
///
/// In v0.2, all four components are populated. The solver uses `selection_score`
/// (GamutExcursion) for fitting; `composite_score` is diagnostic only.
pub fn compute_metric_breakdown(
    sharpened: &LinearRgbImage,
    original: &LinearRgbImage,
    luma_original: &[f32],
    luma_sharpened: &[f32],
    artifact_metric: ArtifactMetric,
    weights: &MetricWeights,
) -> MetricBreakdown {
    let gamut = match artifact_metric {
        ArtifactMetric::ChannelClippingRatio => channel_clipping_ratio(sharpened),
        ArtifactMetric::PixelOutOfGamutRatio => pixel_out_of_gamut_ratio(sharpened),
    };

    let w = original.width() as usize;
    let h = original.height() as usize;

    // Shared edge profiling for halo + overshoot.
    let profiles = edges::extract_edge_profiles(
        luma_original,
        luma_sharpened,
        w,
        h,
        edges::DEFAULT_EDGE_THRESHOLD,
    );

    let halo_score = halo::halo_ringing_score(&profiles);
    let overshoot_score = overshoot::edge_overshoot_score(&profiles);
    let texture_score = texture::texture_flattening_score(
        luma_original,
        luma_sharpened,
        w,
        h,
        texture::DEFAULT_TEXTURE_THRESHOLD,
    );

    let composite_score = composite::weighted_aggregate(
        gamut,
        halo_score,
        overshoot_score,
        texture_score,
        weights,
    );

    let mut components = BTreeMap::new();
    components.insert(MetricComponent::GamutExcursion, gamut);
    components.insert(MetricComponent::HaloRinging, halo_score);
    components.insert(MetricComponent::EdgeOvershoot, overshoot_score);
    components.insert(MetricComponent::TextureFlattening, texture_score);

    #[allow(deprecated)]
    MetricBreakdown {
        components,
        selected_metric: MetricComponent::GamutExcursion,
        selection_score: gamut,
        composite_score,
        aggregate: gamut,
    }
}

/// Fast path: compute only the selection metric used by the solver.
///
/// Skips the expensive edge profiling, halo, overshoot, and texture metrics
/// that are only needed for diagnostics.
pub fn compute_selection_metric(
    sharpened: &LinearRgbImage,
    artifact_metric: ArtifactMetric,
) -> f32 {
    match artifact_metric {
        ArtifactMetric::ChannelClippingRatio => channel_clipping_ratio(sharpened),
        ArtifactMetric::PixelOutOfGamutRatio => pixel_out_of_gamut_ratio(sharpened),
    }
}

/// Deprecated alias for [`channel_clipping_ratio`].
#[deprecated(note = "renamed to channel_clipping_ratio")]
pub fn artifact_ratio(img: &LinearRgbImage) -> f32 {
    channel_clipping_ratio(img)
}
