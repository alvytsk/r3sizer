//! Serializable metadata types and enums.

use serde::{Deserialize, Serialize};

/// Categories of metadata found in images.
#[derive(
    Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq,
)]
#[cfg_attr(feature = "typegen", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum MetadataCategory {
    /// EXIF orientation, GPS, camera settings
    Exif,
    /// Extensible Metadata Platform (XMP)
    Xmp,
    /// IPTC keywords and metadata
    Iptc,
    /// Color profile (ICC)
    Icc,
    /// Text comments or descriptions
    Text,
    /// Embedded thumbnail data
    Thumbnail,
    /// Camera-specific maker notes
    MakerNote,
    /// Resolution/density information
    Density,
    /// Unknown or unclassified metadata
    Unknown,
}

/// Reasons metadata could not be processed.
#[derive(
    Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq,
)]
#[cfg_attr(feature = "typegen", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum MetadataIssueReason {
    /// Format or metadata type not supported
    Unsupported,
    /// Malformed or corrupted metadata
    Malformed,
    /// Stale metadata removed (e.g., outdated orientation)
    RemovedStale,
    /// Metadata present but unverified (conservative fallback)
    Unverified,
    /// Exceeded size or complexity limits
    LimitExceeded,
    /// Merge operation failed
    MergeFailed,
}

/// A single issue encountered during metadata processing.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "typegen", derive(ts_rs::TS))]
pub struct MetadataIssue {
    /// Category of metadata affected
    pub category: MetadataCategory,
    /// Reason the issue occurred
    pub reason: MetadataIssueReason,
    /// Optional tag name, text keyword, or container identifier (not the value)
    pub field: Option<String>,
}

/// Report of metadata issues encountered.
#[derive(
    Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq,
)]
#[cfg_attr(feature = "typegen", derive(ts_rs::TS))]
pub struct MetadataReport {
    /// Issues encountered during processing
    pub issues: Vec<MetadataIssue>,
}

/// Orientation handling for output.
#[derive(
    Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq,
)]
#[cfg_attr(feature = "typegen", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum OrientationAction {
    /// Keep original orientation metadata
    Preserve,
    /// Normalize to default orientation
    Normalize,
}

/// Color space handling for output.
#[derive(
    Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq,
)]
#[cfg_attr(feature = "typegen", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ColorAction {
    /// Leave color space unchanged
    Unchanged,
    /// Convert to sRGB
    Srgb,
    /// Color space unverified
    Unverified,
}

/// Output image facts for metadata merge operations.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "typegen", derive(ts_rs::TS))]
pub struct OutputFacts {
    /// Output image width in pixels
    pub width: u32,
    /// Output image height in pixels
    pub height: u32,
    /// Orientation handling strategy
    pub orientation: OrientationAction,
    /// Color space handling strategy
    pub color: ColorAction,
}

/// Exported metadata with bytes and diagnostic report.
#[derive(Debug, Clone)]
pub struct MetadataExport {
    /// Raw metadata bytes suitable for embedding in output
    pub bytes: Vec<u8>,
    /// Diagnostic report of any issues
    pub report: MetadataReport,
}
