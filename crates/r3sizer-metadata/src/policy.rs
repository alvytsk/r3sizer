//! Color and density preservation policy across a metadata bundle.
//!
//! `prepare()` is the single place that turns an extraction `MetadataBundle`
//! plus the caller's `OutputFacts`/`destination_icc` into the bundle that
//! gets embedded: it runs `exif::correct`/`xmp::correct` over the EXIF/XMP
//! payloads (those correctors need `OutputFacts` but not `destination_icc`),
//! decides whether the source ICC profile survives (which needs
//! `destination_icc`, so it can't live inside either corrector), and merges
//! every issue -- extraction plus correction -- into one deduplicated list.
//! Density payloads (JFIF/PNG resolution) are never touched: this crate
//! keeps DPI for same-format exports regardless of how much the pixel
//! dimensions shrank, so there's nothing to decide for them.

use crate::bundle::{MetadataBundle, Payload};
use crate::limits::{checked_range, MetadataLimits};
use crate::types::{
    ColorAction, MetadataCategory, MetadataIssue, MetadataIssueReason, MetadataReport, OutputFacts,
};
use crate::{exif, xmp};

/// Apply color/density policy and EXIF/XMP correction to `bundle`, and
/// deduplicate the resulting issue list -- extraction issues plus every
/// correction issue -- by `(category, reason, field)`, keeping first-seen
/// order.
pub fn prepare(
    bundle: &MetadataBundle,
    facts: &OutputFacts,
    destination_icc: Option<&[u8]>,
    limits: &MetadataLimits,
) -> MetadataBundle {
    let mut issues = bundle.report().issues.clone();
    let mut payloads = Vec::with_capacity(bundle.payloads.len());

    for payload in &bundle.payloads {
        match payload {
            Payload::Exif(bytes) => {
                let (corrected, mut new_issues) = exif::correct(bytes, facts, limits);
                issues.append(&mut new_issues);
                if let Some(b) = corrected {
                    payloads.push(Payload::Exif(b));
                }
            }
            Payload::Xmp(bytes) => {
                let (corrected, mut new_issues) = xmp::correct(bytes, facts, limits);
                issues.append(&mut new_issues);
                if let Some(b) = corrected {
                    payloads.push(Payload::Xmp(b));
                }
            }
            Payload::Icc(bytes) => {
                match icc_decision(bytes, facts.color, destination_icc, limits) {
                    Ok(()) => payloads.push(payload.clone()),
                    Err(issue) => issues.push(issue),
                }
            }
            other => payloads.push(other.clone()),
        }
    }

    MetadataBundle::new(
        bundle.format,
        payloads,
        bundle.source_color(),
        MetadataReport {
            issues: dedup_issues(issues),
        },
    )
}

fn issue(category: MetadataCategory, reason: MetadataIssueReason, field: &str) -> MetadataIssue {
    MetadataIssue {
        category,
        reason,
        field: Some(field.to_string()),
    }
}

/// Decide whether a source ICC profile survives into the output.
///
/// `Unchanged` is an explicit caller guarantee that output pixels equal
/// source pixels, so any structurally valid source ICC is retained as-is.
/// `Srgb` keeps whatever sRGB declarations the caller writes elsewhere
/// (EXIF/XMP); the source ICC survives only if it is byte-identical to a
/// structurally valid `destination_icc` (proving it, rather than assuming
/// an unrelated profile happens to also mean sRGB). `Unverified` never
/// retains a source color declaration, because no color transform applied
/// during processing has been proven correct.
///
/// Structural validation (declared size, `acsp` header signature, in-bounds
/// tag table) only proves the bytes form a well-shaped ICC profile -- it
/// does not prove the profile is sRGB-equivalent.
fn icc_decision(
    bytes: &[u8],
    color: ColorAction,
    destination_icc: Option<&[u8]>,
    limits: &MetadataLimits,
) -> Result<(), MetadataIssue> {
    if !is_structurally_valid_icc(bytes, limits) {
        return Err(issue(
            MetadataCategory::Icc,
            MetadataIssueReason::Malformed,
            "icc",
        ));
    }
    match color {
        ColorAction::Unchanged => Ok(()),
        ColorAction::Srgb => {
            let matches_destination = destination_icc
                .map(|dest| dest == bytes && is_structurally_valid_icc(dest, limits))
                .unwrap_or(false);
            if matches_destination {
                Ok(())
            } else {
                Err(issue(
                    MetadataCategory::Icc,
                    MetadataIssueReason::Unverified,
                    "icc",
                ))
            }
        }
        ColorAction::Unverified => Err(issue(
            MetadataCategory::Icc,
            MetadataIssueReason::Unverified,
            "icc",
        )),
    }
}

