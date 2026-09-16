# Preserve Image Metadata Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve supported original embedded metadata in web and CLI image exports, correcting output-dependent fields and reporting losses without blocking image export.

**Architecture:** Add a platform-independent `r3sizer-metadata` crate used by native I/O and WASM. Read metadata from source bytes, apply an explicit output policy, and merge into already-encoded destination bytes; the numerical core remains metadata-independent. Generate shared report types into the existing TypeScript declarations and bind web exports to the source of their processed pixels.

**Tech Stack:** Rust 2021, workspace Rust minimum 1.87, `img-parts` 0.4.0, `quick-xml` 0.38, existing `flate2`/`crc32fast` dependency families, serde, optional ts-rs 12, wasm-bindgen, React/Zustand, Vitest/happy-dom. Use `kamadak-exif` 0.6 as an independent test reader only.

**Spec:** `docs/superpowers/specs/2026-09-13-preserve-image-metadata-design.md`

## Global Constraints

- "Preserve original embedded image metadata by default in web downloads and CLI outputs."
- "No companion files."
- "Do not promise universal or byte-identical preservation across formats."
- "Filesystem timestamps and permissions are outside the embedded-metadata scope."
- "The new crate inherits workspace package metadata, including version (currently `0.9.0`), edition, Rust version, and license."
- "Keep `ts-rs` out of production builds."
- "Existing image-format support must not be reduced."
- "A complete new color-management engine is outside scope; unsupported cases must produce an explicit message."
- "Metadata issues are stderr-only in the CLI: do not add fields to `diag.json`, JSON stdout, or sweep `summary.json`."
- "When a sweep has no output directory and writes no images, emit no preservation warnings."
- No new metadata controls, telemetry, companion files, external runtime tools, or numerical algorithm changes.
- Work on the existing `feature/preserve-image-metadata-web-cli` branch. Do not create another branch or worktree as part of this plan.

## Evidence and implementation decisions

The following references informed this plan. They describe capabilities and format rules; the dependency build and regression tests below remain required.

