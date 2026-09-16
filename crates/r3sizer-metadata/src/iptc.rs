//! Photoshop Image Resource Block (IRB) and IPTC IIM parsing.
//!
//! Used by the JPEG scanner (APP13 `Photoshop 3.0\0` segments) to walk the
//! `8BIM`-signed resource records that follow. Only resource `0x0404`
//! (IPTC-NAA record) is retained, after its own bounded validation.
//! Thumbnail resources are flagged for removal; everything else is named
//! and left out rather than copied, since previews and dimension-dependent
//! data go stale across a resize.

use crate::containers::Collector;
use crate::limits::checked_range;
use crate::types::{MetadataCategory, MetadataIssueReason};

const RESOURCE_IPTC: u16 = 0x0404;
const RESOURCE_THUMBNAIL_4: u16 = 0x0409;
const RESOURCE_THUMBNAIL_5: u16 = 0x040C;

/// Walk `8BIM` image resource blocks in a Photoshop IRB payload (the bytes
/// following the `Photoshop 3.0\0` identifier in a JPEG APP13 segment).
pub(crate) fn parse_irbs(data: &[u8], collector: &mut Collector) {
    let mut pos = 0usize;
    while pos < data.len() {
        let sig_range = match checked_range(pos, 4, 1, data.len()) {
            Some(r) => r,
            None => {
                collector.push_issue(
                    MetadataCategory::Unknown,
                    MetadataIssueReason::Malformed,
                    Some("irb_truncated".to_string()),
                );
                return;
            }
        };
        if &data[sig_range.clone()] != b"8BIM" {
            collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Malformed,
                Some("irb_signature".to_string()),
            );
            return;
        }
        pos = sig_range.end;

        let id_range = match checked_range(pos, 2, 1, data.len()) {
            Some(r) => r,
            None => {
                collector.push_issue(
                    MetadataCategory::Unknown,
                    MetadataIssueReason::Malformed,
                    Some("irb_truncated".to_string()),
                );
                return;
            }
        };
        let resource_id = u16::from_be_bytes([data[id_range.start], data[id_range.start + 1]]);
        pos = id_range.end;

        // Pascal string name: 1-byte length + name bytes, padded so the
        // (length byte + name) field has even total size.
        let name_len_range = match checked_range(pos, 1, 1, data.len()) {
            Some(r) => r,
            None => {
                collector.push_issue(
                    MetadataCategory::Unknown,
                    MetadataIssueReason::Malformed,
                    Some("irb_truncated".to_string()),
                );
                return;
            }
        };
        let name_len = data[name_len_range.start] as usize;
        pos = name_len_range.end;
        let name_range = match checked_range(pos, name_len, 1, data.len()) {
            Some(r) => r,
            None => {
                collector.push_issue(
                    MetadataCategory::Unknown,
                    MetadataIssueReason::Malformed,
                    Some("irb_truncated".to_string()),
                );
                return;
            }
        };
        pos = name_range.end;
        if !(1 + name_len).is_multiple_of(2) {
            let pad_range = match checked_range(pos, 1, 1, data.len()) {
                Some(r) => r,
                None => {
                    collector.push_issue(
                        MetadataCategory::Unknown,
                        MetadataIssueReason::Malformed,
                        Some("irb_truncated".to_string()),
                    );
                    return;
                }
            };
            pos = pad_range.end;
        }

        let len_range = match checked_range(pos, 4, 1, data.len()) {
            Some(r) => r,
            None => {
                collector.push_issue(
                    MetadataCategory::Unknown,
                    MetadataIssueReason::Malformed,
                    Some("irb_truncated".to_string()),
                );
                return;
            }
        };
        let resource_len = u32::from_be_bytes([
            data[len_range.start],
            data[len_range.start + 1],
            data[len_range.start + 2],
            data[len_range.start + 3],
        ]) as usize;
        pos = len_range.end;

        let data_range = match checked_range(pos, resource_len, 1, data.len()) {
            Some(r) => r,
            None => {
                collector.push_issue(
                    MetadataCategory::Unknown,
                    MetadataIssueReason::Malformed,
                    Some("irb_truncated".to_string()),
                );
                return;
            }
        };
        let resource_data = &data[data_range.clone()];
        pos = data_range.end;
        if !resource_len.is_multiple_of(2) {
            let pad_range = match checked_range(pos, 1, 1, data.len()) {
                Some(r) => r,
                None => {
                    collector.push_issue(
                        MetadataCategory::Unknown,
                        MetadataIssueReason::Malformed,
                        Some("irb_truncated".to_string()),
                    );
                    return;
                }
            };
            pos = pad_range.end;
        }

        match resource_id {
            RESOURCE_IPTC => {
                if validate_iptc_iim(resource_data) {
                    collector.offer_iptc(resource_data.to_vec());
                } else {
                    collector.push_issue(
                        MetadataCategory::Iptc,
                        MetadataIssueReason::Malformed,
                        Some("iptc_iim".to_string()),
                    );
                }
            }
            RESOURCE_THUMBNAIL_4 | RESOURCE_THUMBNAIL_5 => {
                collector.push_issue(
                    MetadataCategory::Thumbnail,
                    MetadataIssueReason::RemovedStale,
                    Some(format!("0x{resource_id:04x}")),
                );
            }
            other => {
                collector.push_issue(
                    MetadataCategory::Unknown,
                    MetadataIssueReason::Unsupported,
                    Some(format!("0x{other:04x}")),
                );
            }
        }
    }
}

