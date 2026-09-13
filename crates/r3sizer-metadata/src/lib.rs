//! r3sizer-metadata — metadata extraction, reporting, and merging.
//!
//! Provides the contract types and conservative fallback behavior for preserving
//! image metadata through resize and sharpen operations. Format-specific extraction
//! and merging are implemented by adapter tasks.

mod bundle;
mod limits;
mod types;

pub use bundle::{MetadataBundle, SourceColor};
pub use limits::MetadataLimits;
pub use types::{
    ColorAction, MetadataCategory, MetadataExport, MetadataIssue, MetadataIssueReason,
    MetadataReport, OrientationAction, OutputFacts,
};

/// Extract metadata from source bytes using conservative fallback.
///
/// Returns a `MetadataBundle` reporting the source format as unknown and metadata as unverified.
/// Later adapter tasks will replace this with format-specific extraction logic.
pub fn extract(source: &[u8], limits: &MetadataLimits) -> MetadataBundle {
    // Conservative fallback: check bounds, report as unverified
    if source.len() > limits.max_source_bytes {
        return MetadataBundle::unavailable(MetadataIssueReason::LimitExceeded);
    }

    MetadataBundle::unavailable(MetadataIssueReason::Unverified)
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
