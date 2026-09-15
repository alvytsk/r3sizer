//! PNG container scanning.
//!
//! Walks the chunk stream (length + type + data + CRC), validating framing
//! and CRC without ever inflating IDAT. Recognizes eXIf, iTXt (including
//! the Adobe XMP keyword), tEXt, zTXt, iCCP, and pHYs. Color-declaring
//! chunks (sRGB/gAMA/cHRM/iCCP) are used only to set `source_color`, not
//! copied as payloads, since color handling is a resize-pipeline policy
//! decision, not metadata to carry through verbatim. A gAMA/cHRM chunk not
//! overridden by sRGB is reported as `Icc`/`Unverified`, since its loss can
//! change how the output renders.

use img_parts::png::{Png, PngChunk};
use img_parts::Bytes;

use crate::bundle::{MetadataBundle, Payload, SourceColor, SourceFormat};
use crate::containers::{inflate_bounded, Collector, InflateError};
use crate::limits::{checked_range, MetadataLimits};
use crate::types::{
    MetadataCategory, MetadataExport, MetadataIssueReason, MetadataReport, OutputFacts,
};

const XMP_KEYWORD: &[u8] = b"XML:com.adobe.xmp";

/// Chunk types that are recognized as standard PNG structure but carry
/// nothing this crate extracts (palette/pixel-dependent ancillary data,
/// APNG frames, etc). Listed explicitly so we don't mistake them for
/// unrecognized/unsupported chunks.
const KNOWN_IGNORED: &[&[u8; 4]] = &[
    b"PLTE", b"IDAT", b"sBIT", b"bKGD", b"hIST", b"tRNS", b"tIME", b"sPLT", b"oFFs", b"pCAL",
    b"sCAL", b"gIFg", b"gIFx", b"gIFt", b"acTL", b"fcTL", b"fdAT", b"dSIG",
];

pub(crate) fn extract(source: &[u8], limits: &MetadataLimits) -> MetadataBundle {
    let mut collector = Collector::new(limits);
    let mut pos = 8usize; // past the 8-byte PNG signature
    let mut first_chunk = true;
    let mut fatal = false;
    let mut iend_found = false;
    let mut srgb_seen = false;
    let mut gama_seen = false;
    let mut chrm_seen = false;

    loop {
        if pos >= source.len() {
            break;
        }
        if pos + 8 > source.len() {
            collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Malformed,
                Some("chunk_header_truncated".to_string()),
            );
            fatal = true;
            break;
        }
        let length = u32::from_be_bytes([
            source[pos],
            source[pos + 1],
            source[pos + 2],
            source[pos + 3],
        ]) as usize;
        let kind: [u8; 4] = [
            source[pos + 4],
            source[pos + 5],
            source[pos + 6],
            source[pos + 7],
        ];
        let Some(data_range) = checked_range(pos + 8, length, 1, source.len()) else {
            collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Malformed,
                Some("chunk_length".to_string()),
            );
            fatal = true;
            break;
        };
        let Some(crc_range) = checked_range(data_range.end, 4, 1, source.len()) else {
            collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Malformed,
                Some("chunk_crc_truncated".to_string()),
            );
            fatal = true;
            break;
        };

        if first_chunk && &kind != b"IHDR" {
            collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Malformed,
                Some("ihdr_not_first".to_string()),
            );
            fatal = true;
            break;
        }
        first_chunk = false;

        let declared_crc = u32::from_be_bytes([
            source[crc_range.start],
            source[crc_range.start + 1],
            source[crc_range.start + 2],
            source[crc_range.start + 3],
        ]);
        let computed_crc = crc32fast::hash(&source[pos + 4..data_range.end]);

        if declared_crc != computed_crc {
            collector.push_issue(
                category_for(&kind),
                MetadataIssueReason::Malformed,
                Some(kind_string(&kind)),
            );
        } else {
            let data = &source[data_range.clone()];
            match &kind {
                b"IHDR" => {}
                b"IEND" => iend_found = true,
                b"eXIf" => collector.offer_exif(data.to_vec()),
                b"iCCP" => handle_iccp(data, &mut collector),
                b"pHYs" => handle_phys(data, &mut collector),
                b"tEXt" => handle_text(data, &mut collector),
                b"zTXt" => handle_ztxt(data, &mut collector),
                b"iTXt" => handle_itxt(data, &mut collector),
                b"sRGB" => {
                    if data.len() == 1 {
                        srgb_seen = true;
                    } else {
                        collector.push_issue(
                            MetadataCategory::Unknown,
                            MetadataIssueReason::Malformed,
                            Some("srgb".to_string()),
                        );
                    }
                }
                b"gAMA" => gama_seen = true,
                b"cHRM" => chrm_seen = true,
                _ if KNOWN_IGNORED.contains(&&kind) => {}
                _ => collector.push_issue(
                    MetadataCategory::Unknown,
                    MetadataIssueReason::Unsupported,
                    Some(kind_string(&kind)),
                ),
            }
        }

        pos = crc_range.end;
        if iend_found {
            break;
        }
    }

    if !iend_found {
        if !fatal {
            collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Malformed,
                Some("missing_iend".to_string()),
            );
        }
    } else if pos < source.len() {
        collector.push_issue(
            MetadataCategory::Unknown,
            MetadataIssueReason::Unverified,
            Some("trailing".to_string()),
        );
    }

    // gAMA/cHRM are never carried into the output (the destination encoder
    // owns color chunks), so their loss is reported -- unless an sRGB chunk
    // is present, which overrides both per the PNG spec.
    if !srgb_seen {
        for (seen, field) in [(gama_seen, "gAMA"), (chrm_seen, "cHRM")] {
            if seen {
                collector.push_issue(
                    MetadataCategory::Icc,
                    MetadataIssueReason::Unverified,
                    Some(field.to_string()),
                );
            }
        }
    }

    let mut bundle = collector.finish(SourceFormat::Png);
    let icc_added = bundle.payloads.iter().any(|p| matches!(p, Payload::Icc(_)));
    bundle.source_color = if icc_added {
        SourceColor::Other
    } else if fatal {
        SourceColor::Unknown
    } else if srgb_seen {
        SourceColor::Srgb
    } else if gama_seen || chrm_seen {
        SourceColor::Other
    } else {
        SourceColor::Unspecified
    };
    bundle
}

