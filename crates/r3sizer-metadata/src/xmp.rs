//! XMP correction: namespace-aware packet correction mirroring the EXIF
//! policy in `exif::patch` but operating on XML instead of TIFF.
//!
//! `correct()` never partially rewrites an XMP packet it can't fully make
//! sense of: malformed XML, DTDs, undefined entities, excessive nesting, or
//! oversized payloads make it omit the whole packet with a single issue,
//! matching `exif::correct`'s "whole block or nothing" contract for
//! anything that would make guessing unsafe.
//!
//! Only six well-known scalar properties are ever rewritten -- TIFF
//! `ImageWidth`/`ImageLength`/`Orientation` and EXIF
//! `PixelXDimension`/`PixelYDimension`/`ColorSpace` -- matched by
//! (namespace URI, local name), never by prefix, so an aliased prefix is
//! corrected exactly like the conventional one. A handful of other
//! well-known properties (the embedded thumbnail array, the ExtendedXMP
//! reference, and the two "last modified" dates) are removed outright
//! because a packet edit invalidates what they claim. Everything else --
//! `rdf:Description`, language alternatives, arrays, rights, GPS, custom
//! namespaces, and descriptions that happen to contain the words
//! "ImageWidth" or "Orientation" as literal text -- passes through
//! byte-for-byte untouched, because nothing here ever regex-matches or
//! rewrites raw text: every rewrite is keyed off a namespace-resolved
//! element or attribute *name*, never off content.
//!
//! Two internal passes: [`parse`] reads the packet once with a live
//! `NsReader` (the only thing that can resolve a prefix to a namespace),
//! validating well-formedness/entities/depth along the way and recording,
//! for every element and attribute, whether it matches one of the tags
//! above. `decide` then aggregates every occurrence of each managed tag
//! (there can be more than one, e.g. one in `tiff:` and one in `exif:`, or
//! a genuine duplicate) into a single per-tag decision, so two occurrences
//! that disagree are treated as an untrustworthy duplicate rather than
//! silently picking one. `rewrite` replays the events, applying decisions
//! by depth-tracked skipping (drop) or splicing in new text (correct).

use std::collections::HashMap;

use quick_xml::events::{BytesRef, BytesStart, BytesText, Event};
use quick_xml::name::{QName, ResolveResult};
use quick_xml::reader::NsReader;
use quick_xml::writer::Writer;

use crate::limits::MetadataLimits;
use crate::types::{
    ColorAction, MetadataCategory, MetadataIssue, MetadataIssueReason, OrientationAction,
    OutputFacts,
};

const TIFF_NS: &[u8] = b"http://ns.adobe.com/tiff/1.0/";
const EXIF_NS: &[u8] = b"http://ns.adobe.com/exif/1.0/";
const XMP_NS: &[u8] = b"http://ns.adobe.com/xap/1.0/";
const XMP_NOTE_NS: &[u8] = b"http://ns.adobe.com/xmp/note/";

/// (namespace, local name) -- the identity of one managed/removed property,
/// independent of whatever prefix alias the packet used for it.
type TagKey = (Vec<u8>, Vec<u8>);

#[derive(Clone, Copy, PartialEq, Eq)]
enum ManagedTag {
    Width,
    Height,
    Orientation,
    ColorSpace,
}

#[derive(Clone)]
enum TagKind {
    /// Rewritten based on `OutputFacts`; the final value is decided once
    /// per `TagKey` in `decide`, after every occurrence is known.
    Managed(ManagedTag),
    /// Removed unconditionally, wherever found, with this fixed issue.
    Remove(MetadataIssue),
}

