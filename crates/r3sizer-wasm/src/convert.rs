use r3sizer_core::{CoreError, LinearRgbImage};
use r3sizer_core::color::linear_to_srgb;

// ---------------------------------------------------------------------------
// Exact u8→linear LUT (256 entries, no interpolation needed)
// ---------------------------------------------------------------------------
//
// Since WASM input is always u8 (0-255 from canvas getImageData), we can
// precompute the exact sRGB→linear value for each of the 256 possible inputs.
// This is a 1KB LUT — trivially fits in L1 and eliminates all powf calls.

/// Precomputed `srgb_to_linear(i / 255.0)` for i in 0..=255.
static SRGB_U8_TO_LINEAR: [f32; 256] = {
    let mut lut = [0.0_f32; 256];
    let mut i: usize = 0;
    while i < 256 {
        let v = i as f64 / 255.0;
        let linear = if v <= 0.04045 {
            v / 12.92
        } else {
            // exp(2.4 * ln((v + 0.055) / 1.055))
            let base = (v + 0.055) / 1.055;
            r3sizer_core::color::const_pow_2_4(base)
        };
        lut[i] = linear as f32;
        i += 1;
    }
    lut
};

/// Convert RGBA sRGB u8 pixels (from canvas `getImageData`) into a `LinearRgbImage`.
///
/// The alpha channel is stripped; each sRGB component is linearized via a
/// 256-entry exact LUT (no interpolation, no `powf` calls).
pub fn rgba_u8_to_linear(data: &[u8], width: u32, height: u32) -> Result<LinearRgbImage, CoreError> {
    let pixel_count = (width as usize) * (height as usize);
    let expected_len = pixel_count * 4;
    if data.len() != expected_len {
        return Err(CoreError::BufferLengthMismatch {
            expected_len,
            got_len: data.len(),
        });
    }

    let mut rgb = Vec::with_capacity(pixel_count * 3);
    for chunk in data.chunks_exact(4) {
        rgb.push(SRGB_U8_TO_LINEAR[chunk[0] as usize]);
        rgb.push(SRGB_U8_TO_LINEAR[chunk[1] as usize]);
        rgb.push(SRGB_U8_TO_LINEAR[chunk[2] as usize]);
    }

    LinearRgbImage::new(width, height, rgb)
}

/// Convert a `LinearRgbImage` back to RGBA sRGB u8 pixels suitable for canvas `putImageData`.
///
/// Each linear component is converted to sRGB, scaled to [0,255], clamped, and
/// an alpha of 255 is appended per pixel.
pub fn linear_to_rgba_u8(img: &LinearRgbImage) -> Vec<u8> {
    let pixel_count = (img.width() as usize) * (img.height() as usize);
    let mut out = Vec::with_capacity(pixel_count * 4);

    for chunk in img.pixels().chunks_exact(3) {
        let r = (linear_to_srgb(chunk[0]) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        let g = (linear_to_srgb(chunk[1]) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        let b = (linear_to_srgb(chunk[2]) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        out.push(r);
        out.push(g);
        out.push(b);
        out.push(255);
    }

    out
}