fn category_for(kind: &[u8; 4]) -> MetadataCategory {
    match kind {
        b"eXIf" => MetadataCategory::Exif,
        b"iCCP" => MetadataCategory::Icc,
        b"pHYs" => MetadataCategory::Density,
        b"tEXt" | b"zTXt" | b"iTXt" => MetadataCategory::Text,
        _ => MetadataCategory::Unknown,
    }
}

fn kind_string(kind: &[u8; 4]) -> String {
    String::from_utf8_lossy(kind).into_owned()
}

fn handle_iccp(data: &[u8], collector: &mut Collector) {
    let Some(nul) = data.iter().position(|&b| b == 0) else {
        collector.push_issue(
            MetadataCategory::Icc,
            MetadataIssueReason::Malformed,
            Some("iccp_keyword".to_string()),
        );
        return;
    };
    if nul + 1 >= data.len() {
        collector.push_issue(
            MetadataCategory::Icc,
            MetadataIssueReason::Malformed,
            Some("iccp_truncated".to_string()),
        );
        return;
    }
    let compression_method = data[nul + 1];
    if compression_method != 0 {
        collector.push_issue(
            MetadataCategory::Icc,
            MetadataIssueReason::Unsupported,
            Some("iccp_compression_method".to_string()),
        );
        return;
    }
    let compressed = &data[nul + 2..];
    let budget = collector.remaining_budget();
    match inflate_bounded(compressed, budget) {
        Ok(profile) => collector.offer_icc(profile),
        Err(InflateError::LimitExceeded) => collector.push_issue(
            MetadataCategory::Icc,
            MetadataIssueReason::LimitExceeded,
            Some("icc".to_string()),
        ),
        Err(InflateError::Malformed) => collector.push_issue(
            MetadataCategory::Icc,
            MetadataIssueReason::Malformed,
            Some("icc_zlib".to_string()),
        ),
    }
}

fn handle_phys(data: &[u8], collector: &mut Collector) {
    if data.len() != 9 {
        collector.push_issue(
            MetadataCategory::Density,
            MetadataIssueReason::Malformed,
            Some("phys".to_string()),
        );
        return;
    }
    let x = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let y = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let unit = data[8];
    collector.add_payload(
        Payload::PngDensity { x, y, unit },
        MetadataCategory::Density,
        "phys",
    );
}