/// Classify a resolved (namespace, local name) pair, or `None` if it's not
/// one this module ever touches (which is nearly everything -- arrays,
/// rights, GPS, `dc:*`, custom namespaces, and so on all fall through here
/// and are copied verbatim by `rewrite`).
///
/// ## `photoshop:LegacyIPTCDigest` is deliberately absent from this list
///
/// The brief calls for omitting stale digest/signature properties after a
/// packet edit, and `photoshop:LegacyIPTCDigest`
/// (`http://ns.adobe.com/photoshop/1.0/`) is the well-known real-world case
/// that fits that description: an MD5 digest camera- and
/// Photoshop-produced XMP carries over the *legacy IPTC IIM block*, used by
/// editors to detect whether that IIM block has since been changed by a
/// tool that doesn't understand XMP.
///
/// It is not classified here because reasoning through what it actually
/// hashes shows it doesn't go stale from anything this crate does: it
/// digests the raw IPTC IIM bytes, not any XMP property, and not the
/// image's pixels or dimensions. `xmp::correct` never touches IPTC content
/// at all, and `policy::prepare` (see `policy.rs`) passes `Payload::Iptc`
/// through byte-for-byte unchanged -- the IIM block this digest describes
/// is bit-identical in the output to what it was in the source. Dropping
/// the digest anyway would only discard metadata a downstream editor could
/// still use correctly, for no reason tied to what this task changes.
///
/// This holds only as long as nothing in this crate rewrites IPTC IIM
/// bytes. If a future task starts editing `Payload::Iptc`, this reasoning
/// no longer holds and `photoshop:LegacyIPTCDigest` should be added to the
/// remove list below at that point (see
/// `legacy_iptc_digest_is_preserved_since_iptc_bytes_are_never_edited` for
/// the regression test guarding this).
fn classify(ns: &[u8], local: &[u8]) -> Option<TagKind> {
    match (ns, local) {
        (TIFF_NS, b"ImageWidth") => Some(TagKind::Managed(ManagedTag::Width)),
        (TIFF_NS, b"ImageLength") => Some(TagKind::Managed(ManagedTag::Height)),
        (TIFF_NS, b"Orientation") => Some(TagKind::Managed(ManagedTag::Orientation)),
        (EXIF_NS, b"PixelXDimension") => Some(TagKind::Managed(ManagedTag::Width)),
        (EXIF_NS, b"PixelYDimension") => Some(TagKind::Managed(ManagedTag::Height)),
        (EXIF_NS, b"ColorSpace") => Some(TagKind::Managed(ManagedTag::ColorSpace)),
        (XMP_NS, b"Thumbnails") => Some(TagKind::Remove(issue(
            MetadataCategory::Thumbnail,
            MetadataIssueReason::RemovedStale,
            "thumbnails",
        ))),
        (XMP_NOTE_NS, b"HasExtendedXMP") => Some(TagKind::Remove(issue(
            MetadataCategory::Xmp,
            MetadataIssueReason::Unsupported,
            "extended_xmp",
        ))),
        (XMP_NS, b"ModifyDate") => Some(TagKind::Remove(issue(
            MetadataCategory::Xmp,
            MetadataIssueReason::RemovedStale,
            "modify_date",
        ))),
        (XMP_NS, b"MetadataDate") => Some(TagKind::Remove(issue(
            MetadataCategory::Xmp,
            MetadataIssueReason::RemovedStale,
            "metadata_date",
        ))),
        _ => None,
    }
}

fn issue(category: MetadataCategory, reason: MetadataIssueReason, field: &str) -> MetadataIssue {
    MetadataIssue {
        category,
        reason,
        field: Some(field.to_string()),
    }
}

fn fatal(reason: MetadataIssueReason, field: &str) -> MetadataIssue {
    issue(MetadataCategory::Xmp, reason, field)
}

/// One classified occurrence: which tag it is, and (for `Managed` only) the
/// decoded value, or `None` if the occurrence's shape wasn't a plain scalar
/// (nested markup, multiple text nodes, ...) and so can't be corrected in
/// place at all.
struct Hit {
    key: TagKey,
    kind: TagKind,
    value: Option<String>,
}

#[derive(Default)]
struct EventMeta {
    /// Set when the element's own resolved name is classified.
    own: Option<Hit>,
    /// Set for each classified attribute, paired with its position in the
    /// element's `attributes()` iteration order (stable across passes,
    /// since the underlying `BytesStart` bytes never change).
    attrs: Vec<(usize, Hit)>,
}

struct Parsed {
    events: Vec<Event<'static>>,
    metas: Vec<EventMeta>,
}

/// Correct dimension, orientation, and color-space XMP properties in `raw`
/// (a complete XMP packet, UTF-8 XML) to match `facts`, within `limits`.
///
/// Returns `None` when the packet can't be safely corrected at all (not
/// UTF-8, malformed XML, a DTD, an undefined entity, excessive nesting, or
/// oversized input) -- the caller should omit the XMP block entirely rather
/// than embed unverified bytes.
pub(crate) fn correct(
    raw: &[u8],
    facts: &OutputFacts,
    limits: &MetadataLimits,
) -> (Option<Vec<u8>>, Vec<MetadataIssue>) {
    if raw.len() > limits.max_payload_bytes {
        return (None, vec![fatal(MetadataIssueReason::LimitExceeded, "max_payload_bytes")]);
    }
    let text = match std::str::from_utf8(raw) {
        Ok(t) => t,
        Err(_) => return (None, vec![fatal(MetadataIssueReason::Malformed, "utf8")]),
    };
    let parsed = match parse(text, limits) {
        Ok(p) => p,
        Err(iss) => return (None, vec![iss]),
    };
    let (decisions, mut issues) = decide(&parsed.metas, facts);
    match rewrite(&parsed.events, &parsed.metas, &decisions, &mut issues) {
        Ok(bytes) => (Some(bytes), issues),
        Err(_) => (None, vec![fatal(MetadataIssueReason::Malformed, "write")]),
    }
}

