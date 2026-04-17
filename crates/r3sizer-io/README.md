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

## Supported formats

PNG, JPEG, GIF, BMP, TIFF, WebP — all via the `image` crate with the matching feature flags enabled by default.

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
