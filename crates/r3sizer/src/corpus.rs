/// Synthetic benchmark corpus generator.
///
/// Creates 8 deterministic test images covering the content categories
/// from the step-6 benchmarking roadmap: smooth gradients, step edges,
/// high-frequency texture, color bars, concentric circles, thin lines,
/// seeded noise (foliage proxy), and mixed-region content.
///
/// Usage: `r3sizer corpus ./corpus`
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
        // Chroma stress cases (step 6 tuning)
        ("low_sat_noise", low_sat_noise()),
        ("saturated_edges", saturated_edges()),
        ("colored_lines", colored_lines()),
        ("noisy_lowlight", noisy_lowlight()),
        ("saturated_texture", saturated_texture()),
    ];

    for (name, img) in &images {
        let path = dir.join(format!("{name}.png"));
        save_from_linear(img, &path)
            .with_context(|| format!("failed to save {}", path.display()))?;
        println!("  {name}.png  ({}×{})", img.width(), img.height());
    }

    println!(
        "Corpus: {} images written to {}",
        images.len(),
        dir.display()
    );
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

// ---------------------------------------------------------------------------
// Chroma stress cases (added for step 6 tuning)
// ---------------------------------------------------------------------------

/// Low-saturation colored noise — camera sensor noise proxy.
/// Tests whether chroma guard over-clamps low-chroma noisy content.
fn low_sat_noise() -> LinearRgbImage {
    let mut state: u32 = 0xA5A5_5A5A;
    let mut data = Vec::with_capacity((W * H * 3) as usize);
    for _y in 0..H {
        for _x in 0..W {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let base = (state as f32 / u32::MAX as f32) * 0.3 + 0.35;
            // Very slight color cast — simulates warm camera noise
            let r = base + 0.015;
            let g = base;
            let b = base - 0.010;
            data.extend_from_slice(&[r.clamp(0.0, 1.0), g, b.clamp(0.0, 1.0)]);
        }
    }
    LinearRgbImage::new(W, H, data).unwrap()
}

/// Sharp transitions between vivid colors — worst case for chroma guard.
/// Tests that the guard catches real color artifacts at strong chroma boundaries.
fn saturated_edges() -> LinearRgbImage {
    let colors: [[f32; 3]; 4] = [
        [0.85, 0.05, 0.05], // red
        [0.05, 0.05, 0.85], // blue
        [0.85, 0.85, 0.05], // yellow
        [0.05, 0.75, 0.05], // green
    ];
    let block_h = H / 4;
    let mut data = Vec::with_capacity((W * H * 3) as usize);
    for y in 0..H {
        for x in 0..W {
            let band = ((y / block_h) as usize).min(3);
            let alt_band = (band + 1) % 4;
            let stripe = (x / 8) % 2;
            let c = if stripe == 0 {
                colors[band]
            } else {
                colors[alt_band]
            };
            data.extend_from_slice(&c);
        }
    }
    LinearRgbImage::new(W, H, data).unwrap()
}

/// Colored thin lines on neutral gray — colored text/UI proxy.
/// Tests chroma preservation for small colored features.
fn colored_lines() -> LinearRgbImage {
    let bg = [0.6, 0.6, 0.6];
    let line_colors: [[f32; 3]; 5] = [
        [0.80, 0.10, 0.10],
        [0.10, 0.10, 0.80],
        [0.10, 0.70, 0.10],
        [0.70, 0.40, 0.05],
        [0.50, 0.10, 0.60],
    ];
    let mut data = Vec::with_capacity((W * H * 3) as usize);
    for y in 0..H {
        let group_y = y % (6 * 5);
        let color_idx = (group_y / 6) as usize;
        let is_line = group_y % 6 == 0;
        let pixel = if is_line {
            line_colors[color_idx.min(4)]
        } else {
            bg
        };
        for _x in 0..W {
            data.extend_from_slice(&pixel);
        }
    }
    LinearRgbImage::new(W, H, data).unwrap()
}

/// Dark noisy image — low-light / high-ISO proxy.
/// Tests guard behavior at low luminance where chroma noise is relatively large.
fn noisy_lowlight() -> LinearRgbImage {
    let mut state: u32 = 0x1234_5678;
    let mut data = Vec::with_capacity((W * H * 3) as usize);
    for _y in 0..H {
        for _x in 0..W {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise = (state as f32 / u32::MAX as f32) * 0.08;
            let r = (0.10 + noise).clamp(0.0, 1.0);
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise2 = (state as f32 / u32::MAX as f32) * 0.08;
            let g = (0.09 + noise2).clamp(0.0, 1.0);
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise3 = (state as f32 / u32::MAX as f32) * 0.08;
            let b = (0.11 + noise3).clamp(0.0, 1.0);
            data.extend_from_slice(&[r, g, b]);
        }
    }
    LinearRgbImage::new(W, H, data).unwrap()
}

/// Vivid colored texture — colorful foliage/fabric proxy.
/// Tests that the guard does not dull legitimate saturated texture.
fn saturated_texture() -> LinearRgbImage {
    let mut state: u32 = 0xBEEF_CAFE;
    let mut data = Vec::with_capacity((W * H * 3) as usize);
    for y in 0..H {
        for x in 0..W {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise = (state as f32 / u32::MAX as f32) * 0.15;
            let hue = (x as f32 / W as f32 + y as f32 / H as f32) * 3.0;
            let r = (0.5 + 0.35 * (hue * std::f32::consts::TAU).sin() + noise).clamp(0.0, 1.0);
            let g = (0.5 + 0.35 * ((hue + 0.333) * std::f32::consts::TAU).sin() + noise)
                .clamp(0.0, 1.0);
            let b = (0.5 + 0.35 * ((hue + 0.667) * std::f32::consts::TAU).sin() + noise)
                .clamp(0.0, 1.0);
            data.extend_from_slice(&[r, g, b]);
        }
    }
    LinearRgbImage::new(W, H, data).unwrap()
}
