//! Container-level metadata scanning: format sniffing and dispatch to
//! per-format scanners, plus the shared payload/issue accumulator and the
//! bounded zlib inflate helper used by PNG (iCCP/zTXt/compressed iTXt).
//!
//! No pixel decoding happens anywhere in this module tree: scanners only
//! walk container framing (segments/chunks/RIFF records) to locate and
//! validate metadata payloads.

pub(crate) mod jpeg;
pub(crate) mod png;
pub(crate) mod webp;

use std::io::Read;

use crate::bundle::{MetadataBundle, Payload, SourceColor, SourceFormat};
use crate::limits::MetadataLimits;
use crate::types::{
    MetadataCategory, MetadataExport, MetadataIssue, MetadataIssueReason, MetadataReport,
    OutputFacts,
};

/// Entry point used by `crate::extract`. Sniffs the container format from a
/// magic-byte prefix and dispatches to the matching scanner. Source bytes
/// that don't match a supported container fall back to the conservative
/// "unverified" bundle (same behavior as before this task).
pub(crate) fn extract_payloads(source: &[u8], limits: &MetadataLimits) -> MetadataBundle {
    if is_jpeg(source) {
        jpeg::extract(source, limits)
    } else if is_png(source) {
        png::extract(source, limits)
    } else if is_webp(source) {
        webp::extract(source, limits)
    } else {
        MetadataBundle::unavailable(MetadataIssueReason::Unverified)
    }
}

/// Entry point used by `crate::merge`. Sniffs the *destination* container
/// format and dispatches to the matching adapter, which inserts `prepared`'s
/// payloads into `encoded` using `img-parts`, then re-validates the result
/// with this module's own bounded scanners before returning it. A
/// destination format this crate doesn't recognize at all can't be merged
/// into, so every attempted category is reported `MergeFailed` and the
/// original bytes are returned unchanged.
pub(crate) fn embed(
    encoded: Vec<u8>,
    prepared: &MetadataBundle,
    facts: &OutputFacts,
    limits: &MetadataLimits,
) -> MetadataExport {
    if is_jpeg(&encoded) {
        jpeg::embed(encoded, prepared, facts, limits)
    } else if is_png(&encoded) {
        png::embed(encoded, prepared, facts, limits)
    } else if is_webp(&encoded) {
        webp::embed(encoded, prepared, facts, limits)
    } else {
        rollback(encoded, prepared)
    }
}

fn is_jpeg(source: &[u8]) -> bool {
    source.len() >= 2 && source[0] == 0xFF && source[1] == 0xD8
}

fn is_png(source: &[u8]) -> bool {
    source.starts_with(b"\x89PNG\r\n\x1a\n")
}

fn is_webp(source: &[u8]) -> bool {
    source.len() >= 12 && &source[0..4] == b"RIFF" && &source[8..12] == b"WEBP"
}

/// The category a payload belongs to, used both for "this payload's
/// category isn't supported by this destination" issues and for the
/// `MergeFailed` rollback sweep.
pub(crate) fn category_of(payload: &Payload) -> MetadataCategory {
    match payload {
        Payload::Exif(_) => MetadataCategory::Exif,
        Payload::Xmp(_) => MetadataCategory::Xmp,
        Payload::Iptc(_) => MetadataCategory::Iptc,
        Payload::Icc(_) => MetadataCategory::Icc,
        Payload::JpegComment(_) | Payload::PngText { .. } => MetadataCategory::Text,
        Payload::JfifDensity { .. } | Payload::PngDensity { .. } => MetadataCategory::Density,
    }
}

/// A short, fixed field name identifying a payload's kind in an issue --
/// never the payload's own content.
pub(crate) fn field_of(payload: &Payload) -> &'static str {
    match payload {
        Payload::Exif(_) => "exif",
        Payload::Xmp(_) => "xmp",
        Payload::Iptc(_) => "iptc",
        Payload::Icc(_) => "icc",
        Payload::JpegComment(_) => "com",
        Payload::PngText { .. } => "text",
        Payload::JfifDensity { .. } => "jfif",
        Payload::PngDensity { .. } => "phys",
    }
}

pub(crate) fn merge_issue(payload: &Payload, reason: MetadataIssueReason) -> MetadataIssue {
    MetadataIssue {
        category: category_of(payload),
        reason,
        field: Some(field_of(payload).to_string()),
    }
}

/// A payload category this adapter never handles for this destination
/// format at all (e.g. IPTC into PNG/WebP, PNG text into JPEG/WebP).
pub(crate) fn unsupported(payload: &Payload) -> MetadataIssue {
    merge_issue(payload, MetadataIssueReason::Unsupported)
}

