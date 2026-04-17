# r3sizer

[![Crates.io](https://img.shields.io/crates/v/r3sizer.svg)](https://crates.io/crates/r3sizer)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/alvytsk/r3sizer/blob/main/LICENSE)

**Command-line image downscaler with automatic sharpness correction.**

Resize images without losing sharpness. r3sizer probes several sharpening strengths, fits a cubic model of the artifact level, and solves for the exact strength that hits your quality budget — per image, no manual tweaking.

For the full project (library crates, WASM build, web UI) see the [repository](https://github.com/alvytsk/r3sizer). A browser-based demo is at [alvytsk.github.io/r3sizer](https://alvytsk.github.io/r3sizer/).

## Install

```sh
cargo install r3sizer
```

## Usage

Single image:

```sh
r3sizer process -i photo.jpg -o out.png --width 800 --height 600
```

Preserve aspect ratio (only one dimension required):

```sh
r3sizer process -i photo.jpg -o out.png --width 800 --preserve-aspect-ratio
```

Emit a full diagnostic report of pipeline decisions:

```sh
r3sizer process -i photo.jpg -o out.png --width 800 --height 600 --diagnostics diag.json
```

Batch a directory:

```sh
r3sizer sweep --in-dir ./photos --out-dir ./out --summary summary.json --width 800 --height 600
```

## Subcommands

| Command            | Purpose                                                             |
|--------------------|---------------------------------------------------------------------|
| `r3sizer process`  | Process a single image.                                             |
| `r3sizer sweep`    | Batch-process a directory, writes an aggregate JSON summary.        |
| `r3sizer diff`     | Compute pixel-level and metric-level differences between two images.|
| `r3sizer corpus`   | Run a labeled test corpus and report per-class statistics.          |
| `r3sizer presets`  | List or show built-in parameter presets (photo, art, screenshot…).  |

Run `r3sizer <subcommand> --help` for the full flag list, or see the [CLI reference](https://github.com/alvytsk/r3sizer/blob/main/docs/cli.md).

## Runtime modes

Every processing command accepts `--mode fast | balanced | quality` (default: `balanced`):

- **fast** — fewer probes, uniform sharpening, no chroma guard.
- **balanced** — the default; full adaptive pipeline with sensible probe budget.
- **quality** — extended probing and guaranteed full adaptive pipeline.

## How it works

See the repository [`docs/algorithm.md`](https://github.com/alvytsk/r3sizer/blob/main/docs/algorithm.md) for the full pipeline description. TL;DR: linearize → downscale → classify regions → probe sharpening strengths → fit cubic P(s) → solve P(s\*) = P0 → apply final sharpen → chroma guard → output.

## License

MIT — see [LICENSE](https://github.com/alvytsk/r3sizer/blob/main/LICENSE).
