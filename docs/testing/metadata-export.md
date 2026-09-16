# Metadata preservation: verification recipe and observed results

This records how embedded-metadata preservation (native CLI and web export)
was actually exercised end-to-end, and what was observed — not an
anticipated checklist. Passing items below have command output or browser
evidence behind them; anything not actually exercised is listed as a named
limitation, not a pass.

## Environment

- Native: `cargo` stable toolchain, workspace at the commit adding this file.
- Browser automation: no `puppeteer`/`playwright` available in this
  environment. Chromium was driven directly over the Chrome DevTools
  Protocol (raw WebSocket, ~150-line Python client) and Firefox over
  WebDriver BiDi (Firefox 149's `--remote-debugging-port` speaks BiDi, not
  CDP). Both scripts live under a scratch directory for this session, not
  committed to the repo.
- Chromium: `Chrome/153.0.8010.36`, headless (`--headless=new`).
- Firefox: `149.0.2`, headless (`--headless`).
- Web app served via `npx vite preview` against the production build
  (`npm run build` output), not the dev server.
- Fixtures: authored with Pillow (`ImageOps.exif_transpose` inverse
  transforms) and ImageMagick, generated fresh into a scratch directory —
  not checked into the repo. Canonical image: 300×200, four distinct-color
  quadrants (red/green/blue/yellow — top-left/top-right/bottom-left/
  bottom-right), which fully disambiguates all 8 EXIF orientation values
  (no two orientations produce the same on-screen quadrant arrangement).
  For each orientation tag, the fixture's raw pixels are the mathematical
  inverse of the display transform, self-checked by round-tripping through
  `ImageOps.exif_transpose` before use (mean abs. pixel diff ≤ 5/255,
  accounting for JPEG quantization). A non-sRGB ICC fixture uses the
  system's Adobe RGB (`a98.icc`, ghostscript's copy, 564 bytes). A >24 MP
  fixture (6400×4000, 25.6 MP) exercises the striped-ingestion path.

## Native verification (Step 3 of the task-11 brief)

Run once, in order, from the repository root:

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Pass (after formatting the metadata crate and fixing the follow-on line-length from the clippy fix) |
| `cargo test --workspace` | Pass — 517 tests across all crates, 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass (after removing the two redundant `&` references in `r3sizer-io` examples) |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | Pass |
| `cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture` | Pass — regenerated `generated.ts` |
| `git diff --exit-code -- web/src/shared/lib/types/generated.ts` | Pass — 0 diff (already committed and fresh) |
| `wasm-pack test --node crates/r3sizer-wasm` | Pass — 1 test (`metadata_response_uses_typed_bytes_and_nullable_fields`) |

### Native fixture spot-check (release CLI, `r3sizer process`)

Beyond the workspace test suite, the release CLI binary was run directly
against the authored fixtures to observe real byte-level outcomes (not just
unit-test assertions):

- `orient-1.jpg`, `orient-6.jpg` (EXIF orientation 1 and 6) → resized
  output's EXIF orientation tag reads back as `1` and `6` respectively
  (verified with Pillow), dimensions corrected to the actual output size,
  **no** stderr warning (orientation preserved cleanly).
- `non-srgb-adobergb.jpg` (source tagged Adobe RGB) → stderr:
  `warning: <path>: metadata icc/unverified (icc)`; the output file has
  **no** ICC profile at all (verified with Pillow) — the unverifiable
  source profile is dropped, not silently kept or mismatched.
- `malformed-exif.jpg` (a JPEG with a deliberately corrupt EXIF/TIFF header
  spliced in right after SOI) → stderr:
  `warning: <path>: metadata exif/malformed (ifd_header)`; the output
  decodes cleanly with **no** EXIF block at all (Pillow's `getexif()` is
  empty) — corrupted input degrades to an omitted block, not a crash and
  not corrupted bytes propagated downstream.
- `unsupported.bmp` (BMP source, outside the three merge-supported formats)
  → stderr: `warning: <path>: metadata unknown/unverified`; pixels still
  process and encode normally.
- `r3sizer sweep --in-dir <fixtures>` **without** `--out-dir` → **zero**
  stderr warning lines, confirming a no-output sweep emits no preservation
  warnings.