/// A payload that would exceed a hard size limit for this destination
/// format (e.g. a JPEG APP segment's 65,533-byte payload cap). Never
/// truncated or split -- omitted outright.
pub(crate) fn too_large(payload: &Payload) -> MetadataIssue {
    merge_issue(payload, MetadataIssueReason::LimitExceeded)
}

/// Roll back a merge attempt: return the original destination bytes
/// unchanged, with every payload category `prepared` tried to embed
/// recorded as `MergeFailed` -- on top of (not replacing) `prepared`'s own
/// extraction/correction/policy issues, since those already happened and
/// are true regardless of whether the merge itself succeeded. No partial
/// "this one made it in" claim survives a rollback: whatever issues an
/// adapter accumulated while attempting individual payloads (`Unsupported`,
/// `LimitExceeded`) are discarded in favor of a blanket `MergeFailed` for
/// every category that was attempted.
pub(crate) fn rollback(original: Vec<u8>, prepared: &MetadataBundle) -> MetadataExport {
    let mut issues = prepared.report().issues.clone();
    for payload in &prepared.payloads {
        issues.push(merge_issue(payload, MetadataIssueReason::MergeFailed));
    }
    MetadataExport {
        bytes: original,
        report: MetadataReport { issues },
    }
}

/// Typed decompression outcome, so callers never classify failures by
/// matching on error strings.
#[derive(Debug)]
pub(crate) enum InflateError {
    /// The zlib stream was invalid or truncated.
    Malformed,
    /// Decompressed output exceeded the caller-supplied bound.
    LimitExceeded,
}

/// Bounded zlib inflate. Reads at most `max + 1` bytes so an oversized
/// stream is detected without ever materializing an unbounded allocation.
pub(crate) fn inflate_bounded(data: &[u8], max: usize) -> Result<Vec<u8>, InflateError> {
    let cap = max.checked_add(1).ok_or(InflateError::LimitExceeded)?;
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut result = Vec::new();
    decoder
        .by_ref()
        .take(cap as u64)
        .read_to_end(&mut result)
        .map_err(|_| InflateError::Malformed)?;
    if result.len() > max {
        return Err(InflateError::LimitExceeded);
    }
    Ok(result)
}

fn payload_len(payload: &Payload) -> usize {
    match payload {
        Payload::Exif(b)
        | Payload::Xmp(b)
        | Payload::Iptc(b)
        | Payload::Icc(b)
        | Payload::JpegComment(b) => b.len(),
        Payload::PngText { data, .. } => data.len(),
        Payload::JfifDensity { .. } => 5,
        Payload::PngDensity { .. } => 9,
    }
}

/// Accumulates validated payloads and issues while a format scanner walks a
/// container. Owns the budget bookkeeping (`max_payload_bytes`,
/// `max_total_metadata_bytes`, `max_records`) so every scanner enforces the
/// same limits the same way, and owns the cross-format singleton rule for
/// EXIF/XMP/IPTC: a container should carry at most one of each, so a second
/// sighting drops the category as ambiguous rather than guessing which one
/// is authoritative.
pub(crate) struct Collector<'a> {
    limits: &'a MetadataLimits,
    payloads: Vec<Payload>,
    issues: Vec<MetadataIssue>,
    total_bytes: usize,
    exif_count: usize,
    exif_pending: Option<Vec<u8>>,
    xmp_count: usize,
    xmp_pending: Option<Vec<u8>>,
    iptc_count: usize,
    iptc_pending: Option<Vec<u8>>,
    icc_count: usize,
    icc_pending: Option<Vec<u8>>,
}

impl<'a> Collector<'a> {
    pub(crate) fn new(limits: &'a MetadataLimits) -> Self {
        Self {
            limits,
            payloads: Vec::new(),
            issues: Vec::new(),
            total_bytes: 0,
            exif_count: 0,
            exif_pending: None,
            xmp_count: 0,
            xmp_pending: None,
            iptc_count: 0,
            iptc_pending: None,
            icc_count: 0,
            icc_pending: None,
        }
    }

    pub(crate) fn push_issue(
        &mut self,
        category: MetadataCategory,
        reason: MetadataIssueReason,
        field: Option<String>,
    ) {
        self.issues.push(MetadataIssue {
            category,
            reason,
            field,
        });
    }

    /// Remaining bytes this collector will still accept for a single
    /// payload, accounting for both the per-payload cap and what's left of
    /// the total budget. Used to bound decompression (iCCP/zTXt/iTXt).
    pub(crate) fn remaining_budget(&self) -> usize {
        self.limits
            .max_payload_bytes
            .min(self.limits.max_total_metadata_bytes.saturating_sub(self.total_bytes))
    }