fn handle_text(data: &[u8], collector: &mut Collector) {
    if !data.contains(&0) {
        collector.push_issue(
            MetadataCategory::Text,
            MetadataIssueReason::Malformed,
            Some("text_keyword".to_string()),
        );
        return;
    }
    collector.add_payload(
        Payload::PngText {
            kind: *b"tEXt",
            data: data.to_vec(),
        },
        MetadataCategory::Text,
        "text",
    );
}

fn handle_ztxt(data: &[u8], collector: &mut Collector) {
    let Some(nul) = data.iter().position(|&b| b == 0) else {
        collector.push_issue(
            MetadataCategory::Text,
            MetadataIssueReason::Malformed,
            Some("ztxt_keyword".to_string()),
        );
        return;
    };
    if nul + 1 >= data.len() {
        collector.push_issue(
            MetadataCategory::Text,
            MetadataIssueReason::Malformed,
            Some("ztxt_truncated".to_string()),
        );
        return;
    }
    let compression_method = data[nul + 1];
    if compression_method != 0 {
        collector.push_issue(
            MetadataCategory::Text,
            MetadataIssueReason::Unsupported,
            Some("ztxt_compression_method".to_string()),
        );
        return;
    }
    let compressed = &data[nul + 2..];
    let budget = collector.remaining_budget();
    match inflate_bounded(compressed, budget) {
        // Decompression only validates the chunk; the ORIGINAL (still
        // compressed) bytes are what get stored, for byte-exact fidelity
        // and so re-embedding doesn't need a compressor. The *decompressed*
        // length is what gets charged against the budget, though — that's
        // the real work/memory this chunk cost, even though it's discarded
        // once validated. Charging only the tiny stored length here would
        // let many small, highly-compressible zTXt chunks each dodge the
        // budget check.
        Ok(decompressed) => {
            collector.charge_and_store(
                decompressed.len(),
                Payload::PngText {
                    kind: *b"zTXt",
                    data: data.to_vec(),
                },
                MetadataCategory::Text,
                "text",
            );
        }
        Err(InflateError::LimitExceeded) => collector.push_issue(
            MetadataCategory::Text,
            MetadataIssueReason::LimitExceeded,
            Some("text".to_string()),
        ),
        Err(InflateError::Malformed) => collector.push_issue(
            MetadataCategory::Text,
            MetadataIssueReason::Malformed,
            Some("ztxt_zlib".to_string()),
        ),
    }
}

fn handle_itxt(data: &[u8], collector: &mut Collector) {
    let Some(kw_end) = data.iter().position(|&b| b == 0) else {
        collector.push_issue(
            MetadataCategory::Text,
            MetadataIssueReason::Malformed,
            Some("itxt_keyword".to_string()),
        );
        return;
    };
    let keyword = &data[..kw_end];
    let mut p = kw_end + 1;
    if p + 2 > data.len() {
        collector.push_issue(
            MetadataCategory::Text,
            MetadataIssueReason::Malformed,
            Some("itxt_truncated".to_string()),
        );
        return;
    }
    let compression_flag = data[p];
    let compression_method = data[p + 1];
    p += 2;
    let Some(lang_end) = data[p..].iter().position(|&b| b == 0).map(|i| p + i) else {
        collector.push_issue(
            MetadataCategory::Text,
            MetadataIssueReason::Malformed,
            Some("itxt_language".to_string()),
        );
        return;
    };
    p = lang_end + 1;
    let Some(trans_end) = data[p..].iter().position(|&b| b == 0).map(|i| p + i) else {
        collector.push_issue(
            MetadataCategory::Text,
            MetadataIssueReason::Malformed,
            Some("itxt_translated".to_string()),
        );
        return;
    };
    p = trans_end + 1;
    let text_bytes = &data[p..];
    let is_xmp = keyword == XMP_KEYWORD;
    let category = if is_xmp {
        MetadataCategory::Xmp
    } else {
        MetadataCategory::Text
    };

    match compression_flag {
        0 => store_itxt_plain(is_xmp, data, text_bytes, collector),
        1 => {
            if compression_method != 0 {
                collector.push_issue(
                    category,
                    MetadataIssueReason::Unsupported,
                    Some("itxt_compression_method".to_string()),
                );
                return;
            }
            let budget = collector.remaining_budget();
            match inflate_bounded(text_bytes, budget) {
                Ok(decompressed) => {
                    if is_xmp {
                        // XMP must be usable text, not a still-compressed
                        // blob, so this is the one iTXt case that stores
                        // the decompressed bytes rather than the original.
                        // `offer_xmp` charges the budget itself.
                        collector.offer_xmp(decompressed);
                    } else {
                        // As with zTXt: charge the real decompressed size,
                        // not the smaller still-compressed bytes we keep.
                        collector.charge_and_store(
                            decompressed.len(),
                            Payload::PngText {
                                kind: *b"iTXt",
                                data: data.to_vec(),
                            },
                            MetadataCategory::Text,
                            "text",
                        );
                    }
                }
                Err(InflateError::LimitExceeded) => collector.push_issue(
                    category,
                    MetadataIssueReason::LimitExceeded,
                    Some("itxt".to_string()),
                ),
                Err(InflateError::Malformed) => collector.push_issue(
                    category,
                    MetadataIssueReason::Malformed,
                    Some("itxt_zlib".to_string()),
                ),
            }
        }
        _ => collector.push_issue(
            category,
            MetadataIssueReason::Malformed,
            Some("itxt_compression_flag".to_string()),
        ),
    }
}