/// Structural-only ICC validation: declared profile size matches the byte
/// count, the `acsp` file signature is present at its fixed offset, and
/// every tag-table entry's offset/size lands in bounds. This says nothing
/// about the profile's actual color transform.
fn is_structurally_valid_icc(bytes: &[u8], limits: &MetadataLimits) -> bool {
    const HEADER_LEN: usize = 128;
    const TAG_TABLE_START: usize = HEADER_LEN + 4;

    if bytes.len() < TAG_TABLE_START || bytes.len() > limits.max_payload_bytes {
        return false;
    }
    let declared_size = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if declared_size != bytes.len() {
        return false;
    }
    if &bytes[36..40] != b"acsp" {
        return false;
    }
    let tag_count = u32::from_be_bytes([
        bytes[HEADER_LEN],
        bytes[HEADER_LEN + 1],
        bytes[HEADER_LEN + 2],
        bytes[HEADER_LEN + 3],
    ]) as usize;
    let Some(table_range) = checked_range(TAG_TABLE_START, tag_count, 12, bytes.len()) else {
        return false;
    };
    for i in 0..tag_count {
        let pos = table_range.start + i * 12;
        let offset = u32::from_be_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let size = u32::from_be_bytes([
            bytes[pos + 8],
            bytes[pos + 9],
            bytes[pos + 10],
            bytes[pos + 11],
        ]) as usize;
        if checked_range(offset, size, 1, bytes.len()).is_none() {
            return false;
        }
    }
    true
}

