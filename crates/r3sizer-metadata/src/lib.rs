//! r3sizer-metadata — metadata extraction, reporting, and merging.
//!
//! Provides the contract types and conservative fallback behavior for preserving
//! image metadata through resize and sharpen operations. Format-specific extraction
//! and merging are implemented by adapter tasks.

mod bundle;
mod containers;
mod exif;
mod iptc;
mod limits;
mod policy;
mod types;
mod xmp;

pub use bundle::{MetadataBundle, SourceColor};
pub use exif::correct;
pub use limits::MetadataLimits;
pub use policy::prepare;
pub use types::{
    ColorAction, MetadataCategory, MetadataExport, MetadataIssue, MetadataIssueReason,
    MetadataReport, OrientationAction, OutputFacts,
};

/// Extract metadata from source bytes.
///
/// Sniffs the container format (JPEG/PNG/WebP) from its magic bytes and
/// scans it for metadata without decoding pixels. Sources exceeding
/// `max_source_bytes`, or containers this crate doesn't recognize, fall
/// back to a conservative unavailable bundle.
pub fn extract(source: &[u8], limits: &MetadataLimits) -> MetadataBundle {
    if source.len() > limits.max_source_bytes {
        return MetadataBundle::unavailable(MetadataIssueReason::LimitExceeded);
    }

    containers::extract_payloads(source, limits)
}

/// Merge metadata into encoded output using conservative fallback.
///
/// Returns the original `encoded` bytes unchanged on unsupported destinations.
/// The report is preserved from the source bundle.
pub fn merge(
    encoded: Vec<u8>,
    source: &MetadataBundle,
    _facts: &OutputFacts,
    _limits: &MetadataLimits,
) -> MetadataExport {
    // Conservative fallback: return original bytes, preserve source report
    MetadataExport {
        bytes: encoded,
        report: source.report().clone(),
    }
}