fn store_itxt_plain(is_xmp: bool, data: &[u8], text_bytes: &[u8], collector: &mut Collector) {
    if is_xmp {
        collector.offer_xmp(text_bytes.to_vec());
    } else {
        collector.add_payload(
            Payload::PngText {
                kind: *b"iTXt",
                data: data.to_vec(),
            },
            MetadataCategory::Text,
            "text",
        );
    }
}

// --- Merge / embed ---------------------------------------------------------

const CHUNK_IDAT: [u8; 4] = *b"IDAT";
const CHUNK_EXIF: [u8; 4] = *b"eXIf";
const CHUNK_ICCP: [u8; 4] = *b"iCCP";
const CHUNK_PHYS: [u8; 4] = *b"pHYs";

/// Insert `prepared`'s payloads into an already-encoded destination PNG.
///
/// `img-parts`'s own `Png::set_exif` inserts right before `IEND` -- after
/// any `IDAT`, which the PNG spec forbids for `eXIf` (it must precede the
/// first `IDAT`) -- so EXIF and `pHYs` (also "before IDAT") are placed by
/// hand here rather than via that method. `Png::set_icc_profile` is *not*
/// reused either, despite inserting right after `IHDR` (always legal
/// ordering-wise): it hardcodes `self.chunks.insert(1, chunk)` with no
/// bounds check, and `Png::from_bytes` accepts a signature-only PNG (zero
/// chunks), so calling it on such a destination would panic. `set_icc`
/// below reimplements the same shape (keyword + zlib-compressed profile)
/// with a clamped insertion position instead.
pub(crate) fn embed(
    encoded: Vec<u8>,
    prepared: &MetadataBundle,
    facts: &OutputFacts,
    limits: &MetadataLimits,
) -> MetadataExport {
    let mut png = match Png::from_bytes(Bytes::from(encoded.clone())) {
        Ok(p) => p,
        Err(_) => return super::rollback(encoded, prepared),
    };

    let mut attempt_issues = Vec::new();
    let mut attempted: Vec<&Payload> = Vec::new();
    let mut stripped_text = false;

    for payload in &prepared.payloads {
        match payload {
            Payload::Exif(bytes) => {
                if bytes.len() > limits.max_payload_bytes {
                    attempt_issues.push(super::too_large(payload));
                } else {
                    set_exif_before_idat(&mut png, bytes);
                    attempted.push(payload);
                }
            }
            Payload::Xmp(bytes) => {
                set_xmp(&mut png, bytes);
                attempted.push(payload);
            }
            Payload::Icc(bytes) => {
                set_icc(&mut png, bytes);
                attempted.push(payload);
            }
            Payload::PngText { kind, data } => {
                if !stripped_text {
                    strip_non_xmp_text(&mut png);
                    stripped_text = true;
                }
                insert_before_iend(&mut png, PngChunk::new(*kind, Bytes::from(data.clone())));
                attempted.push(payload);
            }
            Payload::PngDensity { x, y, unit } => {
                set_phys(&mut png, *x, *y, *unit);
                attempted.push(payload);
            }
            // No PNG container concept for JPEG-only IPTC/comment/density.
            Payload::Iptc(_) | Payload::JpegComment(_) | Payload::JfifDensity { .. } => {
                attempt_issues.push(super::unsupported(payload));
            }
        }
    }

    let mut merged = Vec::with_capacity(png.len());
    if png.encoder().write_to(&mut merged).is_err() {
        return super::rollback(encoded, prepared);
    }

    if !validate(&merged, &attempted, facts, limits) {
        return super::rollback(encoded, prepared);
    }

    let mut issues = prepared.report().issues.clone();
    issues.extend(attempt_issues);
    MetadataExport {
        bytes: merged,
        report: MetadataReport { issues },
    }
}