- [`img-parts` 0.4.0](https://docs.rs/img-parts/0.4.0/img_parts/) exposes JPEG, PNG, and WebP containers plus raw EXIF/ICC access. Use it for destination container manipulation, not EXIF tag editing or a claim of complete preservation.
- [`WebP` container API](https://docs.rs/img-parts/latest/img_parts/webp/struct.WebP.html) exposes chunk mutation. Its [0.4.0 source](https://docs.rs/img-parts/latest/src/img_parts/webp/mod.rs.html) leaves existing VP8X flag updates unfinished and omits XMP from extended-format inference. Use low-level chunk mutation plus our checked header construction, not its automatic metadata setters or dimension helper.
- [PNG specification](https://www.w3.org/TR/png-3/) defines eXIf, text, color chunks, CRCs, and ordering. Keep output-dependent image/color chunks under encoder control.
- [WebP format, RFC 9649](https://www.rfc-editor.org/rfc/rfc9649.html) defines RIFF lengths, padding, extended headers, and EXIF/XMP placement.
- [Adobe XMP specifications](https://developer.adobe.com/xmp/docs/xmp-specifications/) define the property namespaces and container packaging. Preserve namespace semantics, not assumptions about prefixes.
- [`NsReader`](https://docs.rs/quick-xml/latest/quick_xml/reader/struct.NsReader.html) supports namespace-aware reading. Use the selected 0.38 API and compile its tests before layering on integration.
- [`image` orientation API](https://docs.rs/image/latest/image/metadata/enum.Orientation.html) distinguishes applying a pixel transform from updating metadata. This feature must never reset orientation merely because a file was resized.

Repository inspection confirmed `image` is locked to 0.25.10; native loading currently calls `image::open` without an explicit orientation transform. Browser input is decoded to an ImageBitmap and drawn into a canvas. The output entity currently does not retain its source `File`. `web/Dockerfile` already copies the whole `crates/` directory, so it already includes the new crate; verify that build, do not add a redundant COPY.

### Concrete format policy

| Payload | JPEG destination | PNG destination | WebP destination |
| --- | --- | --- | --- |
| Corrected classic TIFF EXIF | APP1 EXIF | eXIf | EXIF chunk |
| Corrected standard XMP packet | APP1 standard XMP | iTXt `XML:com.adobe.xmp` | XMP chunk |
| Validated IPTC IIM in Photoshop IRB | APP13, retain IPTC resource | Unsupported, issue | Unsupported, issue |
| JPEG comments | COM | Unsupported, issue | Unsupported, issue |
| PNG tEXt/zTXt/iTXt other than XMP | Unsupported, issue | Preserve validated text records | Unsupported, issue |
| ICC profile | Only if output color facts verify it still applies | Same | Same |
| Validated source density | JFIF density for JPEG-to-JPEG | pHYs for PNG-to-PNG | Unsupported, issue |
| Unknown ancillary payloads | Preserve only after a documented placement/meaning rule; otherwise issue | Same | Same |

Do not invent private chunks to hide unsupported metadata. Cross-format translation of IPTC IIM and arbitrary text into new XMP schemas is outside this implementation. IPTC properties already in XMP travel with that XMP. Unsupported source formats produce `Unknown/Unverified` rather than claiming absence of metadata. Export pixel formats remain unchanged.

Use bounded classic TIFF patching, not a general EXIF serializer: validate all reachable IFD tables/value ranges; modify existing scalar dimension/orientation/color entries without relocating retained payloads. Remove unsafe entries with warnings. MakerNotes and embedded previews are deliberately excluded; no vendor-offset rewriting. Extended JPEG XMP is detected and reported as unsupported; do not claim its partial standard packet is complete.

Native metadata-aware loading initially preserves stored pixel orientation (`Preserve` facts), matching its current pixel decode behavior. Web uses explicit `imageOrientation: "from-image"` and explicit sRGB canvas contexts, with `Normalize` facts verified in real browsers. Browser color conversion must not be inferred from the current nonstandard `colorSpace` bitmap option. Source profiles are retained only when facts verify unchanged encoding, or when they exactly match a validated profile attached by the destination encoder. Otherwise retain the encoder's output color declaration and report the original profile as unverified/replaced. Do not identify a profile as sRGB by its human-readable description.

### Limits

Use named defaults, adjustable through the Rust library only: 256 MiB source encoded bytes, 8 MiB per metadata payload, 16 MiB total retained/decompressed metadata, 4,096 container records, 4,096 EXIF entries, 32 visited IFDs, and XML depth 64. Check source size before reading it in both clients. A limit skips the affected metadata with `LimitExceeded`; it must not reject an otherwise decodable image. Destination image bytes are not subject to the source-byte limit. Allocation and arithmetic checks precede copies/decompression. No additional full-resolution pixel decode is allowed for metadata work.

## File and interface map

New crate files:

- `crates/r3sizer-metadata/src/lib.rs`: public extraction/merge functions and shared orchestration.
- `src/types.rs`: Rust/serde/TS boundary types and output facts.
- `src/bundle.rs`: private payload records and public opaque `MetadataBundle`.
- `src/limits.rs`: budgets and checked cursor helpers.
- `src/containers/{mod,jpeg,png,webp}.rs`: inventory, payload packaging, destination validation.
- `src/exif/{mod,reader,patch}.rs`: bounded TIFF traversal and patching.
- `src/xmp.rs`: namespace-aware patching.
- `src/iptc.rs`: Photoshop IRB validation and supported IPTC resource retention.
- `src/policy.rs`: color, density, unsupported-payload decisions and issue deduplication.
- `tests/{contract,containers,exif,xmp,roundtrip}.rs`, `tests/fixtures/`: independent fixtures and assertions.

Integration files are named under their tasks. Do not put container code into CLI commands, React components, or `r3sizer-core`.

### Task 1: Establish the metadata contract and bounded fallback

**Files:** Create `crates/r3sizer-metadata/{Cargo.toml,README.md,src/lib.rs,src/types.rs,src/bundle.rs,src/limits.rs,tests/contract.rs}`. Modify root `Cargo.toml`, `Cargo.lock`.

**Consumes:** No existing processing types.

**Produces:** The following public contracts. All serializable enums use `snake_case`, derive `Serialize`, `Deserialize`, `Debug`, `Clone`, `PartialEq`, `Eq`, and optionally `TS`; enums also derive `Copy`. Structs use the same derives except `Copy`. `MetadataReport` additionally derives `Default`. Add public API doc comments.

```rust
pub enum MetadataCategory {
    Exif, Xmp, Iptc, Icc, Text, Thumbnail, MakerNote, Density, Unknown,
}
pub enum MetadataIssueReason {
    Unsupported, Malformed, RemovedStale, Unverified, LimitExceeded, MergeFailed,
}
pub struct MetadataIssue {
    pub category: MetadataCategory,
    pub reason: MetadataIssueReason,
    pub field: Option<String>, // tag name, text keyword, or container identifier; never tag value
}
pub struct MetadataReport { pub issues: Vec<MetadataIssue> }
pub enum OrientationAction { Preserve, Normalize }
pub enum ColorAction { Unchanged, Srgb, Unverified }
pub struct OutputFacts {
    pub width: u32,
    pub height: u32,
    pub orientation: OrientationAction,
    pub color: ColorAction,
}
pub struct MetadataExport { // native-only bytes wrapper, no serde/TS required
    pub bytes: Vec<u8>,
    pub report: MetadataReport,
}
pub struct MetadataBundle {
    pub(crate) format: SourceFormat,
    pub(crate) payloads: Vec<Payload>,
    pub(crate) source_color: SourceColor,
    pub(crate) report: MetadataReport,
}
pub enum SourceColor { Srgb, Unspecified, Other, Unknown }
pub(crate) enum SourceFormat { Jpeg, Png, WebP, Unknown }
pub(crate) enum Payload {
    Exif(Vec<u8>), Xmp(Vec<u8>), Iptc(Vec<u8>), Icc(Vec<u8>),
    JpegComment(Vec<u8>),
    PngText { kind: [u8; 4], data: Vec<u8> },
    JfifDensity { units: u8, x: u16, y: u16 },
    PngDensity { x: u32, y: u32, unit: u8 },
}
pub struct MetadataLimits {
    pub max_source_bytes: usize,
    pub max_payload_bytes: usize,
    pub max_total_metadata_bytes: usize,
    pub max_records: usize,
    pub max_exif_entries: usize,
    pub max_ifds: usize,
    pub max_xml_depth: usize,
}
pub fn extract(source: &[u8], limits: &MetadataLimits) -> MetadataBundle;
pub fn merge(
    encoded: Vec<u8>, source: &MetadataBundle,
    facts: &OutputFacts, limits: &MetadataLimits,
) -> MetadataExport;
impl MetadataBundle {
    pub fn unavailable(reason: MetadataIssueReason) -> Self;
    pub fn report(&self) -> &MetadataReport;
    pub fn source_color(&self) -> SourceColor;
}
```

Bundle/private payload types derive `Debug` and `Clone`; format and color enums
also derive `Copy`, `PartialEq`, and `Eq`. They do not cross WASM boundaries and
need no serde/TS derives. Define `SourceColor` in `bundle.rs` and re-export it.
Inventory sets `Other` for an unconverted ICC/custom color declaration,
`Srgb` for a verified sRGB declaration without a conflicting profile,
`Unspecified` for verified absence of color declarations in a supported format,
and `Unknown` for unsupported or unreadable inventory. This gives native I/O a
concrete way to choose output facts without inspecting private bundle fields.

- [ ] **Step 1: Add workspace registration, manifest, and failing contract test.** Inherit all workspace package fields used by `r3sizer-io`; production dependencies are workspace serde plus optional workspace ts-rs (`typegen = ["dep:ts-rs"]`). Add codec dependencies in the tasks that use them, not before.

```rust
use r3sizer_metadata::*;

#[test]
fn unknown_source_warns_and_leaves_output_untouched() {
    let limits = MetadataLimits::default();
    let source = extract(b"unrecognized source", &limits);
    let encoded = b"unsupported encoded output".to_vec();
    let facts = OutputFacts {
        width: 8, height: 4,
        orientation: OrientationAction::Preserve,
        color: ColorAction::Unverified,
    };
    let result = merge(encoded.clone(), &source, &facts, &limits);
    assert_eq!(result.bytes, encoded);
    assert!(result.report.issues.iter().any(|issue|
        issue.category == MetadataCategory::Unknown &&
        issue.reason == MetadataIssueReason::Unverified));
}
```

- [ ] **Step 2:** Run `cargo test -p r3sizer-metadata --test contract`; expect unresolved public API failures before implementation.
- [ ] **Step 3: Implement the contracts, defaults, and conservative fallback.** For now `extract` returns unavailable/unverified; later adapters replace this case. `merge` returns ownership of the original bytes on unsupported destinations. Implement checked range math in one helper; never index input with unchecked offsets.

```rust
pub(crate) fn checked_range(
    offset: usize, count: usize, unit: usize, length: usize,
) -> Option<std::ops::Range<usize>> {
    let end = offset.checked_add(count.checked_mul(unit)?)?;
    (end <= length).then_some(offset..end)
}
```

- [ ] **Step 4:** Add tests for overflow, truncated ranges, source-byte limit, and stable snake_case report serialization. Run `cargo test -p r3sizer-metadata`; expect all contract tests to pass.
- [ ] **Step 5:** Commit only this task's files: `feat(metadata): add shared export contracts and limits`.

### Task 2: Read JPEG, PNG, and WebP metadata without decoding pixels

**Files:** Create `src/containers/{mod,jpeg,png,webp}.rs`, `src/iptc.rs`, `tests/containers.rs`, `tests/fixtures/README.md` under the new crate. Modify `src/{lib,bundle,limits}.rs`, new crate manifest and lockfile.

**Consumes:** Task 1 contracts and budgets.

**Produces:** `containers::extract_payloads(source: &[u8], limits: &MetadataLimits) -> MetadataBundle`. Populate Task 1's payload variants, source format/color and extraction report. The private bundle stores owned metadata bytes only; it must not retain the compressed image. Public callers cannot construct unchecked payloads.

- [ ] **Step 1: Add failing inventory tests.** Begin with byte-level PNG fixtures so extraction tests cannot accidentally depend on the production writer. Add `crc32fast = "1"` and `flate2 = { version = "1", default-features = false, features = ["rust_backend"] }` as production dependencies.

```rust
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32fast::hash(&out[4..]);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

#[test]
fn png_text_is_detected_without_a_pixel_decode() {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(chunk(b"IHDR", &[0,0,0,1,0,0,0,1,8,2,0,0,0]));
    png.extend(chunk(b"tEXt", b"Author\0Fixture Author"));
    png.extend(chunk(b"IDAT", &[])); // container fixture; deliberately not a valid pixel stream
    png.extend(chunk(b"IEND", &[]));
    let bundle = extract(&png, &MetadataLimits::default());
    assert!(bundle.report().issues.is_empty());
    // Add a module-local assertion in containers/png.rs that the bundle
    // contains exactly one PngText payload with the original bytes.
}
```

- [ ] **Step 2:** Run `cargo test -p r3sizer-metadata --test containers`; expect recognized, valid metadata to incorrectly yield the initial unverified fallback.
- [ ] **Step 3: Implement checked container scans and inventory.** JPEG: validate segment lengths; recognize APP1 EXIF prefix, standard/extended XMP prefixes, APP2 ICC sequence fields, APP13 Photoshop IRBs, COM, and APP0 JFIF density. Scan through entropy data using stuffed-byte/restart marker rules so later scans/metadata cannot be silently missed. Skip pixel segments; flag unrecognized APP/trailer payloads. Assemble ICC segments only with a complete, unique, consistent sequence. EXIF payload stored in the bundle starts at its TIFF byte-order marker, without `Exif\0\0`.

PNG: validate framing and CRC, require IHDR/IEND and a structurally valid order, skip IDAT bytes without inflating them. Recognize eXIf, iTXt XMP, all three text forms, iCCP and pHYs. Bound decompression of iCCP/zTXt/compressed iTXt using the remaining total budget plus one byte; check the extra byte before retaining. Preserve text keyword, language, translated keyword, encoding, and multiplicity. Never treat XMP iTXt as ordinary text too. Color declarations and image-dependent ancillary chunks are inventoried as policy decisions, not blindly copied.

WebP: validate RIFF size, WEBP signature, chunk lengths and odd-byte padding; extract EXIF, XMP, ICCP. Normalize an optional EXIF identifier prefix to raw TIFF. Do not recurse into arbitrary RIFF LIST records. Recognize VP8/VP8L/VP8X/ALPH/animation as image structure; report unsupported metadata chunks. Trailing bytes in any format produce an unverified issue.

IRB: recognize `Photoshop 3.0\0`, `8BIM` signature, big-endian resource id, padded Pascal name, length and padded data. Retain resource `0x0404` after bounded IPTC IIM record validation; remove thumbnail resources `0x0409`/`0x040c` with `Thumbnail/RemovedStale`. Other resources produce named issues rather than copying possible previews/dimension data.

Use the same decompression guard everywhere:

```rust
use std::io::Read;
fn inflate_bounded(data: &[u8], max: usize) -> std::io::Result<Vec<u8>> {
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut result = Vec::new();
    decoder.by_ref().take(max as u64 + 1).read_to_end(&mut result)?;
    if result.len() > max {
        return Err(std::io::Error::other("metadata limit exceeded"));
    }
    Ok(result)
}
```

Map decompression failure to `Malformed`, size exhaustion to `LimitExceeded`; use a typed internal error so classification does not depend on error-string matching. Precheck `max + 1` for overflow in the production helper.

- [ ] **Step 4:** Add inventory fixtures for JPEG ICC out-of-order/missing/duplicate sequences, metadata after a progressive scan, PNG corrupted CRC, compressed text exceeding budget, duplicate EXIF/XMP, WebP odd-size XMP, extended XMP, IPTC, unknown APP/chunk, and truncated containers. For conflicting singleton metadata, drop the ambiguous category and report it. Run container and contract tests. Document every fixture's origin; use authored fixtures with no personal GPS or copyrighted photographs.
- [ ] **Step 5:** Commit: `feat(metadata): extract bounded image metadata payloads`.

### Task 3: Correct EXIF without relocating opaque offsets

**Files:** Create `src/exif/{mod,reader,patch}.rs`, `tests/exif.rs`; modify new crate manifest for dev-only `kamadak-exif = "0.6"` and lockfile.

**Consumes:** Raw TIFF bytes, `OutputFacts`, `MetadataLimits`.

**Produces:** `exif::correct(raw: &[u8], facts: &OutputFacts, limits: &MetadataLimits) -> (Option<Vec<u8>>, Vec<MetadataIssue>)`. `None` means omit EXIF with the returned reason. Private parser returns checked entry locations, types, value ranges, IFD identities, and next-IFD-pointer locations. It never calls `image` or reads files.

- [ ] **Step 1: Add a hand-built EXIF fixture and independent-reader test.** Extend this helper with explicit entries for capture date, copyright, and GPS in separate tests; helper names are test-only and defined in their test module.

```rust
fn tiny_exif() -> Vec<u8> {
    let mut b = b"II\x2a\0\x08\0\0\0".to_vec();
    b.extend(3u16.to_le_bytes());
    for (tag, kind, value) in [(0x0100u16, 4u16, 400u32),
                               (0x0101, 4, 200), (0x0112, 3, 6)] {
        b.extend(tag.to_le_bytes()); b.extend(kind.to_le_bytes());
        b.extend(1u32.to_le_bytes()); b.extend(value.to_le_bytes());
    }
    b.extend(0u32.to_le_bytes());
    b
}

// Keep this test inside exif/mod.rs so it can call the private correct().
#[test]
fn updates_dimensions_and_normalizes_only_on_request() {
    let facts = OutputFacts { width: 100, height: 50,
        orientation: OrientationAction::Normalize, color: ColorAction::Unverified };
    let (bytes, issues) = correct(&tiny_exif(), &facts, &MetadataLimits::default());
    assert!(issues.is_empty());
    let exif = ::exif::Reader::new().read_raw(bytes.unwrap()).unwrap();
    assert_eq!(exif.get_field(::exif::Tag::ImageWidth, ::exif::In::PRIMARY)
        .unwrap().value.get_uint(0), Some(100));
    assert_eq!(exif.get_field(::exif::Tag::Orientation, ::exif::In::PRIMARY)
        .unwrap().value.get_uint(0), Some(1));
}
```

Inside the `exif` module use `::exif` for the independent crate to avoid the local module name collision.

- [ ] **Step 2:** Run `cargo test -p r3sizer-metadata exif`; expect the independent reader to observe original dimensions/orientation before patching exists.
- [ ] **Step 3: Implement two-phase validation then patching.** Support little- and big-endian classic TIFF (magic 42). Reject BigTIFF with `Unsupported`. Traverse IFD0, ExifIFD (`0x8769`), GPSIFD (`0x8825`), InteropIFD (`0xa005`) with a visited-offset set; validate table and value spans with checked arithmetic. Enforce entry/IFD limits. Reject cycles, conflicting duplicate tags, overlapping table/value regions, unsupported TIFF type widths, and invalid pointer counts before mutating bytes.

Update existing IFD0 width/height (`0x0100`, `0x0101`) and ExifIFD pixel dimensions (`0xa002`, `0xa003`) as count-one SHORT/LONG; promote SHORT to LONG in the same 12-byte entry when needed. Normalize existing orientation (`0x0112`) to 1 only for `Normalize`; preserve values 1–8 for `Preserve`, and drop invalid orientation with a named issue. Color policy supplies the final handling of Exif ColorSpace (`0xa001`) and InteropIndex (`0x0001` in InteropIFD): set a consistent known value or remove a conflicting/unverified declaration with an issue.

Retain validated standard value payloads (including rationals, strings, GPS and UserComment) at their original offsets. Remove MakerNote (`0x927c`), SubIFDs (`0x014a`) and offset-bearing preview records with named issues. Detach the next-IFD chain used for thumbnails; zero discarded exclusively-owned thumbnail tables/payloads. Repack retained 12-byte entries within each existing IFD table, update its entry count and next pointer, zero the freed table bytes, and keep all retained out-of-line value offsets unchanged. If discarded ranges alias retained ranges, omit the EXIF block as malformed rather than risking corruption. A private/unknown tag whose interpretation may carry offsets is dropped with `Unverified`; do not silently bless opaque pointer data. Standard scalar unknown values may be retained only with an unverified issue.

The patch selector should be explicit, independently unit-tested, and include this mapping:

```rust
fn output_dimension(tag: u16, width: u32, height: u32) -> Option<u32> {
    match tag {
        0x0100 | 0xa002 => Some(width),
        0x0101 | 0xa003 => Some(height),
        _ => None,
    }
}
```

Do not add absent metadata tags solely to make the output look more complete. Structural pixel-storage TIFF tags must not be copied from an actual TIFF source file as though it were an EXIF payload; this iteration reports TIFF source extraction as unsupported.

- [ ] **Step 4:** Add independent assertions for both byte orders, all eight orientation values in Preserve/Normalize modes, SHORT promotion, ExifIFD dimensions, rational GPS/capture settings, embedded preview removal, MakerNote removal, cycles, overlaps, malformed counts, and retained value-offset stability. Run `cargo test -p r3sizer-metadata exif` and the full crate tests.
- [ ] **Step 5:** Commit: `feat(metadata): correct EXIF fields with bounded TIFF patching`.

### Task 4: Correct XMP and apply color/density preservation policy

**Files:** Create `src/{xmp,policy}.rs`, `tests/xmp.rs`; modify crate manifest and lockfile for `quick-xml = "0.38"` with default UTF-8 support, no network/DTD resolver.

**Consumes:** `OutputFacts`, validated payloads and limits.

**Produces:** `xmp::correct(raw: &[u8], facts: &OutputFacts, limits: &MetadataLimits) -> (Option<Vec<u8>>, Vec<MetadataIssue>)`; `policy::prepare(bundle: &MetadataBundle, facts: &OutputFacts, destination_icc: Option<&[u8]>, limits: &MetadataLimits) -> MetadataBundle`. The prepared bundle retains extraction issues and adds correction issues, deduplicated by category/reason/field in first-seen order.

- [ ] **Step 1: Add namespace-alias regression test.** Put private-function tests in the module, integration preservation assertions in `tests/xmp.rs` once merge is available.

```rust
#[test]
fn updates_xmp_using_namespace_uri_not_prefix() {
    let raw = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
      <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
       <rdf:Description xmlns:t="http://ns.adobe.com/tiff/1.0/"
        xmlns:dc="http://purl.org/dc/elements/1.1/" t:ImageWidth="400"
        t:Orientation="6"><t:ImageLength>200</t:ImageLength>
        <dc:description>Original description</dc:description></rdf:Description>
      </rdf:RDF></x:xmpmeta>"#;
    let facts = OutputFacts { width: 100, height: 50,
        orientation: OrientationAction::Normalize, color: ColorAction::Srgb };
    let (bytes, issues) = correct(raw, &facts, &MetadataLimits::default());
    assert!(issues.is_empty());
    let text = String::from_utf8(bytes.unwrap()).unwrap();
    assert!(text.contains("t:ImageWidth=\"100\""));
    assert!(text.contains("<t:ImageLength>50</t:ImageLength>"));
    assert!(text.contains("t:Orientation=\"1\""));
    assert!(text.contains("Original description"));
}
```

- [ ] **Step 2:** Run `cargo test -p r3sizer-metadata xmp`; expect unchanged source values to fail.
- [ ] **Step 3: Implement a namespace-aware event transformation with `NsReader` and `Writer`.** Match namespace URI plus local name for attributes and simple elements: TIFF ImageWidth/ImageLength/Orientation, EXIF PixelXDimension/PixelYDimension/ColorSpace, and TIFF/EXIF color declarations that must be removed if unverified. Preserve unrelated elements, namespaces, language alternatives, RDF arrays, rights, descriptions, GPS and custom properties. Normalize numeric fields consistently with EXIF. Preserve capture dates. Remove `xmp:Thumbnails` and `xmpNote:HasExtendedXMP` with explicit thumbnail/unsupported issues when the referenced data is not carried. Do not claim existing signatures/digests remain valid after packet edits; omit identified stale digest/signature properties with issues. Unsupported representations of a field that must change cause that property to be removed with an issue; if it cannot be isolated safely, omit the packet with `Unverified`.

Reject malformed XML, DTDs, undefined entities, excessive depth/payloads, non-UTF-8 input and conflicting duplicate scalar properties with a category-specific issue. Do not use regex replacements. Treat empty/self-closing fields and attribute values as separate cases. Do not replace the words ImageWidth or Orientation in user descriptions.

Use these URI constants in the module:

```rust
const TIFF_NS: &[u8] = b"http://ns.adobe.com/tiff/1.0/";
const EXIF_NS: &[u8] = b"http://ns.adobe.com/exif/1.0/";
const XMP_NS: &[u8] = b"http://ns.adobe.com/xap/1.0/";
const XMP_NOTE_NS: &[u8] = b"http://ns.adobe.com/xmp/note/";
```

`ColorAction::Unchanged` is an explicit caller guarantee and allows a validated source ICC. `Srgb` keeps destination color declarations, allows source ICC only when it equals a validated destination profile, and otherwise emits `Icc/Unverified` for the source profile. `Unverified` omits source color declarations with issues; it must not set EXIF/XMP sRGB based on an unproven conversion. Validate ICC declared size, header signature and tag-table spans before retention; validation proves structure, not equivalence to sRGB. Keep density for same-format exports; do not change DPI merely because pixel dimensions shrink. Stale modification timestamps and pixel-derived ancillary metadata are omitted with issues, while capture timestamps are retained.

- [ ] **Step 4:** Test default and aliased namespaces, attributes/elements, arrays/rights, descriptions containing tag names, malformed XML, depth limits, thumbnails, extended-XMP references, ICC equality/mismatch/malformed profiles, and unchanged density. Run the complete metadata crate tests.
- [ ] **Step 5:** Commit: `feat(metadata): reconcile XMP and output color policy`.

### Task 5: Merge corrected metadata into encoded images and verify the result

**Files:** Modify `src/{lib,policy}.rs`, `src/containers/{mod,jpeg,png,webp}.rs`; create `tests/roundtrip.rs`, `tests/fixtures/{plain.jpg,plain.png,plain.webp,metadata.jpg,metadata.png,metadata.webp,expected.json}`. Modify crate manifest/lockfile for production `img-parts = "0.4.0"`; dev-only `image` with JPEG/PNG/WebP and `serde_json`.

**Consumes:** Extracted bundles, Tasks 3–4 corrections and policy, destination bytes and facts.

**Produces:** Completed public `merge()` and private `containers::embed(encoded: Vec<u8>, prepared: &MetadataBundle, facts: &OutputFacts, limits: &MetadataLimits) -> MetadataExport`. Unsupported payloads are reported individually. Container failures return the original encoded bytes with `MergeFailed`; partial successful retention must list all omitted categories.

- [ ] **Step 1: Generate authored pixel fixtures independently of production metadata code.** Create 32×16 RGB color blocks with `image` encoders in a test fixture-generation helper, then inject hand-built metadata records using test-only byte code. Commit the resulting files and expected values; the normal test run reads them and never rewrites fixtures. Use fictional values: Artist `Fixture Author`, Copyright `Fixture Copyright`, DateTimeOriginal `2024:01:02 03:04:05`, GPS 1°2′3″N 4°5′6″E, description `Metadata fixture`. Include an EXIF value-offset map in `expected.json`. Keep the generation recipe in the fixture README.

The pixel-generation code should use the encoder independently of the feature:

```rust
let rgb = image::RgbImage::from_fn(32, 16, |x, y| {
    image::Rgb([if x < 16 { 240 } else { 20 }, if y < 8 { 180 } else { 40 }, 90])
});
let mut encoded = std::io::Cursor::new(Vec::new());
rgb.write_to(&mut encoded, image::ImageFormat::Png).unwrap();
```

For JPEG EXIF injection use APP1 after SOI with a big-endian segment length of `2 + 6 + tiff.len()`, payload `Exif\0\0` plus TIFF. For PNG use the independent `chunk` helper from Task 2 before IDAT. For WebP rebuild RIFF length/padding and VP8X metadata flags using the RFC, not the production adapter. Independently inspect committed fixtures with `kamadak-exif` and, during fixture preparation, ExifTool if available; ExifTool is not a runtime dependency and tests must not silently skip when it is absent.

- [ ] **Step 2: Add a failing cross-format test and run it.** Every destination is compared against its own pre-merge decoded pixels, avoiding JPEG re-encoding comparisons.

```rust
#[test]
fn preserves_artist_across_all_supported_destinations() {
    use r3sizer_metadata::*;
    let limits = MetadataLimits::default();
    let sources: [&[u8]; 3] = [include_bytes!("fixtures/metadata.jpg"),
        include_bytes!("fixtures/metadata.png"), include_bytes!("fixtures/metadata.webp")];
    let destinations: [&[u8]; 3] = [include_bytes!("fixtures/plain.jpg"),
        include_bytes!("fixtures/plain.png"), include_bytes!("fixtures/plain.webp")];
    for source in sources {
        let bundle = extract(source, &limits);
        for dest in destinations {
            let facts = OutputFacts { width: 32, height: 16,
                orientation: OrientationAction::Normalize, color: ColorAction::Srgb };
            let output = merge(dest.to_vec(), &bundle, &facts, &limits);
            let tags = exif::Reader::new().read_from_container(
                &mut std::io::Cursor::new(&output.bytes)).unwrap();
            assert!(tags.get_field(exif::Tag::Artist, exif::In::PRIMARY)
                .unwrap().display_value().to_string().contains("Fixture Author"));
            assert_eq!(image::load_from_memory(&output.bytes).unwrap().to_rgba8(),
                       image::load_from_memory(dest).unwrap().to_rgba8());
        }
    }
}
```

Run `cargo test -p r3sizer-metadata --test roundtrip`; expect missing output metadata before embedding exists.

- [ ] **Step 3: Implement destination adapters with `img-parts`.** Parse owned encoded bytes, keep the original available until serialization and validation succeed, insert corrected payloads, and serialize. Example API use for the JPEG EXIF branch:

```rust
use img_parts::{jpeg::Jpeg, ImageEXIF};
let mut jpeg = Jpeg::from_bytes(encoded.clone().into())?;
jpeg.set_exif(Some(corrected_tiff.into()));
let mut merged = Vec::new();
jpeg.encoder().write_to(&mut merged)?;
```

Do not use that example without the adapter's payload-size precheck: JPEG APP payload max is 65,533 bytes, including identifiers. Oversized EXIF/standard XMP is omitted with a named issue; do not truncate or split EXIF. ICC uses correctly sequenced APP2 segments. Remove destination metadata of a category before replacing it, but retain encoder-generated structural/color declarations selected by policy. Preserve destination entropy/pixel data exactly. Insert PNG eXIf/text in legal order and recompute CRCs. Do not copy source palette/transparency/image chunks. JPEG density updates only the destination JFIF density fields; do not copy a source JFIF thumbnail or Adobe transform marker. PNG density updates only pHYs.

For WebP, use `chunks_mut()` and `RiffChunk::new` directly, avoiding automatic format conversion in `set_exif`/`set_icc_profile`. Ensure VP8X exists when EXIF/XMP/ICC requires it, set feature flags from actual chunks (including existing alpha), maintain required chunk ordering, and update RIFF size/padding. Never insert a second VP8X. Validate dimensions from VP8/VP8L or VP8X using the bounded scanner, not the library's dimension helper. VP8X dimensions are bytes 4..7 and 7..10 of its 10-byte payload. Use this checked construction:

```rust
fn vp8x_header(width: u32, height: u32, flags: u8) -> Option<[u8; 10]> {
    if width == 0 || height == 0 || width > 0x0100_0000 || height > 0x0100_0000 {
        return None;
    }
    let mut data = [0u8; 10];
    data[0] = flags;
    data[4..7].copy_from_slice(&(width - 1).to_le_bytes()[..3]);
    data[7..10].copy_from_slice(&(height - 1).to_le_bytes()[..3]);
    Some(data)
}
```

Flag bits are ICC 0x20, alpha 0x10, EXIF 0x08, XMP 0x04, animation 0x02; reserved bits stay zero. Detect VP8L alpha from its bitstream header even without an ALPH chunk. In addition to the roundtrip tests, assert that XMP-only insertion produces a valid extended header, and that adding EXIF to an existing extended/alpha file updates flags without losing alpha.

Before returning merged bytes, run the bounded container validator and compare the intended metadata payloads against its inventory. Validate image dimensions against output facts from the encoded header without decoding pixels. A mismatch is a merge failure, not a successful preservation report. On failure return original bytes and add `MergeFailed` for every attempted category so no retained-data claim survives rollback.

- [ ] **Step 4:** Run the 3×3 matrix for EXIF/XMP, same-format IPTC/text/comments/density, unsupported cross-format categories, no metadata, destination encoder color tags, JPEG segment limits, opaque blocks, corrupt metadata and container rollback. Assert thumbnail/MakerNote absence with the independent EXIF reader; check PNG CRC and WebP flags/ordering with test-only byte readers. Confirm no source-file bytes or EXIF values leak into issue text.
- [ ] **Step 5:** Run `cargo test -p r3sizer-metadata`, `cargo check -p r3sizer-metadata --target wasm32-unknown-unknown`, and `cargo clippy -p r3sizer-metadata --all-targets -- -D warnings`. Verify new dependencies support workspace Rust 1.87, using `cargo +1.87 check -p r3sizer-metadata --lib` when that toolchain is available; install it through the normal approval mechanism if required. Commit: `feat(metadata): preserve metadata in JPEG PNG and WebP exports`.

### Task 6: Add metadata-aware native I/O while keeping existing APIs

**Files:** Modify `crates/r3sizer-io/{Cargo.toml,src/lib.rs,src/load.rs,src/save.rs}`; create `crates/r3sizer-io/src/metadata.rs`, `crates/r3sizer-io/tests/metadata.rs`; update lockfile.

**Consumes:** `r3sizer-metadata` public API and existing linear pixel conversion helpers.

**Produces:** Additive public APIs, re-exported from `r3sizer-io`:

```rust
pub struct LoadedImage {
    pub image: r3sizer_core::LinearRgbImage,
    pub metadata: r3sizer_metadata::MetadataBundle,
    pub orientation: r3sizer_metadata::OrientationAction,
    pub color: r3sizer_metadata::ColorAction,
}
pub fn load_with_metadata(
    path: &std::path::Path, decode_limits: &DecodeLimits,
    metadata_limits: &r3sizer_metadata::MetadataLimits,
) -> Result<LoadedImage, IoError>;
pub fn save_with_metadata(
    image: &r3sizer_core::LinearRgbImage, path: &std::path::Path,
    source: &LoadedImage, limits: &r3sizer_metadata::MetadataLimits,
) -> Result<r3sizer_metadata::MetadataReport, IoError>;
```

Re-export report/category/reason/limits types from I/O for CLI use so the CLI does not need another production dependency. `save_from_linear` and `load_as_linear[_with_limits]` retain their pixel-only signatures and semantics; factor pixel encoding into a private helper shared with `save_with_metadata`.

- [ ] **Step 1: Add native integration tests using Task 5 committed fixtures.** Add independent EXIF reading as a development dependency.

```rust
#[test]
fn native_export_preserves_source_orientation_and_artist() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("input.jpg");
    let dest = dir.path().join("output.png");
    std::fs::write(&source,
        include_bytes!("../../r3sizer-metadata/tests/fixtures/metadata.jpg")).unwrap();
    let limits = r3sizer_metadata::MetadataLimits::default();
    let loaded = r3sizer_io::load_with_metadata(
        &source, &r3sizer_io::DecodeLimits::default(), &limits).unwrap();
    let expected = exif::Reader::new().read_from_container(
        &mut std::io::BufReader::new(std::fs::File::open(&source).unwrap())).unwrap();
    r3sizer_io::save_with_metadata(&loaded.image, &dest, &loaded, &limits).unwrap();
    let actual = exif::Reader::new().read_from_container(
        &mut std::io::BufReader::new(std::fs::File::open(&dest).unwrap())).unwrap();
    assert_eq!(actual.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .unwrap().value.get_uint(0), expected.get_field(exif::Tag::Orientation,
        exif::In::PRIMARY).unwrap().value.get_uint(0));
}
```

- [ ] **Step 2:** Run `cargo test -p r3sizer-io --test metadata`; expect unresolved additive APIs.
- [ ] **Step 3: Implement bounded source reading and encode-then-merge.** Open the source once for metadata-aware loading and ensure pixels/metadata are read from that same file identity. For sources within the metadata cap, decode the same read bytes after checking pixel dimensions against `DecodeLimits`; for larger sources, decode from the open file and return a limit issue without allocating all encoded bytes. Avoid reading from the path again after export begins, including input=output cases. Successful pixel decode plus metadata read/parse failure returns a loaded image with `MetadataBundle::unavailable(...)`; pixel decode/read failure still returns `IoError`.

Native `orientation` is `Preserve` while using the current untransformed decode. Select `Srgb` only for inputs whose color declaration/absence is supported by the native sRGB assumption; select `Unverified` for an ICC/custom color declaration the decoder did not demonstrably convert. Do not infer color facts from filename. For source formats outside metadata adapters use `Unverified` color facts.

Build save facts from actual processed dimensions:

```rust
let facts = r3sizer_metadata::OutputFacts {
    width: image.width(), height: image.height(),
    orientation: source.orientation, color: source.color,
};
let exported = r3sizer_metadata::merge(encoded, &source.metadata, &facts, limits);
std::fs::write(path, &exported.bytes)?;
Ok(exported.report)
```

`encoded` is produced by the factored existing encoder using the destination extension. For an output format that cannot use an in-memory writer with current APIs, preserve the original save path and return unsupported metadata issues; do not reduce supported formats. Unsupported output with verified empty source metadata need not warn. Disk failures remain errors and suppress a successful-export claim.

- [ ] **Step 4:** Test pixel parity with the old API, default dimension limits, reduced metadata caps, no metadata, malformed metadata with valid pixels, unsupported BMP/TIFF destinations, input=output, file-writing failure, native orientation values, and ICC warnings. Run `cargo test -p r3sizer-io`.
- [ ] **Step 5:** Commit: `feat(io): carry source metadata through image export`.

### Task 7: Integrate CLI processing and per-file sweep warnings

**Files:** Modify `crates/r3sizer/src/{main,run,sweep}.rs`; create `crates/r3sizer/src/metadata.rs`, `crates/r3sizer/tests/metadata_cli.rs`; update `docs/cli.md` and `crates/r3sizer/README.md`.

**Consumes:** Task 6 additive I/O functions and re-exported report types.

**Produces:** `metadata::write_metadata_warnings(writer: &mut impl std::io::Write, path: &Path, report: &MetadataReport) -> std::io::Result<()>`; both commands write warnings after a successful image write, one line per distinct issue. Format: `warning: <output path>: metadata <category>/<reason> (<field>)`. Omit parentheses for no field. Escape control characters in field identifiers/path display to keep one issue on one line; never print tag values.

- [ ] **Step 1: Add a failing CLI regression test.** The test fixture used here contains a MakerNote that will be dropped by Task 3; create and independently verify `metadata-with-makernote.jpg` in the shared fixture directory with the same authorship convention as Task 5.

```rust
#[test]
fn process_reports_metadata_loss_only_on_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("input.jpg");
    let dst = dir.path().join("output.png");
    let diag = dir.path().join("diag.json");
    std::fs::write(&src, include_bytes!(
        "../../r3sizer-metadata/tests/fixtures/metadata-with-makernote.jpg")).unwrap();
    let output = assert_cmd::Command::cargo_bin("r3sizer").unwrap()
        .args(["process", "-i"]).arg(&src).arg("-o").arg(&dst)
        .args(["--width", "16", "--height", "8", "--output-format", "json"])
        .arg("--diagnostics").arg(&diag).output().unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("maker_note"));
    assert!(stderr.contains(dst.to_str().unwrap()));
    let stdout: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(diag).unwrap()).unwrap();
    assert_eq!(stdout, saved);
    assert!(saved.get("metadata").is_none());
    assert!(saved.get("metadata_issues").is_none());
}
```

`--output-format json` is the existing clap flag in `args.rs`; no new flag is needed. Add the fixture to the test-only commit inputs.

- [ ] **Step 2:** Run `cargo test -p r3sizer --test metadata_cli`; expect the missing stderr warning to fail.
- [ ] **Step 3: Replace pixel-only load/save calls in `run.rs` with Task 6 APIs.** Use `loaded.image` when resolving dimensions/processing and keep `loaded` until saving finishes. Render returned issues on stderr. Keep `AutoSharpDiagnostics`, `output.rs` JSON formatting and success codes unchanged.

In `sweep.rs`, use metadata-aware loading only when `out_dir.is_some()`. For no-output sweeps retain pixel-only loading. Each successful PNG save gets its own warning lines, and `FileResult`/`SweepSummary` stay unchanged. No warning entries enter `errors`. Do not touch generated corpus output. A stderr write failure should follow existing CLI error conventions; it must not be misclassified as a pixel or metadata parse failure.

The command integration is deliberately small:

```rust
let report = save_with_metadata(&output.image, &args.output, &loaded, &metadata_limits)
    .with_context(|| format!("failed to save output file: {}", args.output.display()))?;
write_metadata_warnings(&mut std::io::stderr().lock(), &args.output, &report)?;
```

- [ ] **Step 4:** Add a two-input sweep test with one warning-bearing source and one no-metadata source; assert warning path, `aggregate.succeeded == 2`, `aggregate.failed == 0`, empty `errors`, and exactly the existing summary/result JSON key sets. Repeat without `--out-dir`: assert no `metadata` warning and no image files. Assert real decode/write failures still fail. Run `cargo test -p r3sizer --test metadata_cli` and existing CLI tests. Document stderr-only behavior and unsupported categories in CLI docs.
- [ ] **Step 5:** Commit: `feat(cli): preserve metadata and report per-output losses`.

### Task 8: Generate metadata boundary types and expose the WASM operation

**Files:** Modify `crates/r3sizer-metadata/{Cargo.toml,src/types.rs}`, `crates/r3sizer-core/{Cargo.toml,tests/typegen.rs}`, `crates/r3sizer-wasm/{Cargo.toml,src/lib.rs}`, lockfile; create `crates/r3sizer-wasm/src/metadata.rs`, `crates/r3sizer-wasm/tests/metadata.rs`. Regenerate `web/src/shared/lib/types/generated.ts`; modify `web/src/shared/lib/types/wasm-types.ts`.

**Consumes:** Tasks 1–5 public APIs.

**Produces:** Generated boundary data and stateless WASM export. Add production `serde_bytes = "0.11"` to metadata; it carries no platform dependency. Define these in metadata `types.rs` with serde/TS derives:

```rust
pub struct MetadataExportRequest {
    #[serde(with = "serde_bytes")]
    #[cfg_attr(feature = "typegen", ts(type = "Uint8Array"))]
    pub source: Vec<u8>,
    #[serde(with = "serde_bytes")]
    #[cfg_attr(feature = "typegen", ts(type = "Uint8Array"))]
    pub encoded: Vec<u8>,
    pub facts: OutputFacts,
}
pub struct MetadataExportResponse {
    #[serde(with = "serde_bytes")]
    #[cfg_attr(feature = "typegen", ts(type = "Uint8Array"))]
    pub bytes: Vec<u8>,
    pub report: MetadataReport,
}
```

WASM export signature: `pub fn preserve_metadata(request: JsValue) -> Result<JsValue, JsValue>`. It does not read/write processing caches. Invalid boundary arguments are errors; metadata parse/merge issues are successful responses with a report.

- [ ] **Step 1: Add WASM serialization regression tests.** Add `wasm-bindgen-test` as a dev dependency in the WASM crate. A test calls `preserve_metadata` with authored fixtures and checks that `bytes` is `js_sys::Uint8Array`, `report.issues` is an array, and missing `field` is `null` as generated TypeScript declares. Use `js_sys::Reflect` for these runtime assertions.

```rust
#[wasm_bindgen_test::wasm_bindgen_test]
fn metadata_response_uses_typed_bytes_and_nullable_fields() {
    use wasm_bindgen::JsCast;
    let request = r3sizer_metadata::MetadataExportRequest {
        source: b"unknown".to_vec(),
        encoded: include_bytes!("../../r3sizer-metadata/tests/fixtures/plain.png").to_vec(),
        facts: r3sizer_metadata::OutputFacts {
            width: 32, height: 16,
            orientation: r3sizer_metadata::OrientationAction::Normalize,
            color: r3sizer_metadata::ColorAction::Srgb,
        },
    };
    let result = r3sizer_wasm::preserve_metadata(
        serde_wasm_bindgen::to_value(&request).unwrap()).unwrap();
    let bytes = js_sys::Reflect::get(&result, &"bytes".into()).unwrap();
    assert!(bytes.is_instance_of::<js_sys::Uint8Array>());
    let report = js_sys::Reflect::get(&result, &"report".into()).unwrap();
    let issues = js_sys::Reflect::get(&report, &"issues".into()).unwrap();
    let first = js_sys::Array::from(&issues).get(0);
    assert!(js_sys::Reflect::get(&first, &"field".into()).unwrap().is_null());
}
```

- [ ] **Step 2:** Run `wasm-pack test --node crates/r3sizer-wasm`; expect the missing binding/response contract to fail. Mark the new integration test file `#![cfg(target_arch = "wasm32")]` so native workspace tests do not invoke JS functions.
- [ ] **Step 3: Implement the binding and type exporter.** Use the serializer's null setting without converting bytes to JSON arrays:

```rust
use serde::Serialize;
let request: r3sizer_metadata::MetadataExportRequest =
    serde_wasm_bindgen::from_value(request)
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
let limits = r3sizer_metadata::MetadataLimits::default();
let source = r3sizer_metadata::extract(&request.source, &limits);
let result = r3sizer_metadata::merge(request.encoded, &source, &request.facts, &limits);
let response = r3sizer_metadata::MetadataExportResponse {
    bytes: result.bytes, report: result.report,
};
response.serialize(&serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true))
    .map_err(|err| JsValue::from_str(&err.to_string()))
```

[`serde-wasm-bindgen` documents](https://docs.rs/serde-wasm-bindgen/latest/serde_wasm_bindgen/) the `serde_bytes`→`Uint8Array` mapping and the optional-null serializer setting. Keep arrays transferable after leaving WASM; do not serialize binary data through JSON.

Add core **dev** dependency `r3sizer-metadata = { path = "../r3sizer-metadata", version = "0.9", features = ["typegen"] }`. Extend the existing `declarations` list with `MetadataCategory`, `MetadataIssueReason`, `MetadataIssue`, `MetadataReport`, `OrientationAction`, `ColorAction`, `OutputFacts`, `MetadataExportRequest`, and `MetadataExportResponse`, importing them from the new crate. Update the generated header to name both Rust type sources. Re-export all nine generated types through `wasm-types.ts`; its existing export is explicit, so adding declarations alone is insufficient.

- [ ] **Step 4:** Run the unchanged core typegen command, `wasm-pack test --node crates/r3sizer-wasm`, `wasm-pack build --target web crates/r3sizer-wasm`, and `cargo tree -p r3sizer-wasm -e normal`. Confirm normal production dependencies exclude ts-rs and `r3sizer-io`. Run typegen twice and confirm the second run changes nothing.
- [ ] **Step 5:** Commit: `feat(wasm): expose metadata export with generated types`.

### Task 9: Bind processed output to its source and transport metadata exports

**Files:** Modify `web/src/shared/api/processing/{client,wasm,wasm-worker,index,ingest}.ts`, existing `client.test.ts` and `ingest.test.ts`, `web/src/entities/outputs/model/store.ts`, `web/src/entities/images/model/store.ts`, `web/src/features/image-processing/model/store.ts`. Create `web/src/shared/api/processing/metadata.ts`, `metadata.test.ts`, `web/src/features/image-processing/model/store.test.ts`.

**Consumes:** Generated `MetadataExportRequest`, `MetadataExportResponse`, `OutputFacts`, `MetadataReport`; WASM `preserve_metadata` binding.

**Produces:** `ProcessJob.sourceFile: File` captures the client input at job construction; `OutputResult.sourceFile: File` and `OutputState.outputSourceFile: File | null` retain that exact reference. Extend the existing worker envelope with `type: "metadata_export"`, `metadataRequest?: MetadataExportRequest`; response adds `type: "metadata_exported"`, `metadataResponse?: MetadataExportResponse`. The envelopes are existing web protocol wrappers; all new payload shapes come from Rust generation.

Public shared API:

```typescript
export function preserveEncodedMetadata(
  source: File, encoded: Blob, facts: OutputFacts,
): Promise<{ blob: Blob; report: MetadataReport }>;
// Internal wasm.ts worker call, also consumed by metadata.ts:
export function exportMetadata(request: MetadataExportRequest): Promise<MetadataExportResponse>;
```

The Blob wrapper is a browser-only return type, not a handwritten equivalent of a Rust boundary type.

- [ ] **Step 1: Extend client tests for source identity and replacement.** Existing tests already define `params()` and mock decoding/worker calls.

```typescript
it("captures the source file in the processing job", async () => {
  const file = new File(["source A"], "a.jpg");
  await client.decode(file);
  const job = client.process(params());
  expect(job.sourceFile).toBe(file);
  await job.promise;
});
```

Add a deferred-promise store test: start job A, change the image entity to B while A resolves, then assert A is either discarded as stale or committed with source A, never source B. When job B has started, A must not overwrite it. Include reset during processing. For overlapping decode A/B, whichever latest requested decode finishes last must remain the selected file; close discarded bitmaps.

- [ ] **Step 2:** Run `npm test -- src/shared/api/processing/client.test.ts src/features/image-processing/model/store.test.ts` from `web`; expect missing source identity or stale-result behavior to fail.
- [ ] **Step 3: Implement source capture and narrow lifecycle guards.** `JobImpl` sets a readonly `sourceFile` from its constructor input. Processing store captures params version at start, commits only if `currentJob === job`, and clears job/progress state only for the same active job. Use `job.sourceFile` in `setResult`. `clearOutput()` resets `outputSourceFile` too. Add decode request generation checks in client and image store; cancel a previous processing job before replacing its decoded bitmap/cache, and ignore stale decode completions/errors. Do not refactor unrelated parameter behavior.

```typescript
// Capture before awaiting the job; never read current input after it finishes.
const paramsVersion = useImageStore.getState().paramsVersion;
const job = processingClient.process(params);
currentJob = job;
const result = await job.promise;
if (currentJob !== job) return;
useOutputStore.getState().setResult({
  ...result, params, paramsVersion, sourceFile: job.sourceFile,
});
```

Make image orientation/color ingress explicit: `createImageBitmap(file, { imageOrientation: "from-image", premultiplyAlpha: "none", colorSpaceConversion: "default" })`; remove the cast for the nonstandard bitmap `colorSpace` field. All ingestion and export `2d` contexts request `{ colorSpace: "srgb" }`, preserving `willReadFrequently` where already used. Verify both striped and monolithic paths call those contexts. Browser tests below establish whether these facts are safe; unsupported context behavior must use `Unverified` rather than declaring an unverified conversion successful.

- [ ] **Step 4: Add worker roundtrip and fallback tests before implementation.** Mock `exportMetadata` at `metadata.ts`'s boundary; do not mock the public function under test.

```typescript
import { expect, it, vi } from "vitest";
vi.mock("./wasm", () => ({ exportMetadata: vi.fn() }));
import { exportMetadata } from "./wasm";
import { preserveEncodedMetadata } from "./metadata";

it("returns the original encoded blob with an issue on worker failure", async () => {
  vi.mocked(exportMetadata).mockRejectedValueOnce(new Error("worker failed"));
  const encoded = new Blob([new Uint8Array([1, 2, 3])], { type: "image/png" });
  const result = await preserveEncodedMetadata(new File(["src"], "a.jpg"), encoded,
    { width: 1, height: 1, orientation: "normalize", color: "srgb" });
  expect(result.blob).toBe(encoded);
  expect(result.report.issues).toContainEqual({
    category: "unknown", reason: "merge_failed", field: null,
  });
});
```

- [ ] **Step 5: Implement worker request/response handling and shared helper.** Reuse `ensureWorker`, request IDs, timeout and reset rejection handling; add resolution of `metadata_exported` responses. Transfer newly-created source/encoded ArrayBuffers to the worker and response bytes back. Never transfer/detach output pixels or a buffer kept in the entity store. The helper keeps the original Blob for fallback. Source File.size is checked before `arrayBuffer()`; above 256 MiB return original Blob with `Unknown/LimitExceeded`. Source read failure yields `Unknown/Unverified`. On timeout/reset/serialization failure return original Blob with `Unknown/MergeFailed`. Null canvas encoding is not handled here; it is an export error in Task 10.

Worker branch uses only generated payload types:

```typescript
const response = preserve_metadata(msg.metadataRequest!) as MetadataExportResponse;
(self as unknown as Worker).postMessage({
  type: "metadata_exported", id: msg.id, metadataResponse: response,
} satisfies WorkerResponse, [response.bytes.buffer]);
```

Export `preserveEncodedMetadata` through processing's barrel and thus `@/shared/api`. Do not expose raw worker internals to the export feature.

- [ ] **Step 6:** Run the affected Vitest tests and `npx tsc -b` after generating WASM package output. Test timeout/reset, source size limit without reading bytes, source read failure, binary response transfer, source identity, stale job suppression and existing large-image fallback. Commit: `feat(web): bind exports to source files and carry metadata through WASM`.

### Task 10: Integrate downloads and accessible metadata warnings

**Files:** Modify `web/src/features/export/ui/download-button.tsx`; create `web/src/features/export/model/{export-image.ts,export-image.test.ts}`, `web/src/features/export/ui/metadata-message.tsx`, `web/src/features/export/ui/metadata-message.test.ts`. Modify `web/public/locales/{en,ru}.json`.

**Consumes:** Output entity with `outputSourceFile`, export preferences, shared `preserveEncodedMetadata`, generated report/issue types.

**Produces:** Browser-local `ExportSnapshot` and `exportImage(snapshot: ExportSnapshot): Promise<{ blob: Blob; report: MetadataReport; filename: string }>` in `export-image.ts`. Snapshot fields: `sourceFile: File`, `rgba: Uint8Array`, `width: number`, `height: number`, `format: ExportFormat`, `quality: number`. Snapshot only references immutable result buffers and File; encoding may copy into canvas as the current implementation already does.

- [ ] **Step 1: Add export-model tests using a mocked canvas encoder and shared metadata helper.** Assert filename and source come from the snapshot, quality 90 maps to 0.9 for JPEG/WebP, PNG omits quality, and metadata is applied to the resulting blob before download creation. A null blob must reject without calling metadata or creating an object URL.

```typescript
import { expect, it, vi } from "vitest";
vi.mock("@/shared/api", () => ({ preserveEncodedMetadata: vi.fn() }));
import { preserveEncodedMetadata } from "@/shared/api";
import { exportImage } from "./export-image";

it("fails when the canvas cannot encode the output", async () => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    putImageData: vi.fn(),
  } as unknown as CanvasRenderingContext2D);
  vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation((callback) => callback(null));
  vi.stubGlobal("ImageData", class {
    constructor(public data: Uint8ClampedArray, public width: number, public height: number) {}
  });
  await expect(exportImage({
    sourceFile: new File(["src"], "original.jpg"), rgba: new Uint8Array(4),
    width: 1, height: 1, format: "png", quality: 90,
  })).rejects.toThrow("Image encoding failed");
  expect(preserveEncodedMetadata).not.toHaveBeenCalled();
});
```

Restore spies and globals with `afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); })`.

- [ ] **Step 2:** Run `npm test -- src/features/export/model/export-image.test.ts` from `web`; expect unresolved model or incorrect error handling.
- [ ] **Step 3: Move encoding orchestration into the model.** Preserve existing MIME/quality behavior. Verify the Blob MIME type actually matches the requested format (canvas may fall back); if it differs, fail with an export error instead of downloading bytes with the wrong extension. Canvas context failure is also an export error. Call metadata with final dimensions and verified browser orientation/color facts. Use the snapshot File stem for the filename. This is the core promise wrapper:

```typescript
const encoded = await new Promise<Blob>((resolve, reject) => {
  canvas.toBlob((blob) => {
    if (blob) resolve(blob);
    else reject(new Error("Image encoding failed"));
  }, mime, snapshot.format === "png" ? undefined : snapshot.quality / 100);
});
const result = await preserveEncodedMetadata(snapshot.sourceFile, encoded, {
  width: snapshot.width, height: snapshot.height,
  orientation: "normalize", color: "srgb",
});
```

Here `mime` is the existing PNG/JPEG/WebP mapping moved into the model; select `Unverified` color facts if actual context attributes cannot establish the requested sRGB context. Do not silently retry a different pixel export format.

`DownloadButton` captures one snapshot per click from output state, shows pending state, and guards duplicate clicks with a ref set synchronously before awaiting. On success download the returned Blob once, show report issues if present, and revoke the object URL after dispatch (defer cleanup to avoid premature browser revocation). On pixel encoding failure show an export error and do not download. If the selected image or output changes mid-export, invalidate that request's completion before download/message updates; reset pending state only for the matching request. Clear old messages on new source, new output, or new export; preserve current messages across ordinary re-renders. This invalidation uses component generation/ref state, not a new global export subsystem.

- [ ] **Step 4: Add localized report rendering.** `MetadataMessage` consumes `MetadataReport` and uses an always-visible `role="status"`/`aria-live="polite"` region near the download controls, with optional expandable field details for multiple issues. It must work below `lg`; do not put the warning in existing hidden desktop controls. Keep metadata values out of messages, render field identifiers as escaped React text, and deduplicate with shared report semantics. Avoid claiming the browser has saved a file to disk; say the download was prepared.

Copy to add under the existing download locale namespace:

```json
{
  "preparing": "Preparing download…",
  "metadataPartial": "Download prepared. Some original metadata could not be retained.",
  "metadataIssue": "{{category}}: {{reason}}",
  "metadataIssueField": "{{category}} ({{field}}): {{reason}}",
  "exportFailed": "Could not prepare the image download."
}
```

Add complete EN/RU labels keyed by all generated categories/reasons. Category labels: EXIF, XMP, IPTC, Color profile, Text, Thumbnail, Camera maker notes, Pixel density, Other metadata. Reason labels: Unsupported, Invalid metadata, Removed because it no longer matches the image, Preservation could not be verified, Metadata size limit exceeded, Metadata could not be added. Russian translations should convey the same distinctions and use the existing locale style. Do not expose enum identifiers as fallback user copy.

- [ ] **Step 5: Add rendering and lifecycle tests with React's existing `createRoot`/`act` (no new testing library).** Because Vitest includes `.test.ts`, use `createElement` in UI tests or deliberately extend the include list for `.test.tsx`. Verify the live region for partial metadata, no warning on an empty report, details for multiple categories, translated reason labels, disabled/pending button, duplicate clicks, source replacement during an awaited export, and new-export message clearing. Test that worker failure still triggers exactly one download with a warning while null canvas output triggers none. Run the export tests, `npx tsc -b`, `npm run lint`, and `npm run fsd`. Commit: `feat(web): preserve download metadata and display export warnings`.

### Task 11: Wire CI, document limits, and verify end-to-end behavior

**Files:** Modify `.github/workflows/ci.yml`, `README.md`, `CONTRIBUTING.md`, `CLAUDE.md`, `crates/r3sizer-io/README.md`, `crates/r3sizer-metadata/README.md`, `web/README.md`; add `docs/testing/metadata-export.md`. Inspect `web/Dockerfile` and `.github/workflows/deploy.yml`; change only inputs that actually exclude the new crate (the current Docker COPY already includes it).

**Consumes:** Completed Tasks 1–10 and their fixtures.

**Produces:** CI type freshness and WASM runtime coverage, user-facing limitations, and a recorded browser verification recipe with observed results supplied during execution.

- [ ] **Step 1: Add generation freshness to CI.** Add a Rust job or steps after native tests:

```yaml
- name: Regenerate shared TypeScript types
  run: cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture
- name: Check generated types are committed
  run: git diff --exit-code -- web/src/shared/lib/types/generated.ts
```

In the existing WASM job, add Node setup through the project's existing action/version convention if Node is not already provisioned, then `wasm-pack test --node crates/r3sizer-wasm` before its build. Existing workspace lint/test/doc jobs automatically cover the new crate. Do not create duplicate jobs for every format.

- [ ] **Step 2: Document the actual capability matrix and commands.** Update four-crate diagrams to five crates and show the core's metadata **dev-only** generation edge separately from production edges. Document default preservation, corrected fields, MakerNote/preview/extended-XMP limits, unsupported source/destination cases, bounds, profile uncertainty, stderr-only CLI reporting and no sweep JSON aggregation. Show additive library use:

```rust
let metadata_limits = r3sizer_io::MetadataLimits::default();
let loaded = r3sizer_io::load_with_metadata(
    input_path, &r3sizer_io::DecodeLimits::default(), &metadata_limits)?;
let output = r3sizer_core::process_auto_sharp_downscale(&loaded.image, &params)?;
let report = r3sizer_io::save_with_metadata(
    &output.image, output_path, &loaded, &metadata_limits)?;
```

Keep the unchanged typegen command in CLAUDE/CONTRIBUTING and explain that the exporter now includes the metadata crate's `types.rs`. Do not describe warnings as a complete inventory for unsupported formats.

- [ ] **Step 3: Run required native verification once after integration is complete.** Record actual outcomes, not anticipated counts:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture
git diff --exit-code -- web/src/shared/lib/types/generated.ts
wasm-pack test --node crates/r3sizer-wasm
```

New generated declarations should already be committed by Task 8. If a later change legitimately modifies them, regenerate and commit before using the freshness check. Do not repeatedly run the whole suite unless new changes or failures justify it.

- [ ] **Step 4: Run web verification from `web`.** `npm test`, `npm run lint`, `npm run fsd`, `npm run build`. Build includes the current WASM bundle, TypeScript checking and Vite. Run `docker build -f web/Dockerfile -t r3sizer-metadata-check .` from root if Docker is available; if unavailable, record that specific limitation and verify its COPY/typegen paths by inspection instead of claiming a Docker build passed.
- [ ] **Step 5: Verify real browser behavior in Chromium and Firefox.** Use authored fixtures with asymmetric color blocks and EXIF orientations 1–8. For each orientation confirm preview orientation, export pixels, and exported orientation 1; repeat through monolithic and striped ingestion (use the existing large-image generation tool or an authored >24 MP fixture for stripes). Export JPEG/PNG/WebP and inspect metadata with an independent reader. Confirm non-sRGB profile sources produce accurate loss/uncertainty messages and are not silently tagged with the original profile after sRGB conversion. Check the actual canvas context attributes. Exercise no metadata, malformed metadata, unsupported source metadata, narrow viewport warning visibility, worker failure, repeated clicks, input replacement mid-export and pixel encoding failure. No server upload should occur. Browser behavior that contradicts output facts must be corrected or downgraded to an explicit unverified issue before completion.
- [ ] **Step 6: Self-review against spec and commit the integration documentation/CI changes.** Inspect `git diff --check`, production dependency graph, actual format fixtures, and remaining modified files. Commit: `test: verify metadata preservation across native and browser exports`. Include relevant validation outcomes in the eventual PR description; creating/publishing a PR is not part of this planning turn.

## Dependency order and acceptance map

Execute Tasks 1–5 in order. Task 6 depends on 5, Task 7 on 6. Task 8 depends on 5, Task 9 on 8, Task 10 on 9. Task 11 closes both integrations. If using agents, native integration (6–7) and WASM/web integration (8–10) can be assigned independently only after the crate contracts are settled; coordinate manifest/lockfile edits explicitly.

| Spec requirement | Implementation and verification |
| --- | --- |
| Separate crate, versions, CI graph | 1, 8, 11 |
| Bounds, malformed metadata, fallback | 1–5, 6, 9 |
| EXIF/GPS/camera/date/copyright | 2, 3, 5 |
| XMP/IPTC/text | 2, 4, 5 |
| Dimensions, orientation, profiles, thumbnails | 3–6, 9, 11 |
| JPEG/PNG/WebP and unsupported-format warnings | 2, 5–7, 10 |
| Native pixel-only API compatibility | 6 |
| CLI stderr only; unchanged diagnostics/sweep summaries | 7 |
| Generated issue/category/binary boundary types | 8, 11 |
| Browser source association, both ingestion paths | 9, 11 |
| Visible localized messages and export failure distinctions | 9, 10, 11 |
| No companion files or new core algorithm dependency | 1, 6–11 |

This document is an implementation plan. No implementation, test pass, browser result, or dependency compatibility result is claimed until the corresponding task is executed.
