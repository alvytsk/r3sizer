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

use bundle::Payload;

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

/// Merge metadata from a source bundle into already-encoded destination
/// bytes.
///
/// Runs `policy::prepare` (color/density policy plus EXIF/XMP correction)
/// using the destination's *own* current ICC profile -- read from `encoded`
/// itself via the same conservative extraction `extract()` uses, so a
/// source ICC profile is only ever retained when it can be proven to match
/// what the destination encoder already declares -- then hands the
/// corrected bundle to the destination-format adapter in `containers`.
///
/// Returns the original `encoded` bytes unchanged, with every attempted
/// category reported as `MergeFailed`, if the destination format isn't
/// recognized or the container can't be safely mutated.
pub fn merge(
    encoded: Vec<u8>,
    source: &MetadataBundle,
    facts: &OutputFacts,
    limits: &MetadataLimits,
) -> MetadataExport {
    // Same size gate `extract()` applies: an oversized destination is
    // never scanned for its own ICC profile either, so this internal
    // re-extraction can't be used to bypass `max_source_bytes` through the
    // back door. `containers::embed` below still has to parse `encoded`
    // regardless of size (that's the merge itself, not optional), but this
    // ICC lookup is -- so it stays honest about the same limit.
    let destination_icc = if encoded.len() > limits.max_source_bytes {
        None
    } else {
        containers::extract_payloads(&encoded, limits)
            .payloads
            .iter()
            .find_map(|p| match p {
                Payload::Icc(bytes) => Some(bytes.clone()),
                _ => None,
            })
    };
    let prepared = policy::prepare(source, facts, destination_icc.as_deref(), limits);
    containers::embed(encoded, &prepared, facts, limits)
}
