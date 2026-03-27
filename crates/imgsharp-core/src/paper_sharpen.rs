//! Paper-style lightness sharpening (scaffold).
//!
//! **Provenance: paper-supported** — this module is a scaffold for the paper's
//! sharpening operator, which is believed to operate on lightness but whose
//! exact kernel/formula is not yet confirmed.
//!
//! Current implementation delegates to the same Gaussian USM used by
//! `SharpenModel::PracticalUsm`. When the paper-exact operator is identified,
//! only this module needs to change.

use crate::sharpen;

/// Apply paper-style lightness sharpening.
///
/// **Current behavior:** identical to `sharpen::unsharp_mask_single_channel_with_kernel`.
/// This is a scaffold that preserves the module boundary for future replacement.
///
/// # Arguments
///
/// * `luminance` — flat CIE Y luminance buffer (W * H elements)
/// * `width`, `height` — image dimensions
/// * `amount` — sharpening strength `s`
/// * `kernel` — pre-computed 1-D Gaussian kernel
pub fn paper_sharpen_lightness(
    luminance: &[f32],
    width: usize,
    height: usize,
    amount: f32,
    kernel: &[f32],
) -> Vec<f32> {
    // TODO(paper-faithful-sharpen): Replace with the paper-exact lightness-domain
    // sharpening operator once its formula is reconstructed and verified.
    sharpen::unsharp_mask_single_channel_with_kernel(luminance, width, height, amount, kernel)
}