/// Validate IPTC IIM dataset framing without interpreting individual
/// fields: every dataset starts with the `0x1C` tag marker, followed by a
/// record number, dataset number, and a length (standard 2-byte form, or
/// the rarely-used extended form where the top bit of the first length
/// byte signals a count of following length-bytes). Returns `false` on any
/// truncation or malformed tag so the caller can reject the whole record
/// rather than trust partially-parsed data.
fn validate_iptc_iim(data: &[u8]) -> bool {
    let mut pos = 0usize;
    while pos < data.len() {
        if data[pos] != 0x1C {
            return false;
        }
        let header_range = match checked_range(pos, 3, 1, data.len()) {
            Some(r) => r,
            None => return false,
        };
        let mut p = header_range.end;

        if p >= data.len() {
            return false;
        }
        let first = data[p];
        let value_len = if first & 0x80 != 0 {
            let n = (first & 0x7F) as usize;
            p += 1;
            if n > std::mem::size_of::<usize>() {
                return false;
            }
            let len_range = match checked_range(p, n, 1, data.len()) {
                Some(r) => r,
                None => return false,
            };
            let mut len: usize = 0;
            for &b in &data[len_range.clone()] {
                len = (len << 8) | b as usize;
            }
            p = len_range.end;
            len
        } else {
            let len_range = match checked_range(p, 2, 1, data.len()) {
                Some(r) => r,
                None => return false,
            };
            let len =
                u16::from_be_bytes([data[len_range.start], data[len_range.start + 1]]) as usize;
            p = len_range.end;
            len
        };

        let value_range = match checked_range(p, value_len, 1, data.len()) {
            Some(r) => r,
            None => return false,
        };
        pos = value_range.end;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Payload;

    fn irb(id: u16, name: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = b"8BIM".to_vec();
        out.extend_from_slice(&id.to_be_bytes());
        out.push(name.len() as u8);
        out.extend_from_slice(name);
        if !(1 + name.len()).is_multiple_of(2) {
            out.push(0);
        }
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        if !data.len().is_multiple_of(2) {
            out.push(0);
        }
        out
    }

    fn iptc_dataset(record: u8, dataset: u8, value: &[u8]) -> Vec<u8> {
        let mut out = vec![0x1C, record, dataset];
        out.extend_from_slice(&(value.len() as u16).to_be_bytes());
        out.extend_from_slice(value);
        out
    }

    #[test]
    fn valid_iptc_record_is_retained() {
        let iptc_bytes = iptc_dataset(2, 5, b"hello");
        let block = irb(RESOURCE_IPTC, b"", &iptc_bytes);
        let limits = crate::limits::MetadataLimits::default();
        let mut collector = Collector::new(&limits);
        parse_irbs(&block, &mut collector);
        let bundle = collector.finish(crate::bundle::SourceFormat::Jpeg);
        assert!(bundle.report().issues.is_empty());
        assert!(bundle
            .payloads
            .iter()
            .any(|p| matches!(p, Payload::Iptc(b) if b == &iptc_bytes)));
    }

    #[test]
    fn thumbnail_resources_are_removed_as_stale() {
        let block = irb(RESOURCE_THUMBNAIL_5, b"", &[1, 2, 3, 4]);
        let limits = crate::limits::MetadataLimits::default();
        let mut collector = Collector::new(&limits);
        parse_irbs(&block, &mut collector);
        let bundle = collector.finish(crate::bundle::SourceFormat::Jpeg);
        assert!(bundle.payloads.is_empty());
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Thumbnail
                && i.reason == MetadataIssueReason::RemovedStale
        }));
    }

    #[test]
    fn unknown_resource_is_named_not_copied() {
        let block = irb(0x0422, b"", &[9, 9]);
        let limits = crate::limits::MetadataLimits::default();
        let mut collector = Collector::new(&limits);
        parse_irbs(&block, &mut collector);
        let bundle = collector.finish(crate::bundle::SourceFormat::Jpeg);
        assert!(bundle.payloads.is_empty());
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Unknown
                && i.reason == MetadataIssueReason::Unsupported
                && i.field.as_deref() == Some("0x0422")
        }));
    }

    #[test]
    fn malformed_iptc_record_is_rejected() {
        // Length claims more bytes than remain in the record.
        let bogus = vec![0x1C, 2, 5, 0xFF, 0xFF];
        let block = irb(RESOURCE_IPTC, b"", &bogus);
        let limits = crate::limits::MetadataLimits::default();
        let mut collector = Collector::new(&limits);
        parse_irbs(&block, &mut collector);
        let bundle = collector.finish(crate::bundle::SourceFormat::Jpeg);
        assert!(bundle.payloads.is_empty());
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Iptc && i.reason == MetadataIssueReason::Malformed
        }));
    }
}
