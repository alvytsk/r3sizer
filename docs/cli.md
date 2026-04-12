# CLI Reference

## Installation

```sh
cargo build --release -p r3sizer-cli
```

The binary is written to `./target/release/r3sizer`.

---

## Single-file mode

```sh
r3sizer --input photo.jpg --output out.png --width 800 --height 600
```

Both `--width` and `--height` are required unless `--preserve-aspect-ratio` is set,
in which case only one is needed:

```sh
r3sizer -i photo.jpg -o out.png --width 800 -p
```

### Diagnostics

Add `--diagnostics` to write a JSON file with full pipeline telemetry:

```sh
r3sizer -i photo.jpg -o out.png --width 800 --height 600 --diagnostics diag.json
```

Use `--diagnostics-level full` for per-probe breakdowns.

---

## Sweep mode

Process a directory of images and produce an aggregate summary:

```sh
r3sizer \
  --sweep-dir ./photos \
  --sweep-output-dir ./out \
  --sweep-summary summary.json \
  --width 800 --height 600
```

The summary JSON includes per-file results (selected strength, selection mode, timing)
and aggregate statistics (mean/median strength, fit success rate, selection mode histogram).

Compare two sweep runs:

```sh
r3sizer --sweep-diff baseline.json,candidate.json
```

---

## All flags

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--input` | `-i` | required | Input image path |
| `--output` | `-o` | required | Output image path |
| `--width` | `-W` | — | Target width (px) |
| `--height` | `-H` | — | Target height (px) |
| `--preserve-aspect-ratio` | `-p` | off | Compute the missing dimension from the input aspect ratio |
| `--target-artifact-ratio` | | `0.003` | P0 threshold (fraction, not percent) |
| `--preset` | | — | Named preset: `photo` (default), `precision` |
| `--diagnostics` | | — | Path to write a JSON diagnostics file |
| `--diagnostics-level` | | `summary` | `summary` or `full` (per-probe breakdowns) |
| `--probe-strengths` | | two-pass | Comma-separated explicit probe list |
| `--sharpen-sigma` | | `1.0` | Gaussian sigma for unsharp mask |
| `--sharpen-mode` | | `lightness` | `lightness` (CIE Y) or `rgb` |
| `--metric-mode` | | `relative` | `relative` (sharpening-added) or `absolute` (total) |
| `--artifact-metric` | | `channel-clipping` | `channel-clipping` or `pixel-out-of-gamut` |
| `--metric-weights` | | `1.0,0.3,0.3,0.1` | Composite weights: gamut, halo, overshoot, texture |
| `--selection-policy` | | `gamut-only` | `gamut-only`, `hybrid`, or `composite-only` |
| `--enable-contrast-leveling` | | off | Enable contrast leveling stage (placeholder) |
| `--sweep-dir` | | — | Directory of images to process in batch mode |
| `--sweep-output-dir` | | — | Output directory for processed images (sweep mode) |
| `--sweep-summary` | | — | Path to write sweep summary JSON |
| `--sweep-diff` | | — | Compare two sweep summaries: `BASE,CANDIDATE` |
| `--generate-corpus` | | — | Generate synthetic benchmark corpus in directory |