    /// Reserve `len` bytes against the budget *immediately*, independent of
    /// whether those bytes ever become a stored payload. This is the one
    /// thing that makes `remaining_budget` mean anything across multiple
    /// chunks: without it, a caller that decompresses to validate a chunk
    /// and then discards or supersedes the result (a PNG zTXt/iTXt chunk
    /// whose original compressed bytes are kept instead of the decompressed
    /// ones; a duplicate EXIF/XMP/IPTC/ICC sighting that loses the
    /// singleton race) would never shrink the budget, letting a file with
    /// many such chunks each trigger a full-budget-sized decompression.
    /// Returns `false` (and records a `LimitExceeded` issue) if charging
    /// would exceed either the per-payload or total-metadata limit.
    fn charge(&mut self, len: usize, category: MetadataCategory, field: &str) -> bool {
        if len > self.limits.max_payload_bytes
            || self.total_bytes.saturating_add(len) > self.limits.max_total_metadata_bytes
        {
            self.push_issue(category, MetadataIssueReason::LimitExceeded, Some(field.to_string()));
            return false;
        }
        self.total_bytes += len;
        true
    }

    /// Validate size/record limits and, if they hold, store the payload.
    /// Returns `false` (and records a `LimitExceeded` issue) if the payload
    /// was rejected.
    pub(crate) fn add_payload(
        &mut self,
        payload: Payload,
        category: MetadataCategory,
        field: &str,
    ) -> bool {
        let len = payload_len(&payload);
        if len > self.limits.max_payload_bytes
            || self.total_bytes.saturating_add(len) > self.limits.max_total_metadata_bytes
            || self.payloads.len() >= self.limits.max_records
        {
            self.push_issue(category, MetadataIssueReason::LimitExceeded, Some(field.to_string()));
            return false;
        }
        self.total_bytes += len;
        self.payloads.push(payload);
        true
    }

    /// Store a payload whose bytes were already charged against the budget
    /// elsewhere (a pending singleton resolved in `finish`, or a chunk that
    /// charged its real decompressed size up front via `charge` before
    /// storing a smaller original-encoding copy). Only `max_records` is
    /// still enforced here, so bytes are never double-charged.
    fn store_prevalidated(&mut self, payload: Payload, category: MetadataCategory, field: &str) {
        if self.payloads.len() >= self.limits.max_records {
            self.push_issue(category, MetadataIssueReason::LimitExceeded, Some(field.to_string()));
            return;
        }
        self.payloads.push(payload);
    }

    /// Decompress-then-store helper for PNG zTXt/non-XMP-iTXt: `decoded_len`
    /// is charged against the budget immediately (the real cost of the
    /// decompression that already happened), and only if that succeeds is
    /// `payload` (typically the original, still-compressed bytes) stored.
    pub(crate) fn charge_and_store(
        &mut self,
        decoded_len: usize,
        payload: Payload,
        category: MetadataCategory,
        field: &str,
    ) {
        if self.charge(decoded_len, category, field) {
            self.store_prevalidated(payload, category, field);
        }
    }

    /// Offer an EXIF candidate (from JPEG APP1, PNG eXIf, or WebP EXIF).
    /// A second sighting makes the category ambiguous; both are dropped.
    /// Charged against the budget immediately (not deferred to `finish`),
    /// so a run of oversized or duplicate candidates can't each dodge the
    /// budget check while `total_bytes` sits unmoved.
    pub(crate) fn offer_exif(&mut self, bytes: Vec<u8>) {
        if !self.charge(bytes.len(), MetadataCategory::Exif, "exif") {
            return;
        }
        self.exif_count += 1;
        if self.exif_count == 1 {
            self.exif_pending = Some(bytes);
        } else {
            self.exif_pending = None;
            if self.exif_count == 2 {
                self.push_issue(
                    MetadataCategory::Exif,
                    MetadataIssueReason::Malformed,
                    Some("duplicate".to_string()),
                );
            }
        }
    }

    /// Offer a standard-XMP candidate (JPEG APP1, PNG iTXt, or WebP XMP).
    /// Charged against the budget immediately; see `offer_exif`.
    pub(crate) fn offer_xmp(&mut self, bytes: Vec<u8>) {
        if !self.charge(bytes.len(), MetadataCategory::Xmp, "xmp") {
            return;
        }
        self.xmp_count += 1;
        if self.xmp_count == 1 {
            self.xmp_pending = Some(bytes);
        } else {
            self.xmp_pending = None;
            if self.xmp_count == 2 {
                self.push_issue(
                    MetadataCategory::Xmp,
                    MetadataIssueReason::Malformed,
                    Some("duplicate".to_string()),
                );
            }
        }
    }

