//! Metadata-aware native I/O.
//!
//! Additive on top of [`crate::load`]/[`crate::save`]: [`load_with_metadata`]
//! reads pixels *and* source metadata from the same file read, and
//! [`save_with_metadata`] merges that source metadata into the freshly
//! encoded destination bytes before writing them to disk.
//!
//! Load path:  file bytes → decode dimensions/pixels → linear RGB
//!                        → extract metadata (same bytes)
//! Save path:  linear RGB → encode to bytes (no disk I/O yet)
//!                        → merge source metadata into those bytes → write
use std::path::Path;

use r3sizer_core::LinearRgbImage;
use r3sizer_metadata::{
    ColorAction, MetadataBundle, MetadataIssueReason, MetadataLimits, MetadataReport,
    OrientationAction, OutputFacts, SourceColor,
};

use crate::{
    load::{check_dimensions, decode_from_dynamic},
    save::encode_linear_image,
    DecodeLimits, IoError,
};

/// A loaded image together with the source metadata needed to carry it
/// through to the eventual export.
///
/// Captured once, at load time, from a single read of the source file --
/// not re-read from `path` later. This matters for input=output exports:
/// by the time [`save_with_metadata`] writes to a path that may equal the
/// original source path, everything it needs from the source already lives
/// in this struct.
#[derive(Debug)]
pub struct LoadedImage {
    /// Decoded pixels, linear RGB.
    pub image: LinearRgbImage,
    /// Metadata extracted from the source (or a conservative "unavailable"
    /// bundle if extraction was skipped or failed).
    pub metadata: MetadataBundle,
    /// Orientation handling to request when this image is exported.
    pub orientation: OrientationAction,
    /// Color handling to request when this image is exported.
    pub color: ColorAction,
}

/// Load a raster image from `path`, decoding pixels and extracting source
/// metadata from the same read.
///
/// If the source is no larger than `metadata_limits.max_source_bytes`, it is
/// read into memory once; that single byte buffer is decoded for pixels
/// (after its header dimensions are checked against `decode_limits`) *and*
/// handed to [`r3sizer_metadata::extract`]. Larger sources are pixel-decoded
/// by streaming from the open path, as [`crate::load_as_linear_with_limits`]
/// does, without ever materializing the full encoded file in memory; their
/// metadata is reported as unavailable (`LimitExceeded`) rather than paying
/// for a second full read.
///
/// A pixel decode/read failure returns [`IoError`], as it does for the
/// pixel-only loaders. A successful pixel decode with metadata that fails to
/// parse does not error: it returns a [`LoadedImage`] carrying
/// `MetadataBundle::unavailable(..)` (metadata extraction itself is designed
/// to never fail outright -- malformed input surfaces as issues on the
/// returned report instead).
pub fn load_with_metadata(
    path: &Path,
    decode_limits: &DecodeLimits,
    metadata_limits: &MetadataLimits,
) -> Result<LoadedImage, IoError> {
    let file_len = std::fs::metadata(path)?.len();

    let (image, metadata) = if file_len <= metadata_limits.max_source_bytes as u64 {
        // Single read: the same bytes serve dimension-checking, pixel
        // decode, and metadata extraction.
        let bytes = std::fs::read(path)?;

        let (width, height) = image::ImageReader::new(std::io::Cursor::new(bytes.as_slice()))
            .with_guessed_format()?
            .into_dimensions()?;
        check_dimensions(width, height, decode_limits)?;

        let dyn_img = image::ImageReader::new(std::io::Cursor::new(bytes.as_slice()))
            .with_guessed_format()?
            .decode()?;
        let image = decode_from_dynamic(dyn_img, width, height)?;

        let metadata = r3sizer_metadata::extract(&bytes, metadata_limits);
        (image, metadata)
    } else {
        // Source exceeds the metadata cap: stream-decode pixels from the
        // open path (never allocating the full encoded file), and skip
        // metadata extraction outright.
        let (width, height) = image::ImageReader::open(path)?
            .with_guessed_format()?
            .into_dimensions()?;
        check_dimensions(width, height, decode_limits)?;

        let dyn_img = image::open(path)?;
        let image = decode_from_dynamic(dyn_img, width, height)?;

        let metadata = MetadataBundle::unavailable(MetadataIssueReason::LimitExceeded);
        (image, metadata)
    };

    // Native decode never applies EXIF orientation transforms, so the pixel
    // data is still in the source's original orientation.
    let orientation = OrientationAction::Preserve;

    // Only a verified sRGB declaration, or a verified absence of any color
    // declaration, matches the native decode's implicit sRGB assumption.
    // Anything else -- including formats outside the metadata adapters
    // (BMP/TIFF/GIF), which report `SourceColor::Unknown` -- is unverified.
    let color = match metadata.source_color() {
        SourceColor::Srgb | SourceColor::Unspecified => ColorAction::Srgb,
        SourceColor::Other | SourceColor::Unknown => ColorAction::Unverified,
    };

    Ok(LoadedImage {
        image,
        metadata,
        orientation,
        color,
    })
}

/// Encode `image` for `path` and export it with `source`'s metadata merged
/// in, writing the result to `path`.
///
/// `image` is the *processed* image (its dimensions, which may differ from
/// `source.image`'s after resizing, are what gets reported to the merge as
/// [`OutputFacts`]). Encoding happens into memory first; the destination
/// file is written only once, from the merged bytes -- so a disk write
/// failure is reported as an [`IoError`] and never as a successful export,
/// and an input=output path is only ever written to after `source` was
/// fully captured by [`load_with_metadata`].
///
/// Destination formats `r3sizer_metadata::merge` doesn't recognize (e.g.
/// BMP/TIFF) fall back to the original encoded bytes with `MergeFailed`
/// issues for whatever source metadata categories existed -- unsupported
/// formats with no source metadata to carry produce no issues at all.
pub fn save_with_metadata(
    image: &LinearRgbImage,
    path: &Path,
    source: &LoadedImage,
    limits: &MetadataLimits,
) -> Result<MetadataReport, IoError> {
    let encoded = encode_linear_image(image, path)?;

    let facts = OutputFacts {
        width: image.width(),
        height: image.height(),
        orientation: source.orientation,
        color: source.color,
    };
    let exported = r3sizer_metadata::merge(encoded, &source.metadata, &facts, limits);
    std::fs::write(path, &exported.bytes)?;
    Ok(exported.report)
}
