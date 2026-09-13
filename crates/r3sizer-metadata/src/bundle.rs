//! Metadata bundle types and color space information.

use crate::types::MetadataReport;

/// Source color space specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceColor {
    /// Verified sRGB without conflicting profile
    Srgb,
    /// Verified absence of color declarations
    Unspecified,
    /// Custom or unconverted ICC/color declaration
    Other,
    /// Unsupported or unreadable color information
    Unknown,
}

/// Source image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum SourceFormat {
    /// JPEG format
    Jpeg,
    /// PNG format
    Png,
    /// WebP format
    WebP,
    /// Unknown or unsupported format
    Unknown,
}

/// Extracted metadata payload.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum Payload {
    /// EXIF data
    Exif(Vec<u8>),
    /// XMP data
    Xmp(Vec<u8>),
    /// IPTC data
    Iptc(Vec<u8>),
    /// ICC color profile
    Icc(Vec<u8>),
    /// JPEG comment
    JpegComment(Vec<u8>),
    /// PNG text chunk
    PngText { kind: [u8; 4], data: Vec<u8> },
    /// JFIF density information
    JfifDensity { units: u8, x: u16, y: u16 },
    /// PNG density information (pHYs chunk)
    PngDensity { x: u32, y: u32, unit: u8 },
}

/// Bundle of extracted metadata and source information.
#[derive(Debug, Clone)]
pub struct MetadataBundle {
    #[allow(dead_code)]
    pub(crate) format: SourceFormat,
    #[allow(dead_code)]
    pub(crate) payloads: Vec<Payload>,
    pub(crate) source_color: SourceColor,
    pub(crate) report: MetadataReport,
}

impl MetadataBundle {
    /// Create a bundle indicating metadata is unavailable with the given reason.
    pub fn unavailable(reason: crate::types::MetadataIssueReason) -> Self {
        Self {
            format: SourceFormat::Unknown,
            payloads: Vec::new(),
            source_color: SourceColor::Unknown,
            report: MetadataReport {
                issues: vec![crate::types::MetadataIssue {
                    category: crate::types::MetadataCategory::Unknown,
                    reason,
                    field: None,
                }],
            },
        }
    }

    /// Get the diagnostic report.
    pub fn report(&self) -> &MetadataReport {
        &self.report
    }

    /// Get the source color space.
    pub fn source_color(&self) -> SourceColor {
        self.source_color
    }
}
