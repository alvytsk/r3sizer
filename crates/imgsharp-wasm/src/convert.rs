use imgsharp_core::{CoreError, LinearRgbImage};
use imgsharp_core::color::{linear_to_srgb, srgb_to_linear};

/// Convert RGBA sRGB u8 pixels (from canvas `getImageData`) into a `LinearRgbImage`.
///
/// The alpha channel is stripped; each sRGB component is normalized to [0,1]
/// and then linearized via the IEC 61966-2-1 transfer function.
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
        let r = srgb_to_linear(chunk[0] as f32 / 255.0);
        let g = srgb_to_linear(chunk[1] as f32 / 255.0);
        let b = srgb_to_linear(chunk[2] as f32 / 255.0);
        rgb.push(r);
        rgb.push(g);
        rgb.push(b);
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
