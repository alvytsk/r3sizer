# A validated, stamped probe-worker seam — design

Date: 2026-11-25
Scope: medium change (core `pipeline`, `r3sizer-wasm` bindings, web probe-pool
glue). Closes #42.

## Intent and why

The parallel-probing seam (`PreparedBase`, probe workers,
`process_from_prepared_with_probes`) relies on `debug_assert!`s and on the web
client's discipline for three invariants. Release and WASM builds compile the
assertions out.

Measured on `main` at `8dad1da` with a throwaway harness (32×24 base,
Lightness mode, strengths 0.5 / 1.0 / 2.0):

| Worker input                | Debug build                  | Release build                                             |
|-----------------------------|------------------------------|-----------------------------------------------------------|
| `detail` at ¾ length        | panics on `debug_assert!`    | `Ok`; samples read 0.380 / 0.498 / 0.500, not 0.507 / 0.664 / 0.667 |
| `detail` at ½ or ¼ length   | panics on `debug_assert!`    | `Ok`; wrong by a further step each time                   |
| `base_luminance` too short  | panics (index out of bounds) | panics, untyped                                           |
| `base_luminance` too long   | panics on `debug_assert!`    | `Ok`; the result happens to be right                      |

A wrong-length buffer yields plausible samples, a wrong s*, and no error.

Nothing else at the seam is checked. `process_from_prepared_with_probes` fits
externally collected samples without evidence that they were measured on the
base being finished or with the params being finished. `BaseParamsKey`
hand-copies ten of the 21 `AutoSharpParams` fields (nine key fields; `target`
holds width and height), and nothing notices a new base-affecting field.

A fourth gap turned up while measuring the others: worker probes ignore
`evaluation_color_space`. With `LumaOnly` set, in-process probes read
`0.471 / 0.500 / 0.500 / 0.508 / 0.938` where worker probes read what they
read with the option off (`0.507 / 0.664 / 0.667 / 0.667 / 0.668`). The web UI
does not expose the option, so only direct API users see it.

## Goals

- Every buffer entering a probe run has the length its sharpen mode implies,
  or the call returns `CoreError::BufferLengthMismatch`, in every build.
- Pixels, luminance, detail, baseline and the settings a probe reads travel as
  one value that cannot be assembled wrongly by position.
- `process_from_prepared_with_probes` refuses samples not stamped for the
  `PreparedBase` instance and the params it is given.
- A new `AutoSharpParams` field cannot be added without being classified as
  base-affecting or not.
- In-process and worker probes measure the same thing for the same inputs.

## Non-goals

- A worker-side wasm cache of the base. Per-batch copies stay as they are;
  that belongs with #51.
- The `two_phase` example, whose `Fast` iteration fails its own
  `matches_params` assertion. Separate issue.
