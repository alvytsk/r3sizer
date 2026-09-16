# r3sizer-io

[![Crates.io](https://img.shields.io/crates/v/r3sizer-io.svg)](https://crates.io/crates/r3sizer-io)
[![Docs.rs](https://docs.rs/r3sizer-io/badge.svg)](https://docs.rs/r3sizer-io)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/alvytsk/r3sizer/blob/main/LICENSE)

Thin image I/O layer for [`r3sizer-core`](https://crates.io/crates/r3sizer-core).
Loads a file from disk into a `LinearRgbImage` (applying the sRGB → linear transfer) and writes one back (applying linear → sRGB). Format is inferred from the extension.

Use this when you want the r3sizer pipeline to just *work* on files without wiring up the [`image`](https://crates.io/crates/image) crate yourself.

## Install

```toml
[dependencies]
r3sizer-core = "0.8"
r3sizer-io   = "0.8"
```

## Usage

```rust
use std::path::Path;
use r3sizer_core::prelude::*;
use r3sizer_io::{load_as_linear, save_from_linear};

let src = load_as_linear(Path::new("photo.jpg"))?;
let params = AutoSharpParams::photo(800, 600).resolved();
let result = process_auto_sharp_downscale(&src, &params)?;
save_from_linear(&result.image, Path::new("out.png"))?;
```

### With embedded metadata preserved

`load_with_metadata` / `save_with_metadata` are additive — same decoded pixels
as above, plus the source's embedded metadata (EXIF, XMP, IPTC, ICC, text,
density) carried into the output via [`r3sizer-metadata`](https://crates.io/crates/r3sizer-metadata),
for JPEG/PNG/WebP. `save_with_metadata` returns a `MetadataReport` listing
anything that couldn't be safely preserved (not an error — the image is
still written):

```rust
let metadata_limits = r3sizer_io::MetadataLimits::default();
let loaded = r3sizer_io::load_with_metadata(
    Path::new("photo.jpg"), &r3sizer_io::DecodeLimits::default(), &metadata_limits)?;
let result = process_auto_sharp_downscale(&loaded.image, &params)?;
let report = r3sizer_io::save_with_metadata(
    &result.image, Path::new("out.png"), &loaded, &metadata_limits)?;
```

## Supported formats

PNG, JPEG, GIF, BMP, TIFF, WebP — all via the `image` crate with the matching feature flags enabled by default.

Metadata preservation (`*_with_metadata`) covers JPEG, PNG, and WebP only —
other formats decode/encode pixels normally but carry no metadata over. Not
every metadata preservation is guaranteed even for the covered formats; see
[`r3sizer-metadata`](../r3sizer-metadata/README.md) for the exact limits and
what's never preserved (MakerNote, embedded previews/thumbnails, extended
XMP).

## Examples

The crate ships runnable examples under [`crates/r3sizer-io/examples/`](https://github.com/alvytsk/r3sizer/tree/main/crates/r3sizer-io/examples):

| Example           | What it shows                                                    |
|-------------------|------------------------------------------------------------------|
| `single_file.rs`  | Basic load → process → save in ~40 lines.                        |
| `two_phase.rs`    | `prepare_base` once, then run Fast / Balanced / Quality modes.   |
| `custom_params.rs`| Build `AutoSharpParams` by hand, compare Uniform vs ContentAdaptive. |

Run them with:

```sh
cargo run --example single_file --manifest-path crates/r3sizer-io/Cargo.toml \
    -- photo.jpg out.png 800 600
```

## Color space

Files are decoded as 8-bit sRGB, then linearized with the IEC 61966-2-1 transfer function into `f32` linear-RGB. This is the colorspace `r3sizer-core` operates in: all resizing and sharpening happen in linear light, which is what makes the artifact metric physically meaningful.

## License

MIT — see [LICENSE](https://github.com/alvytsk/r3sizer/blob/main/LICENSE).
