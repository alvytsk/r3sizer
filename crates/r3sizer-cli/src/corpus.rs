/// Synthetic benchmark corpus generator.
///
/// Creates 8 deterministic test images covering the content categories
/// from the step-6 benchmarking roadmap: smooth gradients, step edges,
/// high-frequency texture, color bars, concentric circles, thin lines,
/// seeded noise (foliage proxy), and mixed-region content.
///
/// Usage: `r3sizer --generate-corpus ./corpus`
use std::path::Path;

use anyhow::{Context, Result};

use r3sizer_core::LinearRgbImage;
use r3sizer_io::save_from_linear;

/// Width and height of generated corpus images.
const W: u32 = 1024;
const H: u32 = 768;

/// Generate the full corpus and save to `dir`.
pub fn generate_corpus(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create corpus directory: {}", dir.display()))?;

    let images: Vec<(&str, LinearRgbImage)> = vec![
        ("smooth_gradient", smooth_gradient()),
        ("step_edge", step_edge()),
        ("fine_checkerboard", fine_checkerboard()),
        ("color_bars", color_bars()),
        ("concentric_circles", concentric_circles()),
        ("thin_lines", thin_lines()),
        ("seeded_noise", seeded_noise()),
        ("mixed_regions", mixed_regions()),
    ];

    for (name, img) in &images {
        let path = dir.join(format!("{name}.png"));
        save_from_linear(img, &path)
            .with_context(|| format!("failed to save {}", path.display()))?;
        println!("  {name}.png  ({}×{})", img.width(), img.height());
    }

    println!("Corpus: {} images written to {}", images.len(), dir.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Image generators (all deterministic)
// ---------------------------------------------------------------------------

/// Smooth horizontal + vertical gradient. Tests basic resize quality.
fn smooth_gradient() -> LinearRgbImage {
    let mut data = Vec::with_capacity((W * H * 3) as usize);
    for y in 0..H {
        for x in 0..W {
            let r = x as f32 / (W - 1) as f32;
            let g = y as f32 / (H - 1) as f32;
            let b = 0.3;
            data.extend_from_slice(&[r, g, b]);
        }
    }
    LinearRgbImage::new(W, H, data).unwrap()
}

/// Hard vertical step edge: dark left, bright right.
/// Tests ringing detection, Lanczos overshoot, baseline quality scoring.
fn step_edge() -> LinearRgbImage {
    let mut data = Vec::with_capacity((W * H * 3) as usize);
    for _y in 0..H {
        for x in 0..W {
            let v = if x < W / 2 { 0.05 } else { 0.95 };
            data.extend_from_slice(&[v, v, v]);
        }
    }
    LinearRgbImage::new(W, H, data).unwrap()
}

/// 2-pixel checkerboard pattern. Tests anti-aliasing and texture flattening.
fn fine_checkerboard() -> LinearRgbImage {
    let mut data = Vec::with_capacity((W * H * 3) as usize);
    for y in 0..H {
        for x in 0..W {
            let cell = (x / 2 + y / 2) % 2;
            let v = if cell == 0 { 0.9 } else { 0.1 };
            data.extend_from_slice(&[v, v, v]);
        }
    }
    LinearRgbImage::new(W, H, data).unwrap()
}

/// 8 vertical color bars: R, G, B, C, M, Y, white, black.
/// Tests chroma guard, saturation-dependent thresholds.
fn color_bars() -> LinearRgbImage {
    let bars: [[f32; 3]; 8] = [
        [0.9, 0.1, 0.1], // red
        [0.1, 0.8, 0.1], // green
        [0.1, 0.1, 0.9], // blue
        [0.1, 0.8, 0.8], // cyan
        [0.8, 0.1, 0.8], // magenta
        [0.8, 0.8, 0.1], // yellow
        [0.9, 0.9, 0.9], // white
        [0.1, 0.1, 0.1], // black
    ];
    let bar_w = W / 8;
    let mut data = Vec::with_capacity((W * H * 3) as usize);
    for _y in 0..H {
        for x in 0..W {
            let idx = ((x / bar_w) as usize).min(7);
            data.extend_from_slice(&bars[idx]);
        }
    }
    LinearRgbImage::new(W, H, data).unwrap()
}

/// Concentric circles (sinusoidal radial pattern).
/// Tests resolution at different spatial frequencies.
fn concentric_circles() -> LinearRgbImage {
    let sz = W.min(H); // square region centered
    let cx = W as f32 / 2.0;
    let cy = H as f32 / 2.0;
    let max_r = sz as f32 / 2.0;
    let freq = 40.0; // number of rings

    let mut data = Vec::with_capacity((W * H * 3) as usize);
    for y in 0..H {
        for x in 0..W {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let r = (dx * dx + dy * dy).sqrt();
            let v = 0.5 + 0.4 * (r / max_r * freq * std::f32::consts::PI).sin();
            data.extend_from_slice(&[v, v, v]);
        }
    }
    LinearRgbImage::new(W, H, data).unwrap()
}

/// Alternating 1px-dark / 3px-light horizontal lines.
/// Simulates text-like content; tests edge preservation and moiré.
fn thin_lines() -> LinearRgbImage {
    let mut data = Vec::with_capacity((W * H * 3) as usize);
    for y in 0..H {
        let v = if y % 4 == 0 { 0.05 } else { 0.95 };
        for _x in 0..W {
            data.extend_from_slice(&[v, v, v]);
        }
    }
    LinearRgbImage::new(W, H, data).unwrap()
}

/// Deterministic pseudo-random noise with spatial correlation.
/// Simulates foliage / grass texture; tests texture retention metrics.
fn seeded_noise() -> LinearRgbImage {
    let mut state: u32 = 0xDEAD_BEEF;
    let mut data = Vec::with_capacity((W * H * 3) as usize);

    for _y in 0..H {
        for _x in 0..W {
            // Xorshift32 PRNG
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;

            // Map to [0.15, 0.85] to stay in comfortable gamut range
            let base = (state as f32 / u32::MAX as f32) * 0.7 + 0.15;
            // Add slight per-channel variation for chroma content
            let r = base;
            let g = (base + 0.05).min(1.0);
            let b = (base - 0.03).max(0.0);
            data.extend_from_slice(&[r, g, b]);
        }
    }
    LinearRgbImage::new(W, H, data).unwrap()
}

/// Four quadrants: flat, textured, strong edge, saturated color.
/// Tests content-adaptive pipeline, per-region classification accuracy.
fn mixed_regions() -> LinearRgbImage {
    let half_w = W / 2;
    let half_h = H / 2;
    let mut state: u32 = 0xCAFE_BABE;
    let mut data = Vec::with_capacity((W * H * 3) as usize);

    for y in 0..H {
        for x in 0..W {
            let pixel = if x < half_w && y < half_h {
                // Top-left: flat mid-gray
                [0.5, 0.5, 0.5]
            } else if x >= half_w && y < half_h {
                // Top-right: fine noise texture
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let v = (state as f32 / u32::MAX as f32) * 0.4 + 0.3;
                [v, v, v]
            } else if x < half_w && y >= half_h {
                // Bottom-left: vertical stripes (strong edges)
                let v = if (x / 4) % 2 == 0 { 0.1 } else { 0.9 };
                [v, v, v]
            } else {
                // Bottom-right: saturated gradient
                let t = x as f32 / (W - 1) as f32;
                [0.8 * t + 0.1, 0.1, 0.8 * (1.0 - t) + 0.1]
            };
            data.extend_from_slice(&pixel);
        }
    }
    LinearRgbImage::new(W, H, data).unwrap()
}
