//! Composite metric: weighted sum of individual components.
//!
//! Observation-only in v0.2 — the solver uses GamutExcursion for selection.

use crate::MetricWeights;

/// Compute the weighted composite score.
pub fn weighted_aggregate(
    gamut_excursion: f32,
    halo_ringing: f32,
    edge_overshoot: f32,
    texture_flattening: f32,
    weights: &MetricWeights,
) -> f32 {
    weights.gamut_excursion * gamut_excursion
        + weights.halo_ringing * halo_ringing
        + weights.edge_overshoot * edge_overshoot
        + weights.texture_flattening * texture_flattening
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_weights_gamut_dominant() {
        let w = MetricWeights::default();
        let score = weighted_aggregate(0.1, 0.1, 0.1, 0.1, &w);
        assert!((score - 0.17).abs() < 1e-6);
    }

    #[test]
    fn zero_inputs_zero_output() {
        let w = MetricWeights::default();
        assert_eq!(weighted_aggregate(0.0, 0.0, 0.0, 0.0, &w), 0.0);
    }

    #[test]
    fn only_gamut_excursion() {
        let w = MetricWeights {
            gamut_excursion: 1.0,
            halo_ringing: 0.0,
            edge_overshoot: 0.0,
            texture_flattening: 0.0,
        };
        assert!((weighted_aggregate(0.5, 0.3, 0.2, 0.1, &w) - 0.5).abs() < 1e-6);
    }
}
