//! r3sizer-io — image loading and saving.
//!
//! Bridges the `image` crate's file I/O to `r3sizer_core::LinearRgbImage`.
//!
//! Load path:  file → u8/u16 pixels → normalized f32 → sRGB → linear RGB
//! Save path:  linear RGB → sRGB → u8 clamp → file

pub mod convert;
pub mod load;
pub mod save;

pub use load::{load_as_linear, load_as_linear_with_limits, DecodeLimits};
pub use save::save_from_linear;

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("image decode/encode error: {0}")]
    Image(#[from] image::ImageError),

    #[error("file I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("core error: {0}")]
    Core(#[from] r3sizer_core::CoreError),

    #[error("unsupported pixel format: {0}")]
    UnsupportedFormat(String),

    #[error("image too large: {width}×{height} exceeds configured decode limits")]
    TooLarge { width: u32, height: u32 },
}
