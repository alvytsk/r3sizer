# CLI Reference

## Installation

```sh
cargo build --release -p r3sizer
```

The binary is written to `./target/release/r3sizer`.

---

## Subcommands

```
r3sizer process   -i <in> -o <out> [options]
r3sizer sweep     --in-dir <dir> [options]
r3sizer diff      <baseline.json> <candidate.json>
r3sizer corpus    <output-dir>
r3sizer presets   list
r3sizer presets   show <name>
```

---

## `process` — Single-file mode

```sh
r3sizer process -i photo.jpg -o out.png --width 800 --height 600
```

Both `--width` and `--height` are required unless `--preserve-aspect-ratio` (`-p`) is set,
in which case only one is needed:

```sh
r3sizer process -i photo.jpg -o out.png --width 800 -p
```

### Diagnostics

Add `--diagnostics` to write a JSON file with full pipeline telemetry:

```sh
r3sizer process -i photo.jpg -o out.png --width 800 --height 600 --diagnostics diag.json
```

Use `--diagnostics-level full` for per-probe breakdowns.

### Structured JSON output

Use `--output-format json` to emit the diagnostics summary as JSON on stdout instead of
the human-readable text format:

```sh
r3sizer process -i photo.jpg -o out.png --width 800 --height 600 --output-format json
```

### Metadata preservation

`process` and `sweep` carry EXIF/XMP/IPTC/ICC metadata from the source file
into the output image where the destination format supports it (JPEG, PNG,
WebP). Anything that can't be safely carried over — a dropped `MakerNote`,
a stale embedded thumbnail, an unverified color tag, a destination format
outside JPEG/PNG/WebP, or a metadata block that failed to parse — is
reported as a warning on **stderr only**, one line per distinct issue:

```
warning: <output path>: metadata <category>/<reason> (<field>)
```

The `(<field>)` suffix is omitted when the issue has no associated field
name. Categories/reasons are the same identifiers `r3sizer-metadata`
reports internally (e.g. `maker_note/unverified`, `exif/removed_stale`,
`icc/merge_failed`) — never the tag's actual value. These warnings never
appear in stdout, `--diagnostics` JSON, or (for `sweep`) `summary.json`;
they don't affect the exit code or `sweep`'s success/failure counts.

---

## `sweep` — Batch mode

Process a directory of images and produce an aggregate summary:

```sh
r3sizer sweep \
  --in-dir ./photos \
  --out-dir ./out \
  --summary summary.json \
  --width 800 --height 600
```

The summary JSON includes per-file results (selected strength, selection mode, timing)
and aggregate statistics (mean/median strength, fit success rate, selection mode histogram).

Metadata warnings (see above) are only emitted when `--out-dir` is set, one warning line
per affected output file as it's written; a sweep with no `--out-dir` writes no images and
so emits none. Warnings never enter `summary.json` and never count toward `aggregate.failed`.

---

## `diff` — Compare sweep summaries

```sh
r3sizer diff baseline.json candidate.json
```

---

## `corpus` — Generate synthetic benchmark images

```sh
r3sizer corpus ./corpus-dir
```

Generates 8 deterministic test images covering smooth gradients, step edges,
high-frequency texture, color bars, concentric circles, thin lines, noise, and
mixed-region content.

---

## `presets` — List and inspect presets

```sh
r3sizer presets list
r3sizer presets show photo
```

---

## All `process` / `sweep` flags

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--input` | `-i` | required | Input image path (`process` only) |
| `--output` | `-o` | required | Output image path (`process` only) |
| `--in-dir` | | required | Input directory (`sweep` only) |
| `--out-dir` | | — | Output directory (`sweep` only) |
| `--summary` | | — | Sweep summary JSON path (`sweep` only) |
| `--width` | `-W` | — | Target width (px) |
| `--height` | `-H` | — | Target height (px) |
| `--preserve-aspect-ratio` | `-p` | off | Compute missing dimension from input aspect ratio |
| `--target-artifact-ratio` | | `0.003` | P0 threshold (fraction, not percent) |
| `--preset` | | — | Named preset. Stable: `photo` (default), `precision`. Legacy: `baseline`, `v3-adaptive`, `v5-full`, `v5-two-pass` |
| `--diagnostics` | | — | Path to write JSON diagnostics (`process` only) |
| `--diagnostics-level` | | `summary` | `summary` or `full` (per-probe breakdowns) |
| `--output-format` | | `text` | `text` or `json` (`process` only) |
| `--probe-strengths` | | two-pass | Comma-separated explicit probe list |
| `--sharpen-sigma` | | `1.0` | Gaussian sigma for unsharp mask |
| `--sharpen-mode` | | `lightness` | `lightness` (CIE Y) or `rgb` |
| `--metric-mode` | | `relative` | `relative` (sharpening-added) or `absolute` (total) |
| `--artifact-metric` | | `channel-clipping` | `channel-clipping` or `pixel-out-of-gamut` |
| `--metric-weights` | | `1.0,0.3,0.3,0.1` | Composite weights: gamut, halo, overshoot, texture |
| `--selection-policy` | | `gamut-only` | `gamut-only`, `hybrid`, or `composite-only` |
| `--enable-contrast-leveling` | | off | Enable contrast leveling stage (placeholder) |
| `--mode` | | `balanced` | Performance-quality tradeoff: `fast`, `balanced`, `quality` |
| `--max-pixels` | | `100000000` | Reject inputs whose width x height exceeds this count |
| `--max-dimension` | | `16384` | Reject inputs whose width or height exceeds this value (px) |