    /// Offer a validated IPTC IIM candidate (JPEG APP13 8BIM 0x0404).
    /// Charged against the budget immediately; see `offer_exif`.
    pub(crate) fn offer_iptc(&mut self, bytes: Vec<u8>) {
        if !self.charge(bytes.len(), MetadataCategory::Iptc, "iptc") {
            return;
        }
        self.iptc_count += 1;
        if self.iptc_count == 1 {
            self.iptc_pending = Some(bytes);
        } else {
            self.iptc_pending = None;
            if self.iptc_count == 2 {
                self.push_issue(
                    MetadataCategory::Iptc,
                    MetadataIssueReason::Malformed,
                    Some("duplicate".to_string()),
                );
            }
        }
    }

    /// Offer an ICC profile candidate that is already fully decoded and
    /// validated (PNG iCCP, WebP ICCP). JPEG's multi-segment ICC assembly
    /// has its own duplicate/sequence handling and doesn't go through here.
    /// Charged against the budget immediately (the bytes here are already
    /// decompressed, for PNG iCCP); see `offer_exif`.
    pub(crate) fn offer_icc(&mut self, bytes: Vec<u8>) {
        if !self.charge(bytes.len(), MetadataCategory::Icc, "icc") {
            return;
        }
        self.icc_count += 1;
        if self.icc_count == 1 {
            self.icc_pending = Some(bytes);
        } else {
            self.icc_pending = None;
            if self.icc_count == 2 {
                self.push_issue(
                    MetadataCategory::Icc,
                    MetadataIssueReason::Malformed,
                    Some("duplicate".to_string()),
                );
            }
        }
    }

    /// Resolve pending singleton candidates and produce the final bundle.
    /// Their bytes were already charged against the budget when offered
    /// (`offer_exif`/`offer_xmp`/`offer_iptc`/`offer_icc`), so this only
    /// enforces `max_records` via `store_prevalidated` — charging again
    /// here would double-count. `source_color` is a caller-computed policy
    /// decision (e.g. whether an ICC profile survived); callers that need
    /// to know whether ICC won the singleton race should inspect the
    /// returned bundle's payloads via `bundle.payloads` before overriding.
    pub(crate) fn finish(mut self, format: SourceFormat) -> MetadataBundle {
        if let Some(bytes) = self.exif_pending.take() {
            self.store_prevalidated(Payload::Exif(bytes), MetadataCategory::Exif, "exif");
        }
        if let Some(bytes) = self.xmp_pending.take() {
            self.store_prevalidated(Payload::Xmp(bytes), MetadataCategory::Xmp, "xmp");
        }
        if let Some(bytes) = self.iptc_pending.take() {
            self.store_prevalidated(Payload::Iptc(bytes), MetadataCategory::Iptc, "iptc");
        }
        if let Some(bytes) = self.icc_pending.take() {
            self.store_prevalidated(Payload::Icc(bytes), MetadataCategory::Icc, "icc");
        }
        MetadataBundle::new(
            format,
            self.payloads,
            SourceColor::Unspecified,
            MetadataReport {
                issues: self.issues,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_known_signatures() {
        assert!(is_jpeg(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(!is_jpeg(&[0xFF]));
        assert!(is_png(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x00"));
        assert!(is_webp(b"RIFF\x00\x00\x00\x00WEBPVP8 "));
        assert!(!is_webp(b"RIFF\x00\x00\x00\x00AVI garbage"));
    }

    #[test]
    fn inflate_bounded_rejects_oversized_stream() {
        use std::io::Write;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&vec![0u8; 1024]).unwrap();
        let compressed = encoder.finish().unwrap();
        let err = inflate_bounded(&compressed, 10).expect_err("should exceed bound");
        assert!(matches!(err, InflateError::LimitExceeded));
    }

    #[test]
    fn inflate_bounded_rejects_garbage() {
        let err = inflate_bounded(b"not zlib data", 1024).expect_err("should fail to decode");
        assert!(matches!(err, InflateError::Malformed));
    }

    #[test]
    fn inflate_bounded_accepts_within_budget() {
        use std::io::Write;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"hello world").unwrap();
        let compressed = encoder.finish().unwrap();
        let out = inflate_bounded(&compressed, 1024).expect("should decode");
        assert_eq!(out, b"hello world");
    }

    #[test]
    fn duplicate_exif_offers_are_dropped_as_ambiguous() {
        let limits = MetadataLimits::default();
        let mut collector = Collector::new(&limits);
        collector.offer_exif(vec![1, 2, 3]);
        collector.offer_exif(vec![4, 5, 6]);
        let bundle = collector.finish(SourceFormat::Jpeg);
        assert!(bundle
            .payloads
            .iter()
            .all(|p| !matches!(p, Payload::Exif(_))));
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Exif && i.reason == MetadataIssueReason::Malformed
        }));
    }
}