/// Resolve a name (element or attribute) against the reader's current
/// namespace scope into an owned (namespace, local name) pair. Unbound or
/// unknown-prefix names resolve to an empty namespace, which never matches
/// any entry in `classify` (all four namespace constants are non-empty).
fn resolve_owned(reader: &NsReader<&[u8]>, name: QName, is_attr: bool) -> TagKey {
    let (resolved, local) = reader.resolve(name, is_attr);
    let ns = match resolved {
        ResolveResult::Bound(ns) => ns.0.to_vec(),
        ResolveResult::Unbound | ResolveResult::Unknown(_) => Vec::new(),
    };
    (ns, local.as_ref().to_vec())
}

/// A numeric/char reference is only ever well-formed without a DTD if it's
/// one of the five predefined XML entities or a `#`-prefixed character
/// reference; anything else is a reference to an entity this packet never
/// declares (DTDs are rejected outright), so it's undefined.
fn validate_general_ref(r: &BytesRef) -> Result<(), MetadataIssue> {
    let content = r
        .decode()
        .map_err(|_| fatal(MetadataIssueReason::Malformed, "entity"))?;
    if content.starts_with('#') {
        return Ok(());
    }
    match content.as_ref() {
        "amp" | "lt" | "gt" | "apos" | "quot" => Ok(()),
        _ => Err(fatal(MetadataIssueReason::Malformed, "entity")),
    }
}

/// Classify a Start/Empty element's own name. Returns the `Hit` to store
/// immediately (for `Remove`, and for `Managed` on an `Empty` element,
/// whose value is trivially the empty string), or, for `Managed` on a
/// `Start` element, the `(key, tag)` to seed the pending state machine that
/// `parse` resolves once the matching text/End arrives.
fn own_classification(
    reader: &NsReader<&[u8]>,
    start: &BytesStart,
    is_empty: bool,
) -> (Option<Hit>, Option<(TagKey, ManagedTag)>) {
    let key = resolve_owned(reader, start.name(), false);
    let Some(kind) = classify(&key.0, &key.1) else {
        return (None, None);
    };
    match kind {
        TagKind::Remove(_) => (
            Some(Hit {
                key,
                kind,
                value: None,
            }),
            None,
        ),
        TagKind::Managed(tag) => {
            if is_empty {
                (
                    Some(Hit {
                        key,
                        kind: TagKind::Managed(tag),
                        value: Some(String::new()),
                    }),
                    None,
                )
            } else {
                (None, Some((key, tag)))
            }
        }
    }
}

/// Validate and classify every attribute of a Start/Empty element. Entity
/// validity is checked on *every* attribute value regardless of whether it
/// ends up classified: an undefined entity anywhere is malformed XMP.
fn collect_attr_hits(
    reader: &NsReader<&[u8]>,
    start: &BytesStart,
) -> Result<Vec<(usize, Hit)>, MetadataIssue> {
    let mut hits = Vec::new();
    for (i, attr_result) in start.attributes().enumerate() {
        let attr = attr_result.map_err(|_| fatal(MetadataIssueReason::Malformed, "attribute"))?;
        let decoded = attr
            .unescape_value()
            .map_err(|_| fatal(MetadataIssueReason::Malformed, "entity"))?;
        let key = resolve_owned(reader, attr.key, true);
        if let Some(kind) = classify(&key.0, &key.1) {
            hits.push((
                i,
                Hit {
                    key,
                    kind,
                    value: Some(decoded.into_owned()),
                },
            ));
        }
    }
    Ok(hits)
}

/// Pending element-form managed occurrence, waiting to see whether its
/// content is empty, a single scalar text node, or something more complex
/// (nested markup, multiple text nodes) that can't be safely corrected.
enum Pending {
    None,
    AfterStart {
        start_idx: usize,
        key: TagKey,
        tag: ManagedTag,
    },
    AfterText {
        start_idx: usize,
        key: TagKey,
        tag: ManagedTag,
        value: String,
    },
}

fn finalize_managed(
    metas: &mut [EventMeta],
    start_idx: usize,
    key: TagKey,
    tag: ManagedTag,
    value: Option<String>,
) {
    metas[start_idx].own = Some(Hit {
        key,
        kind: TagKind::Managed(tag),
        value,
    });
}

