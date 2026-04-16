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
| `--preset` | | — | Named preset: `photo` (default), `precision` |
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