fn first_idat_index(png: &Png) -> Option<usize> {
    png.chunks().iter().position(|c| c.kind() == CHUNK_IDAT)
}

/// Insertion point that satisfies "before IDAT": the first `IDAT` chunk's
/// index, or (a destination with no `IDAT` at all is already broken) right
/// before `IEND` as a last resort.
fn before_idat_pos(png: &Png) -> usize {
    first_idat_index(png).unwrap_or_else(|| png.chunks().len().saturating_sub(1))
}

fn insert_before_iend(png: &mut Png, chunk: PngChunk) {
    let pos = png.chunks().len().saturating_sub(1);
    png.chunks_mut().insert(pos, chunk);
}

fn set_exif_before_idat(png: &mut Png, exif: &[u8]) {
    png.remove_chunks_by_type(CHUNK_EXIF);
    let pos = before_idat_pos(png);
    png.chunks_mut()
        .insert(pos, PngChunk::new(CHUNK_EXIF, Bytes::from(exif.to_vec())));
}

/// Hand-rolled equivalent of `img_parts::png::Png::set_icc_profile`: same
/// `iCCP` chunk shape (a nul-terminated profile-name C string, a
/// compression-method byte, then the zlib-compressed profile) but clamped
/// to `chunks().len().min(1)` so a destination with fewer than one chunk
/// (a bare 8-byte-signature-only PNG, which `Png::from_bytes` accepts)
/// can't panic the way `Png::set_icc_profile`'s unconditional
/// `insert(1, ...)` would.
fn set_icc(png: &mut Png, profile: &[u8]) {
    use std::io::Write;

    png.remove_chunks_by_type(CHUNK_ICCP);
    let mut contents = b"icc\0".to_vec(); // profile name, as a C string
    contents.push(0); // compression method: zlib/deflate
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    // `Vec<u8>` writes never fail; any error here would mean profile data
    // was dropped silently, which this crate never does, so a write
    // failure is treated the same as the img-parts equivalent would be --
    // unreachable in practice, but omit the profile rather than embed a
    // truncated one if it ever somehow happened.
    if encoder.write_all(profile).is_err() {
        return;
    }
    let Ok(compressed) = encoder.finish() else {
        return;
    };
    contents.extend_from_slice(&compressed);
    let pos = png.chunks().len().min(1);
    png.chunks_mut()
        .insert(pos, PngChunk::new(CHUNK_ICCP, Bytes::from(contents)));
}

fn is_xmp_itxt(contents: &[u8]) -> bool {
    contents.starts_with(XMP_KEYWORD) && contents.get(XMP_KEYWORD.len()) == Some(&0)
}

/// No `ImageXMP`-style trait exists in `img-parts`, so this builds the iTXt
/// payload by hand: keyword, empty compression flag/method, empty
/// language/translated-keyword fields, then the raw XMP text -- matching
/// what `handle_itxt` in this file's extractor reads back as XMP.
fn set_xmp(png: &mut Png, xmp: &[u8]) {
    png.chunks_mut()
        .retain(|c| !(c.kind() == *b"iTXt" && is_xmp_itxt(c.contents())));
    let mut contents = XMP_KEYWORD.to_vec();
    contents.push(0); // end keyword
    contents.push(0); // compression flag
    contents.push(0); // compression method
    contents.push(0); // empty language tag
    contents.push(0); // empty translated keyword
    contents.extend_from_slice(xmp);
    insert_before_iend(png, PngChunk::new(*b"iTXt", Bytes::from(contents)));
}