- `r3sizer sweep --in-dir <fixtures> --out-dir <dir> --summary summary.json`
  → per-file stderr warnings appear (e.g. `metadata exif/malformed
  (ifd_header)` for the malformed fixture; at the time of this run JPEG→PNG
  conversions also warned `metadata density/unsupported (jfif)`, which no
  longer happens for aspect-ratio-only JFIF (`units = 0`) sources since that
  carries no physical density), and `summary.json`'s keys were
  enumerated recursively — **no** `metadata` key anywhere in it, confirming
  metadata issues never reach the JSON summary.

## Web verification (Step 4)

Run from `web/`:

| Command | Result |
|---|---|
| `npm test` | Pass — 8 files, 64 tests |
| `npm run lint` | 0 errors, 42 warnings (pre-existing baseline, unchanged) |
| `npm run fsd` | Pass — "No problems found!" |
| `npm run build` | Pass — `build:wasm` (wasm-pack + wasm-opt, installed locally via `cargo install wasm-opt --locked`) → `tsc -b` → `vite build`; only pre-existing warning is the >500kB chunk-size advisory, unrelated to metadata |

`docker build -f web/Dockerfile -t r3sizer-metadata-check .` (from the repo
root): **succeeded**, exit code 0. All three stages completed (`wasm-builder`
on `rust:1.94-bullseye`, `web-builder` on `node:24-alpine` running
`tsc -b && vite build`, `runtime` on `nginx:1.29-alpine`); image built and
tagged. This is partial evidence toward the workspace's `rust-version =
"1.87"` MSRV claim (the Docker stage uses 1.94, a newer toolchain) — a real
`cargo +1.87 check -p r3sizer-metadata --lib` was not run, since no 1.87
toolchain was installed in this environment and none was installed for this
check per instructions. **Not independently verified**: the exact MSRV of
1.87 for the new `r3sizer-metadata` crate.

## Browser verification (Step 5)

All of the following were driven against the built `web/dist` app served by
`vite preview`, via real Chromium (headless) unless noted.

### Canvas context attributes (both browsers)

Directly evaluated in-page: `document.createElement('canvas')
.getContext('2d', {colorSpace:'srgb'}).getContextAttributes().colorSpace`
returned `"srgb"` in **both** Chromium 153 and Firefox 149. This confirms
the app's `color: "unverified"` fallback in `export-image.ts` (used when
this check does *not* return `"srgb"`) is a real conditional, not dead code
that always takes one branch in practice — and that on the browsers tested,
export legitimately reports verified sRGB rather than falling back.

### Full ingest → process → export round trip (Chromium)

Driven through the real UI (file input via `DOM.setFileInputFiles`, the
"Process" button, then "Save", with `Browser.setDownloadBehavior` capturing
the actual downloaded bytes). All 15 cases completed and were independently
re-opened with Pillow and ImageMagick's `identify`:

| Case | Result |
|---|---|
| EXIF orientation 1–8 (JPEG source, JPEG export) | All 8: exported EXIF orientation reads back as `1` (confirmed via Pillow and, for orientation 6, ImageMagick `identify` reporting `Orientation: TopLeft`); the canonical red/green/blue/yellow quadrant layout is reproduced pixel-for-pixel (via `ImageOps.exif_transpose`) in every case. Preview dimensions and export pixels both reflect the already-corrected orientation, matching monolithic ingestion decoding with `imageOrientation: "from-image"`. |
| Format coverage: JPEG/PNG/WebP export of the same source | All three decode correctly and reproduce the canonical quadrant layout. |
| Non-sRGB ICC source (Adobe RGB) | Export shows the visible warning `Color profile (icc): Preservation could not be verified`. The output JPEG **does** carry an ICC profile, but it is a genuine 456-byte sRGB profile (confirmed via `ImageCms.getProfileDescription` → `"sRGB"`, and byte-for-byte different from the 564-byte source Adobe RGB profile) — this is the browser's own canvas JPEG encoder tagging its actually-sRGB pixel output, not the merge logic resurrecting or mismatching the original profile. The original Adobe RGB tag is correctly dropped and reported, never silently kept. |
| No-metadata PNG source | No warning shown (`status_message: None`); output has no ICC/EXIF, matching "verified absence needs no warning." |
| Malformed EXIF source | Warning `EXIF (ifd_header): Invalid metadata` shown; output decodes cleanly with **no** EXIF block (empty `getexif()`) — no crash, no corrupted bytes carried through. |
| Unsupported source format (BMP) | Warning `Other metadata: Preservation could not be verified` shown; pixels still export correctly. |
| >24 MP fixture (25.6 MP, striped ingestion path) | Processed and exported successfully; output reproduces the canonical quadrant layout at the resized output dimensions. |

### Interaction checks (Chromium)

- **Narrow viewport (380px, mobile emulation)**: the `role="status"` export
  warning is present, `display: block`, non-zero size, and does not
  overflow the 380px viewport — visible, not hidden behind an `lg:`/`xl:`
  breakpoint as the code intends.
- **Repeated clicks**: two synchronous clicks on the Save button (fired
  back-to-back with no round-trip delay) produced exactly **one** download —
  the `disabled`/pending guard held under a true double-click, not just a
  slow one.
- **Input replacement mid-export**: replacing the source file ~50 ms after
  clicking Save (before the export promise resolves) produced **zero**
  downloads from the stale request and the app remained fully responsive
  (file input still present/functional afterward) — the stale-generation
  check in `download-button.tsx` discarded the in-flight result rather than
  downloading a mismatched pairing of old pixels with new source metadata,
  or crashing.
- **No metadata / malformed metadata / unsupported source metadata**:
  covered above as part of the round-trip table.

### Not verified in this environment

- **Live worker-crash simulation**: attempted via `Target.setDiscoverTargets`
  + `Target.closeTarget` against the dedicated processing Web Worker, to
  force `exportMetadata`'s RPC to reject and observe the `merge_failed`
  fallback path in `preserveEncodedMetadata` actually engage from a real
  crash (rather than from a unit test's mocked rejection). The dedicated
  worker never surfaced as a discoverable CDP target in this headless setup
  (`Target.targetCreated` for `type: "worker"` was never observed), so the
  kill could not be applied. The fallback *logic* itself is covered by the
  existing unit test suite (`metadata.test.ts`, exercised by `npm test`
  above, e.g. "reports an unreadable source as unverified"), but a live
  browser crash-and-recover was not observed here. Treat this as an
  explicit gap, not a pass.
- **Pixel encoding failure**: forcing `canvas.toBlob`/`OffscreenCanvas`
  encoding to fail in a real browser (as opposed to a mocked unit test)
  requires either instrumenting the page (out of scope — no application
  code changes for verification purposes) or a browser/driver capability not
  available here. Not exercised live; not claimed as passing.
- **Full Firefox ingest→export matrix**: Firefox 149's remote debugging
  surface is WebDriver BiDi, not CDP. The BiDi `input.setFiles` command
  (needed to drive a real `<input type="file">` without a user gesture) was
  not implemented in the time available for this task — only the direct
  canvas-`colorSpace` check and a bare page-load check were run against
  Firefox (both passed, see above). The full upload → process → export →
  verify matrix above ran on Chromium only.
- **Server upload check**: no network request other than the initial static
  asset fetches (`vite preview`) and the WASM binary was observed in the
  course of driving these tests; no dedicated network-trace instrumentation
  (e.g. asserting zero `XMLHttpRequest`/`fetch` calls during export) was
  added. Consistent with a fully client-side pipeline, but not exhaustively
  instrumented.
- **`+1.87` MSRV check for `r3sizer-metadata`**: see the Docker paragraph
  above — only partial evidence (a newer 1.94 toolchain building
  successfully), no toolchain-pinned check was run.

## Reproducing this locally

The fixture generator and CDP/BiDi driver scripts used for this task were
written into a scratch directory for this session and are not part of the
repository (per "no companion files" / no test-infrastructure scope creep
for this task). To redo this verification:

1. Build fixtures with Pillow: a landscape canonical image with four
   distinct-color quadrants, saved 8 times with EXIF orientation tags 1–8
   using the *inverse* of `PIL.ImageOps.exif_transpose`'s per-tag transform,
   plus a non-sRGB ICC source (any real, non-sRGB `.icc` file works — e.g.
   `ghostscript`'s bundled `a98.icc` on most Linux systems) and a >24 MP
   fixture for the striped path.
2. `cd web && npm run build && npx vite preview --port 4173`.
3. Drive the app with any CDP-capable client (Puppeteer/Playwright, if
   available, are much less code than raw WebSocket JSON-RPC): open the
   preview URL, set the hidden `<input type="file">`'s files via
   `DOM.setFileInputFiles`, click the "Process" button, then the "Save"
   button (title starts with `"Save as"`), and read back the file Chrome
   saves via `Browser.setDownloadBehavior`.
4. Verify the result with an independent reader (Pillow, ImageMagick
   `identify`, or `exiftool` if available) — never trust the app's own
   report as the only signal that metadata round-tripped correctly.
