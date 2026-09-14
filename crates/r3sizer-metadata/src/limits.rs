//! Metadata size and complexity limits.

use serde::{Deserialize, Serialize};

/// Configurable limits for metadata extraction and merging.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "typegen", derive(ts_rs::TS))]
pub struct MetadataLimits {
    /// Maximum source file size in bytes
    pub max_source_bytes: usize,
    /// Maximum single payload size in bytes
    pub max_payload_bytes: usize,
    /// Maximum total metadata bytes across all payloads
    pub max_total_metadata_bytes: usize,
    /// Maximum number of metadata records
    pub max_records: usize,
    /// Maximum EXIF entries
    pub max_exif_entries: usize,
    /// Maximum EXIF IFDs
    pub max_ifds: usize,
    /// Maximum XML nesting depth for XMP
    pub max_xml_depth: usize,
}

/// Default maximum source file size: 256 MiB.
pub const DEFAULT_MAX_SOURCE_BYTES: usize = 256 * 1024 * 1024;
/// Default maximum single payload size: 8 MiB.
pub const DEFAULT_MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
/// Default maximum total retained/decompressed metadata: 16 MiB.
pub const DEFAULT_MAX_TOTAL_METADATA_BYTES: usize = 16 * 1024 * 1024;
/// Default maximum number of container metadata records.
pub const DEFAULT_MAX_RECORDS: usize = 4_096;
/// Default maximum number of EXIF entries.
pub const DEFAULT_MAX_EXIF_ENTRIES: usize = 4_096;
/// Default maximum number of visited EXIF IFDs.
pub const DEFAULT_MAX_IFDS: usize = 32;
/// Default maximum XML nesting depth for XMP.
pub const DEFAULT_MAX_XML_DEPTH: usize = 64;

impl Default for MetadataLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: DEFAULT_MAX_SOURCE_BYTES,
            max_payload_bytes: DEFAULT_MAX_PAYLOAD_BYTES,
            max_total_metadata_bytes: DEFAULT_MAX_TOTAL_METADATA_BYTES,
            max_records: DEFAULT_MAX_RECORDS,
            max_exif_entries: DEFAULT_MAX_EXIF_ENTRIES,
            max_ifds: DEFAULT_MAX_IFDS,
            max_xml_depth: DEFAULT_MAX_XML_DEPTH,
        }
    }
}

/// Checked range construction with overflow detection.
///
/// Returns a byte range `offset..end` if:
/// - `count * unit` does not overflow
/// - `offset + (count * unit)` does not overflow
/// - The resulting range does not exceed `length`
///
/// # Example
/// ```ignore
/// let range = checked_range(0, 10, 4, 100)?;  // 0..40
/// let range = checked_range(100, 1, 1, 100)?; // None (out of bounds)
/// ```
pub(crate) fn checked_range(
    offset: usize,
    count: usize,
    unit: usize,
    length: usize,
) -> Option<std::ops::Range<usize>> {
    let end = offset.checked_add(count.checked_mul(unit)?)?;
    (end <= length).then_some(offset..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_range_valid() {
        assert_eq!(checked_range(0, 10, 4, 100), Some(0..40));
        assert_eq!(checked_range(20, 5, 2, 100), Some(20..30));
    }

    #[test]
    fn checked_range_overflow_mul() {
        assert_eq!(checked_range(0, usize::MAX, 2, 100), None);
    }

    #[test]
    fn checked_range_overflow_add() {
        assert_eq!(checked_range(usize::MAX, 1, 1, usize::MAX), None);
    }

    #[test]
    fn checked_range_out_of_bounds() {
        assert_eq!(checked_range(0, 10, 4, 39), None); // Would be 40, exceeds 39
        assert_eq!(checked_range(50, 10, 4, 89), None); // Would be 90, exceeds 89
    }

    #[test]
    fn checked_range_edge_cases() {
        assert_eq!(checked_range(0, 0, 100, 100), Some(0..0));
        assert_eq!(checked_range(100, 0, 1, 100), Some(100..100));
        assert_eq!(checked_range(0, 1, 100, 100), Some(0..100));
    }
}
