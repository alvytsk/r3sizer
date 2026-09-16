# Fixture provenance

This crate uses no binary image files as test fixtures. Every JPEG, PNG, and
WebP byte sequence exercised by `tests/containers.rs` and by the
module-local `#[cfg(test)]` blocks in `src/containers/{jpeg,png,webp}.rs`
and `src/iptc.rs` is synthesized programmatically, byte-by-byte, from the
format specifications (ITU-T T.81 for JPEG, the PNG spec, the RIFF/WebP
container spec, and Adobe's Photoshop IRB / IPTC IIM references).

Rationale:

- Hand-authoring the exact byte sequences (rather than loading real photos)
  gives precise control over edge cases this task must cover: corrupted
  CRCs, truncated segments/chunks, out-of-order or duplicate ICC segments,
  decompression-bomb-shaped zTXt/iCCP payloads, odd-size WebP chunks, and
  so on. These are difficult or impossible to reliably reproduce by editing
  a real image file.
- It guarantees no personal GPS data, camera serial numbers, or copyrighted
  photograph content ever enters the repository — every pixel-adjacent
  field (entropy-coded scan bytes, IDAT, VP8 data) is a placeholder that is
  never decoded, only skipped by length.
- All EXIF/XMP/IPTC text content in fixtures (e.g. `"Fixture Author"`,
  `"MM\0*fake tiff"`) is invented placeholder text with no relation to any
  real person, place, or device.

No fixture in this crate was downloaded, copied, or derived from an
external image file.

## Binary fixtures (Task 5: merge/roundtrip)

`plain.{jpg,png,webp}` and `metadata.{jpg,png,webp}` (plus `expected.json`)
are the only *binary* fixtures in this crate, used by `tests/roundtrip.rs`
to exercise `merge()` against real encoder output (byte-splicing metadata
into a container is one thing; making sure a real third-party encoder's
output survives it is another). They are committed, generated once, and
never rewritten by the normal test run.

Regenerate them with:

```sh
cargo test -p r3sizer-metadata --test gen_fixtures -- --ignored --nocapture
```

That command (`tests/gen_fixtures.rs`) is the authoritative recipe:

1. A 32x16 `image::RgbImage` (a plain two-tone color block, no photograph)
   is encoded to JPEG/PNG/WebP bytes using the `image` crate's own
   encoders directly -- this is `plain.{jpg,png,webp}`.
2. A little-endian classic TIFF block is hand-built byte-by-byte (own code,
   not `src/exif`) carrying: `Artist` = "Fixture Author", `Copyright` =
   "Fixture Copyright", `ImageDescription` = "Metadata fixture", an
   `ExifIFD` with `DateTimeOriginal` = "2024:01:02 03:04:05", and a
   `GPSIFD` for 1 deg 2 min 3 sec N, 4 deg 5 min 6 sec E.
3. That TIFF block is spliced into each `plain.*` file's bytes by hand to
   produce `metadata.*`:
   - JPEG: an APP1 segment (`FF E1`, big-endian length `2 + 6 + tiff.len()`,
     payload `Exif\0\0` + TIFF) inserted right after SOI (after the
     APP0/JFIF segment, if the encoder wrote one, since JFIF must stay the
     first marker).
   - PNG: an `eXIf` chunk (length + type + data + CRC32) inserted
     immediately before the first `IDAT` chunk, since PNG requires `eXIf`
     to precede image data.
   - WebP: the plain VP8L "simple" file is upgraded to the "extended"
     (VP8X) form: a `VP8X` chunk (flags byte `0x08` for EXIF only -- this
     image has no alpha and isn't animated) becomes the first chunk, the
     original VP8L chunk follows unchanged, and an `EXIF` chunk is
     appended; the outer RIFF size field is recomputed.
4. Every `metadata.*` file is independently read back with `kamadak-exif`
   (a real, independent EXIF reader, not this crate's own) to confirm
   `Artist` is actually present and correct before anything is written to
   disk.
5. `expected.json` records the fixture dimensions and every injected
   EXIF/GPS value as plain strings, so `tests/roundtrip.rs` can assert
   against one source of truth instead of repeating literals.

If ExifTool (`exiftool`) is installed, running it against `metadata.jpg`
independently (`exiftool tests/fixtures/metadata.jpg`) is a useful sanity
check during fixture preparation, but it is not a runtime dependency of
this crate and no test depends on its presence.

## `metadata-with-makernote.jpg` (Task 7: CLI stderr-warning regression test)

A second, independent binary fixture used only by
`crates/r3sizer/tests/metadata_cli.rs` to exercise the CLI's
stderr-only metadata-loss warning end to end against a real dropped
tag. Same 32x16 plain-pixel JPEG block as `plain.jpg`, with a
hand-built TIFF block spliced in via the same `inject_jpeg_exif`
helper, carrying: `Artist` = "Fixture Author" (same authorship
convention as the fixtures above) in IFD0, and an `ExifIFD` holding a
single `MakerNote` (tag `0x927c`, type UNDEFINED, 20 bytes of `0xAB`
placeholder data) -- the exact shape `src/exif/patch.rs`'s
`maker_note_is_removed` unit test exercises, and the shape Task 3's
`correct()` is expected to drop with a `MakerNote`/`Unverified` issue.

Regenerate it (independently of the other fixtures, which stay
untouched) with:

```sh
cargo test -p r3sizer-metadata --test gen_fixtures generate_makernote_fixture -- --ignored --nocapture
```

That test (`generate_makernote_fixture` in `tests/gen_fixtures.rs`)
independently re-reads the freshly written bytes with `kamadak-exif`
and asserts both `Artist` and `MakerNote` are actually present in the
*source* fixture before anything is committed -- confirming the CLI
test's premise (Task 3 removes it) is exercising a MakerNote that was
really there to begin with.