/// Strip destination `tEXt`/`zTXt`/plain `iTXt` chunks (but never the XMP
/// `iTXt`, which `set_xmp` owns) before replacing the Text category with
/// the source's own text chunks.
fn strip_non_xmp_text(png: &mut Png) {
    png.chunks_mut().retain(|c| {
        !matches!(
            c.kind(),
            [b't', b'E', b'X', b't'] | [b'z', b'T', b'X', b't']
        ) && !(c.kind() == *b"iTXt" && !is_xmp_itxt(c.contents()))
    });
}

/// Set the destination's `pHYs` chunk: patched in place if one already
/// exists, or inserted fresh (before the first `IDAT`, per the "before
/// IDAT" ordering constraint) otherwise.
fn set_phys(png: &mut Png, x: u32, y: u32, unit: u8) {
    let mut contents = Vec::with_capacity(9);
    contents.extend_from_slice(&x.to_be_bytes());
    contents.extend_from_slice(&y.to_be_bytes());
    contents.push(unit);
    let chunk = PngChunk::new(CHUNK_PHYS, Bytes::from(contents));
    if let Some(idx) = png.chunks().iter().position(|c| c.kind() == CHUNK_PHYS) {
        png.chunks_mut()[idx] = chunk;
    } else {
        let pos = before_idat_pos(png);
        png.chunks_mut().insert(pos, chunk);
    }
}

/// Re-scan the just-written bytes with this module's own bounded extractor
/// and confirm every payload we attempted round-trips byte-identically,
/// with no new structural damage and dimensions matching `facts`. Never
/// inflates IDAT.
fn validate(
    merged: &[u8],
    attempted: &[&Payload],
    facts: &OutputFacts,
    limits: &MetadataLimits,
) -> bool {
    let bundle = extract(merged, limits);
    if bundle
        .report()
        .issues
        .iter()
        .any(|i| i.reason == MetadataIssueReason::Malformed)
    {
        return false;
    }
    for payload in attempted {
        let present = match payload {
            Payload::Exif(b) => bundle.payloads.iter().any(|p| matches!(p, Payload::Exif(pb) if pb == b)),
            Payload::Xmp(b) => bundle.payloads.iter().any(|p| matches!(p, Payload::Xmp(pb) if pb == b)),
            Payload::Icc(b) => bundle.payloads.iter().any(|p| matches!(p, Payload::Icc(pb) if pb == b)),
            Payload::PngText { kind, data } => bundle.payloads.iter().any(|p| {
                matches!(p, Payload::PngText { kind: pk, data: pd } if pk == kind && pd == data)
            }),
            Payload::PngDensity { x, y, unit } => bundle.payloads.iter().any(|p| {
                matches!(p, Payload::PngDensity { x: px, y: py, unit: pu }
                    if px == x && py == y && pu == unit)
            }),
            Payload::Iptc(_) | Payload::JpegComment(_) | Payload::JfifDensity { .. } => true,
        };
        if !present {
            return false;
        }
    }
    matches!(dimensions(merged), Some((w, h)) if w == facts.width && h == facts.height)
}

