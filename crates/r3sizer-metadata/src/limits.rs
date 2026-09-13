//! Metadata size and complexity limits.

/// Configurable limits for metadata extraction and merging.
#[derive(Debug, Clone)]
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

impl Default for MetadataLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 100 * 1024 * 1024,              // 100 MB
            max_payload_bytes: 10 * 1024 * 1024,              // 10 MB per payload
            max_total_metadata_bytes: 50 * 1024 * 1024,       // 50 MB total
            max_records: 1_000_000,
            max_exif_entries: 50_000,
            max_ifds: 32,
            max_xml_depth: 64,
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
#[allow(dead_code)]
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