/// Single pass over the packet with a live `NsReader`: validates
/// well-formedness/entities/depth, and records per-event classification
/// metadata (`EventMeta`) alongside the owned event stream.
fn parse(text: &str, limits: &MetadataLimits) -> Result<Parsed, MetadataIssue> {
    let mut reader = NsReader::from_str(text);

    let mut events: Vec<Event<'static>> = Vec::new();
    let mut metas: Vec<EventMeta> = Vec::new();
    let mut depth: usize = 0;
    let mut pending = Pending::None;

    loop {
        let ev = reader
            .read_event()
            .map_err(|_| fatal(MetadataIssueReason::Malformed, "xml"))?;

        pending = match pending {
            Pending::AfterStart {
                start_idx,
                key,
                tag,
            } => match &ev {
                Event::End(_) => {
                    finalize_managed(&mut metas, start_idx, key, tag, Some(String::new()));
                    Pending::None
                }
                Event::Text(t) => {
                    let value = t
                        .decode()
                        .map_err(|_| fatal(MetadataIssueReason::Malformed, "text"))?
                        .into_owned();
                    Pending::AfterText {
                        start_idx,
                        key,
                        tag,
                        value,
                    }
                }
                _ => {
                    finalize_managed(&mut metas, start_idx, key, tag, None);
                    Pending::None
                }
            },
            Pending::AfterText {
                start_idx,
                key,
                tag,
                value,
            } => match &ev {
                Event::End(_) => {
                    finalize_managed(&mut metas, start_idx, key, tag, Some(value));
                    Pending::None
                }
                _ => {
                    finalize_managed(&mut metas, start_idx, key, tag, None);
                    Pending::None
                }
            },
            Pending::None => Pending::None,
        };

        if let Event::DocType(_) = &ev {
            return Err(fatal(MetadataIssueReason::Unsupported, "doctype"));
        }
        if let Event::GeneralRef(r) = &ev {
            validate_general_ref(r)?;
        }

        let idx = events.len();
        let mut meta = EventMeta::default();
        let mut is_eof = false;

        match &ev {
            Event::Start(start) => {
                depth += 1;
                if depth > limits.max_xml_depth {
                    return Err(fatal(MetadataIssueReason::LimitExceeded, "max_xml_depth"));
                }
                let (own_hit, seed) = own_classification(&reader, start, false);
                meta.own = own_hit;
                if let Some((key, tag)) = seed {
                    pending = Pending::AfterStart {
                        start_idx: idx,
                        key,
                        tag,
                    };
                }
                meta.attrs = collect_attr_hits(&reader, start)?;
            }
            Event::Empty(start) => {
                if depth + 1 > limits.max_xml_depth {
                    return Err(fatal(MetadataIssueReason::LimitExceeded, "max_xml_depth"));
                }
                let (own_hit, _seed) = own_classification(&reader, start, true);
                meta.own = own_hit;
                meta.attrs = collect_attr_hits(&reader, start)?;
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| fatal(MetadataIssueReason::Malformed, "end"))?;
            }
            Event::Eof => is_eof = true,
            _ => {}
        }

        events.push(ev.into_owned());
        metas.push(meta);
        if is_eof {
            break;
        }
    }

    Ok(Parsed { events, metas })
}

/// Aggregate every occurrence of every managed tag across the whole packet
/// into one decision per `TagKey`: the new text to write everywhere that
/// tag occurs, or `None` to remove every occurrence (an invalid shape, a
/// value the target action can't verify, or occurrences that disagree).
fn decide(
    metas: &[EventMeta],
    facts: &OutputFacts,
) -> (HashMap<TagKey, Option<String>>, Vec<MetadataIssue>) {
    let mut occurrences: HashMap<TagKey, (ManagedTag, Vec<Option<String>>)> = HashMap::new();
    let mut note = |key: &TagKey, tag: ManagedTag, value: &Option<String>| {
        occurrences
            .entry(key.clone())
            .or_insert_with(|| (tag, Vec::new()))
            .1
            .push(value.clone());
    };
    for meta in metas {
        if let Some(hit) = &meta.own {
            if let TagKind::Managed(tag) = hit.kind {
                note(&hit.key, tag, &hit.value);
            }
        }
        for (_, hit) in &meta.attrs {
            if let TagKind::Managed(tag) = hit.kind {
                note(&hit.key, tag, &hit.value);
            }
        }
    }

    let mut decisions = HashMap::new();
    let mut issues = Vec::new();
    for (key, (tag, values)) in occurrences {
        let field = String::from_utf8_lossy(&key.1).into_owned();
        if values.iter().any(Option::is_none) {
            issues.push(issue(
                MetadataCategory::Xmp,
                MetadataIssueReason::Unsupported,
                &field,
            ));
            decisions.insert(key, None);
            continue;
        }
        let values: Vec<String> = values.into_iter().map(|v| v.unwrap()).collect();
        let first = values[0].clone();
        if !values.iter().all(|v| *v == first) {
            issues.push(issue(
                MetadataCategory::Xmp,
                MetadataIssueReason::Malformed,
                &field,
            ));
            decisions.insert(key, None);
            continue;
        }

        let decision = match tag {
            ManagedTag::Width => Some(facts.width.to_string()),
            ManagedTag::Height => Some(facts.height.to_string()),
            ManagedTag::Orientation => match first.trim().parse::<u32>() {
                Ok(v) if (1..=8).contains(&v) => Some(match facts.orientation {
                    OrientationAction::Preserve => v.to_string(),
                    OrientationAction::Normalize => "1".to_string(),
                }),
                _ => {
                    issues.push(issue(
                        MetadataCategory::Xmp,
                        MetadataIssueReason::Malformed,
                        &field,
                    ));
                    None
                }
            },
            ManagedTag::ColorSpace => match facts.color {
                ColorAction::Unchanged => Some(first),
                ColorAction::Srgb => Some("1".to_string()),
                ColorAction::Unverified => {
                    issues.push(issue(
                        MetadataCategory::Xmp,
                        MetadataIssueReason::Unverified,
                        &field,
                    ));
                    None
                }
            },
        };
        decisions.insert(key, decision);
    }
    (decisions, issues)
}