/// Deduplicate by `(category, reason, field)`, keeping first-seen order.
/// Linear `contains` scan rather than a `HashSet`: `MetadataCategory`/
/// `MetadataIssueReason` don't derive `Hash` (out of this task's scope to
/// add). That's fine here for two independent reasons -- not
/// `MetadataLimits::max_records`, which bounds extracted *payload* records
/// per container (see `containers/mod.rs`), not how many issues a single
/// XMP packet's repeated removed elements can generate. First, the
/// deduplicated key space is small and fixed: a handful of
/// `MetadataCategory`/`MetadataIssueReason` variants crossed with `field`
/// values that are always short literal strings (never user data), so
/// `seen` stops growing almost immediately regardless of how many raw
/// issues come in. Second, the raw issue count itself is still bounded --
/// by `MetadataLimits::max_payload_bytes`, which caps how many removable
/// elements a single EXIF/XMP payload can even contain.
///
/// ponytail: O(n^2) in the deduplicated-key count, which stays tiny;
/// revisit only if these enums ever grow `Hash` derives for other reasons.
fn dedup_issues(issues: Vec<MetadataIssue>) -> Vec<MetadataIssue> {
    let mut seen: Vec<(MetadataCategory, MetadataIssueReason, Option<String>)> = Vec::new();
    let mut out = Vec::with_capacity(issues.len());
    for issue in issues {
        let key = (issue.category, issue.reason, issue.field.clone());
        if !seen.contains(&key) {
            seen.push(key);
            out.push(issue);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::SourceFormat;
    use crate::types::OrientationAction;

    fn valid_icc(byte: u8) -> Vec<u8> {
        // Minimal structurally-valid ICC profile: 128-byte header (size +
        // `acsp` signature at the fixed offset) followed by a zero-entry
        // tag table. `byte` varies the content so two profiles can differ.
        let mut b = vec![byte; 132];
        let size = (b.len() as u32).to_be_bytes();
        b[0..4].copy_from_slice(&size);
        b[36..40].copy_from_slice(b"acsp");
        b[128..132].copy_from_slice(&0u32.to_be_bytes());
        b
    }

    fn facts(color: ColorAction) -> OutputFacts {
        OutputFacts {
            width: 100,
            height: 50,
            orientation: OrientationAction::Preserve,
            color,
        }
    }

    fn icc_only_bundle(icc: Vec<u8>) -> MetadataBundle {
        MetadataBundle::new(
            SourceFormat::Jpeg,
            vec![Payload::Icc(icc)],
            crate::bundle::SourceColor::Other,
            MetadataReport { issues: Vec::new() },
        )
    }

    #[test]
    fn valid_icc_structure_passes() {
        assert!(is_structurally_valid_icc(
            &valid_icc(1),
            &MetadataLimits::default()
        ));
    }

    #[test]
    fn truncated_icc_fails_structural_check() {
        assert!(!is_structurally_valid_icc(
            &[0u8; 10],
            &MetadataLimits::default()
        ));
    }

    #[test]
    fn wrong_signature_fails_structural_check() {
        let mut icc = valid_icc(1);
        icc[36] = b'X';
        assert!(!is_structurally_valid_icc(&icc, &MetadataLimits::default()));
    }

    #[test]
    fn declared_size_mismatch_fails_structural_check() {
        let mut icc = valid_icc(1);
        icc[0..4].copy_from_slice(&999u32.to_be_bytes());
        assert!(!is_structurally_valid_icc(&icc, &MetadataLimits::default()));
    }

    #[test]
    fn out_of_bounds_tag_table_entry_fails_structural_check() {
        let mut icc = valid_icc(1);
        icc[128..132].copy_from_slice(&1u32.to_be_bytes()); // claims 1 tag, no room
        assert!(!is_structurally_valid_icc(&icc, &MetadataLimits::default()));
    }

    #[test]
    fn unchanged_keeps_valid_icc_without_issue() {
        let bundle = icc_only_bundle(valid_icc(1));
        let prepared = prepare(
            &bundle,
            &facts(ColorAction::Unchanged),
            None,
            &MetadataLimits::default(),
        );
        assert!(prepared.report().issues.is_empty());
    }

    #[test]
    fn srgb_keeps_icc_equal_to_destination() {
        let dest = valid_icc(7);
        let bundle = icc_only_bundle(dest.clone());
        let prepared = prepare(
            &bundle,
            &facts(ColorAction::Srgb),
            Some(&dest),
            &MetadataLimits::default(),
        );
        assert!(prepared.report().issues.is_empty());
    }

    #[test]
    fn srgb_drops_icc_that_differs_from_destination() {
        let bundle = icc_only_bundle(valid_icc(1));
        let prepared = prepare(
            &bundle,
            &facts(ColorAction::Srgb),
            Some(&valid_icc(2)),
            &MetadataLimits::default(),
        );
        assert!(prepared.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Unverified
        }));
    }

    #[test]
    fn srgb_drops_icc_when_no_destination_given() {
        let bundle = icc_only_bundle(valid_icc(1));
        let prepared = prepare(
            &bundle,
            &facts(ColorAction::Srgb),
            None,
            &MetadataLimits::default(),
        );
        assert!(prepared.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Unverified
        }));
    }

    #[test]
    fn unverified_always_drops_icc() {
        let icc = valid_icc(1);
        let bundle = icc_only_bundle(icc.clone());
        let prepared = prepare(
            &bundle,
            &facts(ColorAction::Unverified),
            Some(&icc),
            &MetadataLimits::default(),
        );
        assert!(prepared.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Unverified
        }));
    }

    #[test]
    fn malformed_icc_reported_regardless_of_action() {
        let bundle = icc_only_bundle(vec![0u8; 4]);
        let prepared = prepare(
            &bundle,
            &facts(ColorAction::Unchanged),
            None,
            &MetadataLimits::default(),
        );
        assert!(prepared.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Malformed
        }));
    }

    #[test]
    fn density_payloads_pass_through_unchanged() {
        let bundle = MetadataBundle::new(
            SourceFormat::Jpeg,
            vec![Payload::JfifDensity {
                units: 1,
                x: 300,
                y: 300,
            }],
            crate::bundle::SourceColor::Unspecified,
            MetadataReport { issues: Vec::new() },
        );
        // Facts describe a large downscale; density must not be adjusted
        // or flagged just because pixel dimensions shrank.
        let shrink_facts = OutputFacts {
            width: 10,
            height: 5,
            orientation: OrientationAction::Preserve,
            color: ColorAction::Unchanged,
        };
        let prepared = prepare(&bundle, &shrink_facts, None, &MetadataLimits::default());
        assert!(prepared.report().issues.is_empty());
    }

    #[test]
    fn duplicate_issues_from_extraction_and_correction_are_deduplicated() {
        let extraction_issue = MetadataIssue {
            category: MetadataCategory::Icc,
            reason: MetadataIssueReason::Unverified,
            field: Some("icc".to_string()),
        };
        let bundle = MetadataBundle::new(
            SourceFormat::Jpeg,
            vec![Payload::Icc(valid_icc(1))],
            crate::bundle::SourceColor::Other,
            MetadataReport {
                issues: vec![extraction_issue.clone(), extraction_issue],
            },
        );
        // Srgb with no destination also raises the identical Icc/Unverified
        // issue, so the prepared report must still only carry it once.
        let prepared = prepare(
            &bundle,
            &facts(ColorAction::Srgb),
            None,
            &MetadataLimits::default(),
        );
        let count = prepared
            .report()
            .issues
            .iter()
            .filter(|i| {
                i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Unverified
            })
            .count();
        assert_eq!(count, 1);
    }
}