- `process_from_prepared*` not calling `validate()` (carried over from #57).
- Forgery resistance. The stamp is an equality check between two values this
  crate minted, not a signature.

## Approaches considered

1. **Validate lengths only.** Rejected: a stale batch would still fit silently
   and the positional raw-buffer entry points would stay.
2. **Typed input and stamped batch.** Chosen. Closes every failure the ticket
   names without taking on #51.
3. **(2) plus a worker-side wasm cache** (`set_probe_base` once, then
   `probe(strengths)`). Rejected for this ticket: a larger wasm and JS rewrite,
   and it removes the per-batch base copies, which is #51's work. Its extra
   binding only guards against a client that starts overlapping jobs, which
   this repo's client does not do.

## The change

### Core (`crates/r3sizer-core/src/pipeline.rs`)

**`ProbeSettings`** (new, public, `Copy + Eq + Serialize + Deserialize`,
private fields) is exactly what a probe reads from params:
`sharpen_sigma` and the four `metric_weights` as `f32::to_bits`, plus
`sharpen_mode`, `metric_mode`, `artifact_metric`, `selection_policy` and
`evaluation_color_space`. `ProbeSettings::from_params` is the only
constructor.

**`ProbeProvenance`** (new, public, `Copy + Eq + Serialize + Deserialize`,
private fields) is `base_id`, `baseline_artifact_ratio` (bits) and
`settings`. `PreparedBase::probe_provenance(&self, &params)` is the only
constructor.

**`PreparedBase`** gains a `base_id` taken from a process-wide counter when
`prepare_base` builds it. The fast path in the wasm `prepare_base` keeps the
instance, so the id is stable for the life of a cached base. Any
re-preparation is a new instance.

**`ProbeWorkerParts<'a>`** (new, public) is the raw buffers with named
fields: `base`, `base_luminance`, `detail`, `baseline_artifact_ratio`.
Nothing accepts it except `ProbeWorkerInput::new`.

**`ProbeWorkerInput<'a>`** (new, public, private fields) has one constructor,
`new(parts, &params, provenance) -> Result<Self, CoreError>`. It checks:

- `base_luminance.len() == W × H`, in both sharpen modes;
- `detail.len() == W × H` in Lightness mode, `W × H × 3` in Rgb mode;
- `provenance.settings == ProbeSettings::from_params(params)`;
- `provenance`'s baseline equals `parts.baseline_artifact_ratio`.

Lengths are exact, the same policy `finish_pipeline` already applies to the
final sharpen. `run_probes(&self, strengths) -> ProbeBatch` takes no params.

`PreparedBase::probe_input(&self, detail, &params) -> Result<ProbeWorkerInput>`
builds one from the base itself and mints the provenance. Only `detail` is
caller-supplied, and it is length-checked.

**`ProbeBatch`** (new, public, `Serialize + Deserialize`) is a `provenance` and
the `samples`. `ProbeBatch::new(provenance, samples)` is public, so a caller
that measures probes its own way can stamp them.

**`process_from_prepared_with_probes`** takes a `ProbeBatch` in place of
`Vec<ProbeSample>`. Before anything else it checks
`batch.provenance == prepared.probe_provenance(params)` and otherwise returns
`CoreError::ProbeProvenanceMismatch` (new variant).

**One measurement path.** `ProbeContext.metric_override` is a `dyn Fn` that
the in-process path sets from `evaluation_color_space` and the worker path
left as `None`. It becomes a plain `Option<EvaluationColorSpace>` field, so
both paths call the same measurement. In-process results do not change.
Worker probes start honoring `evaluation_color_space`.

**Removed:** the free functions `run_probes_standalone`,
`run_probes_from_detail` and `compute_probe_detail`. `ProbeWorkerInput::new` is
the single raw entry. `PreparedBase::compute_detail` stays;
`PreparedBase::run_probes` returns `Result<ProbeBatch>`.

**`BaseParamsKey`** is unchanged. Two tests cover it:

1. A compile-time classification. A test destructures all 21 fields of
   `AutoSharpParams` without `..` and feeds every binding into one of two
   role arrays. A new field fails the test build until it is listed: the
   destructure rejects the pattern, and a binding left out of both arrays is
   an unused-variable error under `-D warnings`.
2. The table-driven test from the issue, extended to every field: toggling
   one field at a time makes `matches_params` return `false` for the ten
   base-affecting fields and `true` for the other eleven.

### WASM (`crates/r3sizer-wasm/src/lib.rs`)

- `get_probe_bundle(params_json)` replaces `get_base_data` and
  `compute_probe_detail`. It returns `basePixels`, `luminance`, `width`,
  `height`, `baseline`, `effectiveP0`, `detail` and `provenanceJson`, minted
  in one call so the base, the detail and the stamp cannot come from
  different moments.
- `probe_batch_with_detail` gains a `provenance_json` argument and returns
  batch JSON instead of a bare samples array.
- `process_from_probes(params_json, batch_json, probing_us,
  pass_diagnostics_json)` takes the batch JSON.
- `CACHED_DETAIL` is keyed by `ProbeProvenance` instead of
  `(sigma bits, sharpen mode)`.
- The exports `get_base_data`, `compute_probe_detail` and `probe_batch` are
  removed. `r3sizer-wasm` is `publish = false` and its only consumer is this
  web app.

### Web (`web/src/shared/api/processing/`)

- `wasm-worker.ts`: the `get_base_data` and `compute_detail` messages become
  one `get_probe_bundle` message.
- `probe-pool.ts`: `BaseData` carries `paramsJson` and `provenanceJson`.
  `set_base` sends them with the buffers, so a worker's cache is one record.
  A `probe` message carries strengths only. `runProbesParallel(strengths)`
  returns a batch JSON.
- `mergeProbeBatches(batchJsons)` is a pure function in `probe-pool.ts`. It
  rejects batches whose provenance differs, then concatenates, sorts and
  deduplicates the samples.
- `probe-worker.ts`: the cached record includes the params and provenance,
  which it passes to `probe_batch_with_detail`.
- `wasm.ts`: `runParallelPipeline` follows the new messages and sends the
  merged batch to `process_from_probes`.

A rejected batch makes `process_from_probes` throw, and `processImageParallel`
already falls back to the in-process path for any error except cancellation.

### Docs

`CHANGELOG.md` (Fixed and Changed under Unreleased), `CLAUDE.md`,
`docs/architecture.md`, `docs/future_work.md`.

## What the stamp does and does not guarantee

It guarantees that a batch accepted by `process_from_prepared_with_probes`
was produced by a `ProbeWorkerInput` built for this `PreparedBase` instance,
with this baseline and these probe settings, or by a caller holding that
instance's provenance.

It does not guarantee these cases:

- At the single wasm call site, `luminance` and `detail` are both `W × H`
  floats in Lightness mode and could be swapped. A node parity test covering
  that call site is the guard.
- An identical re-preparation gets a new `base_id`. Samples from the previous
  instance are then rejected and the web client falls back to the in-process
  path: slower, never wrong.

## Left to the builder

- JSON field names beyond those listed above.
- Message type names on the web side.
- Test images, and the strengths that make each parity case discriminate.