/// Read width/height directly from IHDR's first 8 bytes, without
/// interpreting any other chunk. Requires IHDR to actually be the first
/// chunk with its spec-mandated 13-byte body (already true of anything
/// `extract`/`validate` above accepted).
fn dimensions(source: &[u8]) -> Option<(u32, u32)> {
    if !source.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let range = checked_range(8, 25, 1, source.len())?; // len(4)+"IHDR"(4)+data(13)+crc(4)
    if &source[range.start + 4..range.start + 8] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(
        source[range.start + 8..range.start + 12]
            .try_into()
            .unwrap(),
    );
    let height = u32::from_be_bytes(
        source[range.start + 12..range.start + 16]
            .try_into()
            .unwrap(),
    );
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MetadataIssueReason;

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32fast::hash(&out[4..]);
        out.extend_from_slice(&crc.to_be_bytes());
        out
    }

    fn bad_crc_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = chunk(kind, data);
        let last = out.len() - 1;
        out[last] ^= 0xFF; // flip a bit in the CRC
        out
    }

    fn ihdr() -> Vec<u8> {
        chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0])
    }

    fn png(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        out.extend(ihdr());
        for c in chunks {
            out.extend_from_slice(c);
        }
        out.extend(chunk(b"IEND", &[]));
        out
    }

    fn compress(data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn text_is_detected_without_a_pixel_decode() {
        let data = png(&[chunk(b"tEXt", b"Author\0Fixture Author")]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.report().issues.is_empty());
        let texts: Vec<_> = bundle
            .payloads
            .iter()
            .filter_map(|p| match p {
                Payload::PngText {
                    kind: [b't', b'E', b'X', b't'],
                    data,
                } => Some(data.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec![b"Author\0Fixture Author".to_vec()]);
    }

    #[test]
    fn eof_exif_is_detected() {
        let mut tiff = b"MM\0*".to_vec();
        tiff.extend_from_slice(b"fake tiff body");
        let data = png(&[chunk(b"eXIf", &tiff)]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle
            .payloads
            .iter()
            .any(|p| matches!(p, Payload::Exif(b) if b == &tiff)));
    }

    #[test]
    fn iccp_is_decompressed_and_stored_raw_profile() {
        let profile = b"fake icc profile bytes".to_vec();
        let compressed = compress(&profile);
        let mut iccp_data = b"sRGB-ish\0\0".to_vec(); // keyword + \0 + compression method 0
        iccp_data.extend_from_slice(&compressed);
        let data = png(&[chunk(b"iCCP", &iccp_data)]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());
        assert!(bundle
            .payloads
            .iter()
            .any(|p| matches!(p, Payload::Icc(b) if b == &profile)));
        assert_eq!(bundle.source_color(), SourceColor::Other);
    }

    #[test]
    fn duplicate_iccp_drops_the_category() {
        let compressed = compress(b"profile one");
        let mut a = b"kw\0\0".to_vec();
        a.extend_from_slice(&compressed);
        let compressed2 = compress(b"profile two");
        let mut b = b"kw\0\0".to_vec();
        b.extend_from_slice(&compressed2);
        let data = png(&[chunk(b"iCCP", &a), chunk(b"iCCP", &b)]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle
            .payloads
            .iter()
            .all(|p| !matches!(p, Payload::Icc(_))));
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Malformed
        }));
    }

    #[test]
    fn compressed_text_exceeding_budget_is_limit_exceeded() {
        let big = vec![b'a'; 10_000];
        let compressed = compress(&big);
        let mut ztxt_data = b"kw\0\0".to_vec();
        ztxt_data.extend_from_slice(&compressed);
        let data = png(&[chunk(b"zTXt", &ztxt_data)]);
        let limits = MetadataLimits {
            max_payload_bytes: 100,
            max_total_metadata_bytes: 100,
            ..MetadataLimits::default()
        };
        let bundle = extract(&data, &limits);
        assert!(bundle.payloads.is_empty());
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Text && i.reason == MetadataIssueReason::LimitExceeded
        }));
    }

    /// Each individual zTXt chunk here decompresses to well under
    /// `max_payload_bytes`, so a per-chunk-only check would let every one
    /// of them through. Only charging the *decompressed* size against the
    /// total budget at the moment of decompression — not just what ends up
    /// stored (the small, still-compressed original bytes) — catches the
    /// cumulative cost: two 3000-byte decompressions can't both fit in a
    /// 5000-byte total budget.
    #[test]
    fn cumulative_decompression_across_many_small_ztxt_chunks_is_bounded() {
        let payload = vec![b'a'; 3000]; // highly compressible: tiny on the wire
        let compressed = compress(&payload);
        let mut ztxt_data = b"kw\0\0".to_vec();
        ztxt_data.extend_from_slice(&compressed);
        let one_chunk = chunk(b"zTXt", &ztxt_data);
        let data = png(&[one_chunk.clone(), one_chunk]);

        let limits = MetadataLimits {
            max_payload_bytes: 4_000,
            max_total_metadata_bytes: 5_000,
            ..MetadataLimits::default()
        };
        let bundle = extract(&data, &limits);

        let stored_ztxt = bundle
            .payloads
            .iter()
            .filter(|p| {
                matches!(
                    p,
                    Payload::PngText {
                        kind: [b'z', b'T', b'X', b't'],
                        ..
                    }
                )
            })
            .count();
        assert_eq!(
            stored_ztxt, 1,
            "the second chunk's decompression must be charged and rejected, not silently accepted"
        );
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Text && i.reason == MetadataIssueReason::LimitExceeded
        }));
    }

    #[test]
    fn itxt_xmp_is_detected_and_not_duplicated_as_text() {
        let mut data = XMP_KEYWORD.to_vec();
        data.push(0); // end keyword
        data.push(0); // compression flag = 0
        data.push(0); // compression method
        data.push(0); // empty language tag
        data.push(0); // empty translated keyword
        data.extend_from_slice(b"<x:xmpmeta>hi</x:xmpmeta>");
        let png_bytes = png(&[chunk(b"iTXt", &data)]);
        let bundle = extract(&png_bytes, &MetadataLimits::default());
        assert!(bundle
            .payloads
            .iter()
            .any(|p| matches!(p, Payload::Xmp(b) if b == b"<x:xmpmeta>hi</x:xmpmeta>")));
        assert!(bundle
            .payloads
            .iter()
            .all(|p| !matches!(p, Payload::PngText { .. })));
    }

    #[test]
    fn corrupted_crc_chunk_is_malformed_and_not_extracted() {
        let data = png(&[bad_crc_chunk(b"tEXt", b"Author\0Someone")]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.payloads.is_empty());
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Text && i.reason == MetadataIssueReason::Malformed
        }));
    }

    #[test]
    fn truncated_chunk_is_malformed() {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.extend(ihdr());
        // Declares 1000 bytes of tEXt data but the file ends immediately.
        data.extend_from_slice(&1000u32.to_be_bytes());
        data.extend_from_slice(b"tEXt");
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle
            .report()
            .issues
            .iter()
            .any(|i| { i.reason == MetadataIssueReason::Malformed }));
        assert_eq!(bundle.source_color(), SourceColor::Unknown);
    }

    #[test]
    fn unknown_chunk_is_flagged_not_copied() {
        let data = png(&[chunk(b"zzZz", b"mystery")]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.payloads.is_empty());
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Unknown
                && i.reason == MetadataIssueReason::Unsupported
                && i.field.as_deref() == Some("zzZz")
        }));
    }

    #[test]
    fn trailing_bytes_after_iend_are_unverified() {
        let mut data = png(&[]);
        data.extend_from_slice(b"garbage");
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.report().issues.iter().any(|i| {
            i.reason == MetadataIssueReason::Unverified && i.field.as_deref() == Some("trailing")
        }));
    }

    #[test]
    fn srgb_declaration_sets_source_color() {
        let data = png(&[chunk(b"sRGB", &[0])]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert_eq!(bundle.source_color(), SourceColor::Srgb);
    }

    fn color_chunk_issues(bundle: &MetadataBundle) -> Vec<String> {
        bundle
            .report()
            .issues
            .iter()
            .filter(|i| {
                i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Unverified
            })
            .filter_map(|i| i.field.clone())
            .collect()
    }

    #[test]
    fn gama_without_srgb_is_reported_once() {
        let gama = chunk(b"gAMA", &45455u32.to_be_bytes());
        let data = png(&[gama.clone(), gama]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert_eq!(color_chunk_issues(&bundle), vec!["gAMA".to_string()]);
        assert_eq!(bundle.source_color(), SourceColor::Other);
    }

    #[test]
    fn chrm_without_srgb_is_reported() {
        let data = png(&[chunk(b"cHRM", &[0u8; 32])]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert_eq!(color_chunk_issues(&bundle), vec!["cHRM".to_string()]);
    }

    #[test]
    fn srgb_overrides_gama_and_chrm_without_warning() {
        let data = png(&[
            chunk(b"sRGB", &[0]),
            chunk(b"gAMA", &45455u32.to_be_bytes()),
            chunk(b"cHRM", &[0u8; 32]),
        ]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());
        assert_eq!(bundle.source_color(), SourceColor::Srgb);
    }

    #[test]
    fn no_color_declaration_is_unspecified() {
        let data = png(&[]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert_eq!(bundle.source_color(), SourceColor::Unspecified);
    }
}
