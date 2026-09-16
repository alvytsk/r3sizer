use r3sizer_metadata::*;

#[test]
fn unknown_source_warns_and_leaves_output_untouched() {
    let limits = MetadataLimits::default();
    let source = extract(b"unrecognized source", &limits);
    let encoded = b"unsupported encoded output".to_vec();
    let facts = OutputFacts {
        width: 8,
        height: 4,
        orientation: OrientationAction::Preserve,
        color: ColorAction::Unverified,
    };
    let result = merge(encoded.clone(), &source, &facts, &limits);
    assert_eq!(result.bytes, encoded);
    assert!(result.report.issues.iter().any(|issue| {
        issue.category == MetadataCategory::Unknown
            && issue.reason == MetadataIssueReason::Unverified
    }));
}

#[test]
fn extract_respects_source_byte_limit() {
    let limits = MetadataLimits {
        max_source_bytes: 10,
        ..Default::default()
    };

    let source = extract(b"12345678901", &limits); // 11 bytes
    assert!(source
        .report()
        .issues
        .iter()
        .any(|issue| issue.reason == MetadataIssueReason::LimitExceeded));
}

#[test]
fn extract_within_limits_returns_unverified() {
    let limits = MetadataLimits::default();
    let source = extract(b"small", &limits);

    assert!(source
        .report()
        .issues
        .iter()
        .any(|issue| issue.reason == MetadataIssueReason::Unverified));
}

#[test]
fn metadata_report_derives_default() {
    let report = MetadataReport::default();
    assert!(report.issues.is_empty());
}

#[test]
fn orientation_action_serializable() {
    assert_eq!(
        serde_json::to_string(&OrientationAction::Preserve).unwrap(),
        "\"preserve\""
    );
    assert_eq!(
        serde_json::to_string(&OrientationAction::Normalize).unwrap(),
        "\"normalize\""
    );
}

#[test]
fn color_action_serializable() {
    assert_eq!(
        serde_json::to_string(&ColorAction::Unchanged).unwrap(),
        "\"unchanged\""
    );
    assert_eq!(
        serde_json::to_string(&ColorAction::Srgb).unwrap(),
        "\"srgb\""
    );
    assert_eq!(
        serde_json::to_string(&ColorAction::Unverified).unwrap(),
        "\"unverified\""
    );
}

#[test]
fn metadata_category_snake_case() {
    assert_eq!(
        serde_json::to_string(&MetadataCategory::MakerNote).unwrap(),
        "\"maker_note\""
    );
    assert_eq!(
        serde_json::to_string(&MetadataCategory::Unknown).unwrap(),
        "\"unknown\""
    );
}

#[test]
fn metadata_issue_reason_snake_case() {
    assert_eq!(
        serde_json::to_string(&MetadataIssueReason::RemovedStale).unwrap(),
        "\"removed_stale\""
    );
    assert_eq!(
        serde_json::to_string(&MetadataIssueReason::MergeFailed).unwrap(),
        "\"merge_failed\""
    );
}

/// Pins `MetadataLimits::default()` to the plan's named budgets: 256 MiB
/// source bytes, 8 MiB per payload, 16 MiB total retained metadata, 4,096
/// records, 4,096 EXIF entries, 32 visited IFDs, XML depth 64. Guards
/// against silent drift back to looser ad hoc values.
#[test]
fn default_limits_match_specified_budgets() {
    let limits = MetadataLimits::default();
    assert_eq!(limits.max_source_bytes, 256 * 1024 * 1024);
    assert_eq!(limits.max_payload_bytes, 8 * 1024 * 1024);
    assert_eq!(limits.max_total_metadata_bytes, 16 * 1024 * 1024);
    assert_eq!(limits.max_records, 4_096);
    assert_eq!(limits.max_exif_entries, 4_096);
    assert_eq!(limits.max_ifds, 32);
    assert_eq!(limits.max_xml_depth, 64);
}

#[test]
fn output_facts_roundtrip() {
    let facts = OutputFacts {
        width: 1920,
        height: 1080,
        orientation: OrientationAction::Preserve,
        color: ColorAction::Srgb,
    };

    let json = serde_json::to_string(&facts).unwrap();
    let deserialized: OutputFacts = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized, facts);
}