enum OpenAction {
    Skip,
    InjectValue(BytesStart<'static>, String),
    Normal(BytesStart<'static>),
}

fn open_action(
    start: &BytesStart<'static>,
    meta: &EventMeta,
    decisions: &HashMap<TagKey, Option<String>>,
    issues: &mut Vec<MetadataIssue>,
) -> OpenAction {
    if let Some(hit) = &meta.own {
        return match &hit.kind {
            TagKind::Remove(iss) => {
                issues.push(iss.clone());
                OpenAction::Skip
            }
            TagKind::Managed(_) => match decisions.get(&hit.key) {
                Some(Some(value)) => OpenAction::InjectValue(start.to_owned(), value.clone()),
                _ => OpenAction::Skip,
            },
        };
    }

    if meta.attrs.is_empty() {
        return OpenAction::Normal(start.to_owned());
    }

    let mut new_start = start.to_owned();
    new_start.clear_attributes();
    for (i, attr_result) in start.attributes().enumerate() {
        let Ok(attr) = attr_result else { continue };
        if let Some((_, hit)) = meta.attrs.iter().find(|(idx, _)| *idx == i) {
            match &hit.kind {
                TagKind::Remove(iss) => issues.push(iss.clone()),
                TagKind::Managed(_) => {
                    if let Some(Some(value)) = decisions.get(&hit.key) {
                        new_start.push_attribute((attr.key.as_ref(), value.as_bytes()));
                    }
                }
            }
        } else {
            new_start.push_attribute(attr);
        }
    }
    OpenAction::Normal(new_start)
}

/// Replay the collected events, applying `decisions` and unconditional
/// removals by depth-tracked skipping (dropping a Start/Empty element also
/// drops everything nested inside it, regardless of what that is) and by
/// splicing in freshly written text for corrected scalars.
fn rewrite(
    events: &[Event<'static>],
    metas: &[EventMeta],
    decisions: &HashMap<TagKey, Option<String>>,
    issues: &mut Vec<MetadataIssue>,
) -> std::io::Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    let mut nest: usize = 0;
    let mut skip_from: Option<usize> = None;
    let mut suppress_text = false;

    for (ev, meta) in events.iter().zip(metas.iter()) {
        match ev {
            Event::Start(start) => {
                if skip_from.is_some() {
                    nest += 1;
                    continue;
                }
                match open_action(start, meta, decisions, issues) {
                    OpenAction::Skip => {
                        skip_from = Some(nest);
                        nest += 1;
                    }
                    OpenAction::InjectValue(new_start, value) => {
                        writer.write_event(Event::Start(new_start))?;
                        writer.write_event(Event::Text(BytesText::new(&value)))?;
                        suppress_text = true;
                        nest += 1;
                    }
                    OpenAction::Normal(new_start) => {
                        writer.write_event(Event::Start(new_start))?;
                        nest += 1;
                    }
                }
            }
            Event::Empty(start) => {
                if skip_from.is_some() {
                    continue;
                }
                match open_action(start, meta, decisions, issues) {
                    OpenAction::Skip => {}
                    OpenAction::InjectValue(new_start, value) => {
                        writer.write_event(Event::Start(new_start.borrow()))?;
                        writer.write_event(Event::Text(BytesText::new(&value)))?;
                        writer.write_event(Event::End(new_start.to_end()))?;
                    }
                    OpenAction::Normal(new_start) => {
                        writer.write_event(Event::Empty(new_start))?;
                    }
                }
            }
            Event::End(end) => {
                if let Some(from) = skip_from {
                    nest -= 1;
                    if nest == from {
                        skip_from = None;
                    }
                    continue;
                }
                nest -= 1;
                suppress_text = false;
                writer.write_event(Event::End(end.borrow()))?;
            }
            Event::Text(_) if skip_from.is_some() => {}
            Event::Text(_) if suppress_text => {
                suppress_text = false;
            }
            Event::Text(t) => {
                writer.write_event(Event::Text(t.borrow()))?;
            }
            Event::Eof => break,
            other => {
                if skip_from.is_some() {
                    continue;
                }
                writer.write_event(other.borrow())?;
            }
        }
    }

    Ok(writer.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MetadataIssueReason as Reason;

    fn facts(
        width: u32,
        height: u32,
        orientation: OrientationAction,
        color: ColorAction,
    ) -> OutputFacts {
        OutputFacts {
            width,
            height,
            orientation,
            color,
        }
    }

    // Verbatim regression test from the task brief: an aliased namespace
    // prefix (`t:` for TIFF, not the conventional `tiff:`) must still be
    // corrected, matched by URI rather than by the literal prefix text, and
    // an unrelated `dc:description` containing the literal words that
    // could be mistaken for tag names must survive untouched.
    #[test]
    fn updates_xmp_using_namespace_uri_not_prefix() {
        let raw = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
          <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
           <rdf:Description xmlns:t="http://ns.adobe.com/tiff/1.0/"
            xmlns:dc="http://purl.org/dc/elements/1.1/" t:ImageWidth="400"
            t:Orientation="6"><t:ImageLength>200</t:ImageLength>
            <dc:description>Original description</dc:description></rdf:Description>
          </rdf:RDF></x:xmpmeta>"#;
        let facts = OutputFacts {
            width: 100,
            height: 50,
            orientation: OrientationAction::Normalize,
            color: ColorAction::Srgb,
        };
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(text.contains("t:ImageWidth=\"100\""));
        assert!(text.contains("<t:ImageLength>50</t:ImageLength>"));
        assert!(text.contains("t:Orientation=\"1\""));
        assert!(text.contains("Original description"));
    }

    #[test]
    fn description_text_containing_tag_names_is_never_touched() {
        // A user-authored description that literally contains the words
        // "ImageWidth" and "Orientation" must survive byte-for-byte: only
        // namespace-resolved element/attribute *names* are ever matched,
        // never text content.
        let raw = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:tiff="http://ns.adobe.com/tiff/1.0/">
          <rdf:Description tiff:ImageWidth="400" tiff:Orientation="6">
            <dc:description>Set the ImageWidth and Orientation tags before export</dc:description>
          </rdf:Description>
        </rdf:RDF>"#;
        let facts = facts(100, 50, OrientationAction::Normalize, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(text.contains(
            "<dc:description>Set the ImageWidth and Orientation tags before export</dc:description>"
        ));
        assert!(text.contains("tiff:ImageWidth=\"100\""));
        assert!(text.contains("tiff:Orientation=\"1\""));
    }

    #[test]
    fn conventional_prefix_and_element_form_are_also_corrected() {
        let raw = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:tiff="http://ns.adobe.com/tiff/1.0/" xmlns:exif="http://ns.adobe.com/exif/1.0/">
          <rdf:Description>
            <tiff:ImageWidth>4000</tiff:ImageWidth>
            <tiff:ImageLength>3000</tiff:ImageLength>
            <exif:PixelXDimension>4000</exif:PixelXDimension>
            <exif:PixelYDimension>3000</exif:PixelYDimension>
          </rdf:Description>
        </rdf:RDF>"#;
        let facts = facts(800, 600, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(text.contains("<tiff:ImageWidth>800</tiff:ImageWidth>"));
        assert!(text.contains("<tiff:ImageLength>600</tiff:ImageLength>"));
        assert!(text.contains("<exif:PixelXDimension>800</exif:PixelXDimension>"));
        assert!(text.contains("<exif:PixelYDimension>600</exif:PixelYDimension>"));
    }

    #[test]
    fn language_alternatives_arrays_rights_and_gps_survive_untouched() {
        let raw = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:exif="http://ns.adobe.com/exif/1.0/">
          <rdf:Description>
            <dc:rights>
              <rdf:Alt>
                <rdf:li xml:lang="en">All rights reserved</rdf:li>
                <rdf:li xml:lang="fr">Tous droits reserves</rdf:li>
              </rdf:Alt>
            </dc:rights>
            <dc:subject>
              <rdf:Bag>
                <rdf:li>vacation</rdf:li>
                <rdf:li>beach</rdf:li>
              </rdf:Bag>
            </dc:subject>
            <exif:GPSLatitude>40,26.767N</exif:GPSLatitude>
            <custom:widget xmlns:custom="urn:example:custom">kept</custom:widget>
          </rdf:Description>
        </rdf:RDF>"#;
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(text.contains("All rights reserved"));
        assert!(text.contains("Tous droits reserves"));
        assert!(text.contains("xml:lang=\"fr\""));
        assert!(text.contains("<rdf:li>vacation</rdf:li>"));
        assert!(text.contains("<rdf:li>beach</rdf:li>"));
        assert!(text.contains("40,26.767N"));
        assert!(text.contains("<custom:widget xmlns:custom=\"urn:example:custom\">kept</custom:widget>"));
    }

    #[test]
    fn empty_and_self_closing_dimension_fields_get_a_value_injected() {
        let raw = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:tiff="http://ns.adobe.com/tiff/1.0/">
          <rdf:Description><tiff:ImageWidth/><tiff:ImageLength></tiff:ImageLength></rdf:Description>
        </rdf:RDF>"#;
        let facts = facts(320, 240, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(text.contains("<tiff:ImageWidth>320</tiff:ImageWidth>"));
        assert!(text.contains("<tiff:ImageLength>240</tiff:ImageLength>"));
    }

    #[test]
    fn nested_qualified_value_is_removed_as_unsupported() {
        // `rdf:parseType="Resource"`-style qualified value: the scalar we'd
        // need to rewrite is buried under a nested element, not a plain
        // text node -- an unsupported representation, so the whole
        // property is dropped rather than guessed at.
        let raw = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:tiff="http://ns.adobe.com/tiff/1.0/">
          <rdf:Description><tiff:ImageWidth><rdf:Description><rdf:value>400</rdf:value></rdf:Description></tiff:ImageWidth></rdf:Description>
        </rdf:RDF>"#;
        let facts = facts(100, 50, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(!text.contains("ImageWidth"));
        assert!(issues.iter().any(|i| {
            i.category == MetadataCategory::Xmp
                && i.reason == Reason::Unsupported
                && i.field.as_deref() == Some("ImageWidth")
        }));
    }

    #[test]
    fn conflicting_duplicate_orientation_is_dropped_as_malformed() {
        let raw = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Orientation="3">
          <rdf:Description><tiff:Orientation>6</tiff:Orientation></rdf:Description>
        </rdf:RDF>"#;
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(!text.contains("Orientation"));
        assert!(issues.iter().any(|i| {
            i.category == MetadataCategory::Xmp
                && i.reason == Reason::Malformed
                && i.field.as_deref() == Some("Orientation")
        }));
    }

    #[test]
    fn out_of_range_orientation_is_dropped_as_malformed() {
        let raw = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:tiff="http://ns.adobe.com/tiff/1.0/">
          <rdf:Description tiff:Orientation="9"/>
        </rdf:RDF>"#;
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(!text.contains("Orientation"));
        assert!(issues
            .iter()
            .any(|i| i.reason == Reason::Malformed && i.field.as_deref() == Some("Orientation")));
    }

    #[test]
    fn color_space_srgb_sets_known_value() {
        let raw = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:exif="http://ns.adobe.com/exif/1.0/" exif:ColorSpace="65535">
          <rdf:Description/>
        </rdf:RDF>"#;
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Srgb);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(text.contains("exif:ColorSpace=\"1\""));
    }

    #[test]
    fn color_space_unverified_removes_declaration_with_issue() {
        let raw = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:exif="http://ns.adobe.com/exif/1.0/" exif:ColorSpace="1">
          <rdf:Description/>
        </rdf:RDF>"#;
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unverified);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(!text.contains("ColorSpace"));
        assert!(issues.iter().any(|i| {
            i.category == MetadataCategory::Xmp
                && i.reason == Reason::Unverified
                && i.field.as_deref() == Some("ColorSpace")
        }));
    }

    #[test]
    fn thumbnails_array_is_removed_with_issue() {
        let raw = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:xmp="http://ns.adobe.com/xap/1.0/">
          <rdf:Description>
            <xmp:Thumbnails>
              <rdf:Alt>
                <rdf:li rdf:parseType="Resource"><xmpGImg:image xmlns:xmpGImg="urn:x">base64data</xmpGImg:image></rdf:li>
              </rdf:Alt>
            </xmp:Thumbnails>
            <xmp:CreatorTool>Kept</xmp:CreatorTool>
          </rdf:Description>
        </rdf:RDF>"#;
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(!text.contains("Thumbnails"));
        assert!(!text.contains("base64data"));
        assert!(text.contains("<xmp:CreatorTool>Kept</xmp:CreatorTool>"));
        assert!(issues.iter().any(|i| {
            i.category == MetadataCategory::Thumbnail
                && i.reason == Reason::RemovedStale
                && i.field.as_deref() == Some("thumbnails")
        }));
    }

    #[test]
    fn extended_xmp_reference_is_removed_as_unsupported() {
        let raw = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:xmpNote="http://ns.adobe.com/xmp/note/" xmpNote:HasExtendedXMP="GUID1234">
          <rdf:Description/>
        </rdf:RDF>"#;
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(!text.contains("HasExtendedXMP"));
        assert!(issues.iter().any(|i| {
            i.category == MetadataCategory::Xmp
                && i.reason == Reason::Unsupported
                && i.field.as_deref() == Some("extended_xmp")
        }));
    }

    #[test]
    fn modify_date_is_removed_but_capture_date_survives() {
        let raw = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:exif="http://ns.adobe.com/exif/1.0/">
          <rdf:Description>
            <xmp:ModifyDate>2024-01-01T00:00:00Z</xmp:ModifyDate>
            <exif:DateTimeOriginal>2020-05-05T10:00:00Z</exif:DateTimeOriginal>
          </rdf:Description>
        </rdf:RDF>"#;
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(!text.contains("ModifyDate"));
        assert!(text.contains("<exif:DateTimeOriginal>2020-05-05T10:00:00Z</exif:DateTimeOriginal>"));
        assert!(issues.iter().any(|i| {
            i.category == MetadataCategory::Xmp
                && i.reason == Reason::RemovedStale
                && i.field.as_deref() == Some("modify_date")
        }));
    }

    #[test]
    fn legacy_iptc_digest_is_preserved_since_iptc_bytes_are_never_edited() {
        // See the doc comment on `classify` for the full reasoning: this
        // digest hashes the legacy IPTC IIM block, not any XMP property or
        // the image's pixels/dimensions, and `policy::prepare` never
        // rewrites `Payload::Iptc` either -- so, unlike `xmp:ModifyDate`,
        // it never goes stale from anything this crate does and must
        // survive byte-for-byte, with no issue raised about it.
        let raw = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/"
          photoshop:LegacyIPTCDigest="D41D8CD98F00B204E9800998ECF8427E">
          <rdf:Description/>
        </rdf:RDF>"#;
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(text.contains("photoshop:LegacyIPTCDigest=\"D41D8CD98F00B204E9800998ECF8427E\""));
    }

    #[test]
    fn malformed_xml_is_rejected() {
        let raw = b"<rdf:RDF><rdf:Description></rdf:RDF>";
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        assert!(bytes.is_none());
        assert!(issues
            .iter()
            .any(|i| i.category == MetadataCategory::Xmp && i.reason == Reason::Malformed));
    }

    #[test]
    fn doctype_is_rejected() {
        let raw = b"<!DOCTYPE foo><rdf:RDF xmlns:rdf=\"ns\"><rdf:Description/></rdf:RDF>";
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        assert!(bytes.is_none());
        assert!(issues.iter().any(|i| i.reason == Reason::Unsupported));
    }

    #[test]
    fn undefined_entity_is_rejected() {
        let raw = b"<rdf:RDF xmlns:rdf=\"ns\"><rdf:Description>&bogus;</rdf:Description></rdf:RDF>";
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        assert!(bytes.is_none());
        assert!(issues.iter().any(|i| i.reason == Reason::Malformed));
    }

    #[test]
    fn predefined_entities_and_char_refs_are_accepted() {
        let raw = b"<rdf:RDF xmlns:rdf=\"ns\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
            <rdf:Description><dc:description>A &amp; B &#65;</dc:description></rdf:Description></rdf:RDF>";
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        assert!(bytes.is_some());
    }

    #[test]
    fn excessive_depth_is_rejected() {
        let mut raw = String::from("<rdf:RDF xmlns:rdf=\"ns\">");
        for _ in 0..10 {
            raw.push_str("<a><b><c>");
        }
        for _ in 0..10 {
            raw.push_str("</c></b></a>");
        }
        raw.push_str("</rdf:RDF>");
        let limits = MetadataLimits {
            max_xml_depth: 5,
            ..MetadataLimits::default()
        };
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(raw.as_bytes(), &facts, &limits);
        assert!(bytes.is_none());
        assert!(issues.iter().any(|i| i.reason == Reason::LimitExceeded));
    }

    #[test]
    fn oversized_payload_is_rejected() {
        let raw = vec![b'a'; 100];
        let limits = MetadataLimits {
            max_payload_bytes: 10,
            ..MetadataLimits::default()
        };
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&raw, &facts, &limits);
        assert!(bytes.is_none());
        assert!(issues.iter().any(|i| i.reason == Reason::LimitExceeded));
    }

    #[test]
    fn non_utf8_input_is_rejected() {
        let raw = vec![0xFF, 0xFE, 0x00, 0x01];
        let facts = facts(10, 10, OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&raw, &facts, &MetadataLimits::default());
        assert!(bytes.is_none());
        assert!(issues.iter().any(|i| i.reason == Reason::Malformed));
    }
}
