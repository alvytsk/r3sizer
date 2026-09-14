/// Renders metadata-loss issues to stderr. Metadata warnings are
/// stderr-only: they never enter diagnostics JSON, stdout, or sweep summary
/// JSON (see `run.rs`/`sweep.rs`).
use std::path::Path;

use r3sizer_io::MetadataReport;

/// Write one warning line per distinct `(category, reason, field)` issue in
/// `report`, in first-seen order.
///
/// Format: `warning: <output path>: metadata <category>/<reason> (<field>)`,
/// with the parenthesized field omitted when absent. Control characters in
/// the path and field text are escaped so each issue stays on one line.
/// Never writes tag *values* -- only the category/reason/field identifiers
/// already present on `MetadataIssue`.
pub fn write_metadata_warnings(
    writer: &mut impl std::io::Write,
    path: &Path,
    report: &MetadataReport,
) -> std::io::Result<()> {
    let display_path = escape_control_chars(&path.display().to_string());
    let mut seen = Vec::new();
    for issue in &report.issues {
        let key = (issue.category, issue.reason, issue.field.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);

        let category = category_name(issue.category);
        let reason = reason_name(issue.reason);
        match &issue.field {
            Some(field) => writeln!(
                writer,
                "warning: {display_path}: metadata {category}/{reason} ({})",
                escape_control_chars(field)
            )?,
            None => writeln!(
                writer,
                "warning: {display_path}: metadata {category}/{reason}"
            )?,
        }
    }
    Ok(())
}

/// Escape control characters (C0 controls, DEL, and friends like `\n`/`\r`/
/// `\t`) so a value can never split a warning across lines or otherwise
/// corrupt terminal output. `char::escape_default` renders control chars as
/// readable escapes (`\n`, `\r`, `\t`, `\u{..}`, ...); every other character
/// -- including non-ASCII Unicode -- passes through unchanged.
fn escape_control_chars(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

fn category_name(category: r3sizer_io::MetadataCategory) -> &'static str {
    use r3sizer_io::MetadataCategory::*;
    match category {
        Exif => "exif",
        Xmp => "xmp",
        Iptc => "iptc",
        Icc => "icc",
        Text => "text",
        Thumbnail => "thumbnail",
        MakerNote => "maker_note",
        Density => "density",
        Unknown => "unknown",
    }
}

fn reason_name(reason: r3sizer_io::MetadataIssueReason) -> &'static str {
    use r3sizer_io::MetadataIssueReason::*;
    match reason {
        Unsupported => "unsupported",
        Malformed => "malformed",
        RemovedStale => "removed_stale",
        Unverified => "unverified",
        LimitExceeded => "limit_exceeded",
        MergeFailed => "merge_failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use r3sizer_io::{MetadataCategory, MetadataIssue, MetadataIssueReason};

    fn write_to_string(path: &Path, report: &MetadataReport) -> String {
        let mut buf = Vec::new();
        write_metadata_warnings(&mut buf, path, report).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn empty_report_writes_nothing() {
        let report = MetadataReport::default();
        let out = write_to_string(Path::new("/tmp/out.png"), &report);
        assert_eq!(out, "");
    }

    #[test]
    fn issue_without_field_omits_parens() {
        let report = MetadataReport {
            issues: vec![MetadataIssue {
                category: MetadataCategory::Unknown,
                reason: MetadataIssueReason::LimitExceeded,
                field: None,
            }],
        };
        let out = write_to_string(Path::new("/tmp/out.png"), &report);
        assert_eq!(
            out,
            "warning: /tmp/out.png: metadata unknown/limit_exceeded\n"
        );
    }

    #[test]
    fn issue_with_field_includes_parens() {
        let report = MetadataReport {
            issues: vec![MetadataIssue {
                category: MetadataCategory::MakerNote,
                reason: MetadataIssueReason::Unverified,
                field: Some("maker_note".to_string()),
            }],
        };
        let out = write_to_string(Path::new("/tmp/out.png"), &report);
        assert_eq!(
            out,
            "warning: /tmp/out.png: metadata maker_note/unverified (maker_note)\n"
        );
    }

    #[test]
    fn duplicate_issues_deduplicate_by_category_reason_field() {
        let issue = MetadataIssue {
            category: MetadataCategory::Exif,
            reason: MetadataIssueReason::Malformed,
            field: Some("orientation".to_string()),
        };
        let report = MetadataReport {
            issues: vec![issue.clone(), issue],
        };
        let out = write_to_string(Path::new("/tmp/out.png"), &report);
        assert_eq!(out.lines().count(), 1);
    }

    #[test]
    fn control_characters_are_escaped_in_path_and_field() {
        let report = MetadataReport {
            issues: vec![MetadataIssue {
                category: MetadataCategory::Text,
                reason: MetadataIssueReason::Malformed,
                field: Some("line1\nline2\ttabbed".to_string()),
            }],
        };
        let out = write_to_string(Path::new("/tmp/weird\npath.png"), &report);
        // Exactly one line: no raw control characters made it through.
        assert_eq!(out.lines().count(), 1);
        assert!(!out.contains('\t'));
        assert!(out.contains("\\n"));
        assert!(out.contains("\\t"));
    }
}
