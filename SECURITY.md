# Security Policy

## Threat model

r3sizer processes raster image files provided by the calling application.
The primary attack surface is the image decode path in `r3sizer-io`.

**In-scope risks:**

- **Maliciously crafted image files** — oversized dimensions, invalid headers,
  corrupt pixel data, or format-specific decoder bugs in the `image` crate
  dependency.  Mitigated by `DecodeLimits` (default cap: 100 MP, 16 384 px per
  axis) applied before the full pixel buffer is allocated.  See
  `r3sizer_io::load_as_linear_with_limits`.

- **Arithmetic overflow** — all pixel dimensions and buffer lengths are checked
  against `DecodeLimits` before any allocation.  Integer arithmetic on
  dimensions uses explicit casts with prior limit validation.

**Out-of-scope (by design):**

- Network access — `r3sizer-core` and `r3sizer-io` perform no network I/O.
  The web UI is a separate browser-side application (threat model differs).

- Side-channel timing attacks — not a concern for image processing libraries.

## Supported versions

The project is pre-1.0.  Only the latest commit on `main` is actively supported.

## Reporting a vulnerability

To report a security issue, please e-mail **alvy.tsk@gmail.com** with:

- A description of the vulnerability and its potential impact.
- Reproduction steps or a minimal proof-of-concept (a test image is preferred
  over a full exploit chain).

Please **do not** open a public GitHub issue for security vulnerabilities.

Expected response: acknowledgement within 7 days, disclosure timeline agreed
within 14 days.  We will credit reporters in the fix commit unless you prefer
to remain anonymous.
