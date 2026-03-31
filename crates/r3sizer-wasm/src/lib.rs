mod convert;

use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use r3sizer_core::{AutoSharpParams, LinearRgbImage, process_auto_sharp_downscale_with_progress};

// ---------------------------------------------------------------------------
// Cached linear image — avoids re-converting sRGB→linear on every process call.
// ---------------------------------------------------------------------------

thread_local! {
    static CACHED_INPUT: RefCell<Option<LinearRgbImage>> = const { RefCell::new(None) };
    static CACHED_BASE: RefCell<Option<r3sizer_core::PreparedBase>> = const { RefCell::new(None) };
}

/// Pre-convert sRGB RGBA pixels to linear RGB and cache the result.
///
/// Call this when the user loads an image.  The cached image is consumed by the
/// next `process_image` call that matches in dimensions, avoiding a redundant
/// colour-space conversion.
#[wasm_bindgen]
pub fn prepare_image(
    srgb_rgba_data: &[u8],
    width: u32,
    height: u32,
) -> Result<(), JsValue> {
    let input = convert::rgba_u8_to_linear(srgb_rgba_data, width, height)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    CACHED_INPUT.with(|c| *c.borrow_mut() = Some(input));
    Ok(())
}

/// Pre-compute the base image (resize + classify + baseline + evaluator).
///
/// Call this after `prepare_image` with the current params JSON.  The result
/// is cached and reused by the next `process_image` call, cutting ~1.5 s of
/// perceived processing time from the "Process" button click.
///
/// If the user changes target dimensions or strategy, call this again.
#[wasm_bindgen]
pub fn prepare_base(
    srgb_rgba_data: &[u8],
    width: u32,
    height: u32,
    params_json: &str,
) -> Result<(), JsValue> {
    let params: AutoSharpParams = serde_json::from_str(params_json)
        .map_err(|e| JsValue::from_str(&format!("invalid params JSON: {e}")))?;

    let input = get_or_convert_input(srgb_rgba_data, width, height)?;

    let prepared = r3sizer_core::prepare_base(&input, &params, &post_progress)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    CACHED_BASE.with(|c| *c.borrow_mut() = Some(prepared));
    CACHED_INPUT.with(|c| *c.borrow_mut() = Some(input));

    post_progress("base_ready");
    Ok(())
}

/// Drop the cached linear image and prepared base.
#[wasm_bindgen]
pub fn clear_cache() {
    CACHED_INPUT.with(|c| *c.borrow_mut() = None);
    CACHED_BASE.with(|c| *c.borrow_mut() = None);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the cached linear image if dimensions match, otherwise convert fresh.
fn get_or_convert_input(
    srgb_rgba_data: &[u8],
    width: u32,
    height: u32,
) -> Result<LinearRgbImage, JsValue> {
    let cached = CACHED_INPUT.with(|c| {
        let mut cache = c.borrow_mut();
        match cache.as_ref() {
            Some(img) if img.width() == width && img.height() == height => cache.take(),
            _ => None,
        }
    });
    match cached {
        Some(img) => Ok(img),
        None => {
            post_progress("converting");
            convert::rgba_u8_to_linear(srgb_rgba_data, width, height)
                .map_err(|e| JsValue::from_str(&e.to_string()))
        }
    }
}

/// Take the cached prepared base if all base-affecting params match.
///
/// Checks target dimensions, strategy, contrast, classification, color space,
/// artifact metric, evaluator, and target artifact ratio — not just dimensions.
fn take_matching_base(params: &AutoSharpParams) -> Option<r3sizer_core::PreparedBase> {
    CACHED_BASE.with(|c| {
        let cache = c.borrow();
        match cache.as_ref() {
            Some(b) if b.matches_params(params) => {}
            _ => return None,
        }
        drop(cache);
        c.borrow_mut().take()
    })
}

/// Encode a pipeline output as a JS object with imageData, dimensions, and diagnostics.
fn serialize_output(output: r3sizer_core::ProcessOutput) -> Result<JsValue, JsValue> {
    post_progress("encoding");

    let out_width = output.image.width();
    let out_height = output.image.height();
    let rgba_bytes = convert::linear_to_rgba_u8(&output.image);

    let result = js_sys::Object::new();
    let image_data = js_sys::Uint8Array::from(rgba_bytes.as_slice());
    js_sys::Reflect::set(&result, &"imageData".into(), &image_data)?;
    js_sys::Reflect::set(&result, &"outputWidth".into(), &JsValue::from(out_width))?;
    js_sys::Reflect::set(&result, &"outputHeight".into(), &JsValue::from(out_height))?;

    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    let diagnostics = serde::Serialize::serialize(&output.diagnostics, &serializer)
        .map_err(|e| JsValue::from_str(&format!("diagnostics serialization failed: {e}")))?;
    js_sys::Reflect::set(&result, &"diagnostics".into(), &diagnostics)?;

    Ok(result.into())
}

fn post_progress(stage: &str) {
    let global = js_sys::global();
    if let Ok(func) = js_sys::Reflect::get(&global, &"postMessage".into()) {
        if let Ok(func) = func.dyn_into::<js_sys::Function>() {
            let msg = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&msg, &"type".into(), &"progress".into());
            let _ = js_sys::Reflect::set(&msg, &"stage".into(), &JsValue::from_str(stage));
            let _ = func.call1(&global, &msg);
        }
    }
}

/// Process an image through the automatic sharpness-adjusted downscale pipeline.
///
/// # Arguments
///
/// * `srgb_rgba_data` — Flat RGBA u8 pixel array from canvas `getImageData()`
/// * `width` — Source image width in pixels
/// * `height` — Source image height in pixels
/// * `params_json` — JSON-serialized `AutoSharpParams`
///
/// # Returns
///
/// A JS object with:
/// - `imageData`: `Uint8Array` of sRGB RGBA u8 pixels (output image)
/// - `outputWidth`: output width
/// - `outputHeight`: output height
/// - `diagnostics`: full diagnostics object
#[wasm_bindgen]
pub fn process_image(
    srgb_rgba_data: &[u8],
    width: u32,
    height: u32,
    params_json: &str,
) -> Result<JsValue, JsValue> {
    let params: AutoSharpParams = serde_json::from_str(params_json)
        .map_err(|e| JsValue::from_str(&format!("invalid params JSON: {e}")))?;

    let input = get_or_convert_input(srgb_rgba_data, width, height)?;

    // Use cached PreparedBase if available and dimensions match.
    // This skips resize + classify + baseline + evaluator (~1.5 s on large images).
    let cached_base = take_matching_base(&params);

    let output = match cached_base.as_ref() {
        Some(prepared) => {
            r3sizer_core::process_from_prepared(prepared, &params, &post_progress)
                .map_err(|e| JsValue::from_str(&e.to_string()))?
        }
        None => {
            process_auto_sharp_downscale_with_progress(&input, &params, &post_progress)
                .map_err(|e| JsValue::from_str(&e.to_string()))?
        }
    };

    // Re-cache for subsequent calls.
    CACHED_INPUT.with(|c| *c.borrow_mut() = Some(input));
    if let Some(base) = cached_base {
        CACHED_BASE.with(|c| *c.borrow_mut() = Some(base));
    }

    serialize_output(output)
}

// ---------------------------------------------------------------------------
// Parallel probing API — used by the probe worker pool
// ---------------------------------------------------------------------------

/// Extract cached base image data for sending to probe workers.
///
/// Returns a JS object with `basePixels` (Float32Array), `luminance` (Float32Array),
/// `width`, `height`, `baseline`, `effectiveP0`, or null if no base is cached.
#[wasm_bindgen]
pub fn get_base_data() -> JsValue {
    CACHED_BASE.with(|c| {
        let cache = c.borrow();
        let prepared = match cache.as_ref() {
            Some(b) => b,
            None => return JsValue::NULL,
        };

        let result = js_sys::Object::new();
        let base_px = js_sys::Float32Array::from(prepared.base_pixels());
        let luma = match prepared.luminance() {
            Some(l) => js_sys::Float32Array::from(l),
            None => return JsValue::NULL,
        };

        let _ = js_sys::Reflect::set(&result, &"basePixels".into(), &base_px);
        let _ = js_sys::Reflect::set(&result, &"luminance".into(), &luma);
        let _ = js_sys::Reflect::set(&result, &"width".into(), &JsValue::from(prepared.base_width()));
        let _ = js_sys::Reflect::set(&result, &"height".into(), &JsValue::from(prepared.base_height()));
        let _ = js_sys::Reflect::set(&result, &"baseline".into(), &JsValue::from(prepared.baseline_artifact_ratio()));
        let _ = js_sys::Reflect::set(&result, &"effectiveP0".into(), &JsValue::from(prepared.effective_p0()));

        result.into()
    })
}

/// Run probes on raw base image data (for use in dedicated probe workers).
///
/// Each probe worker receives the base pixels + luminance via postMessage and
/// calls this with its assigned strengths.  Returns a JSON array of ProbeSamples.
#[wasm_bindgen]
pub fn probe_batch(
    base_pixels: &[f32],
    width: u32,
    height: u32,
    luminance: &[f32],
    strengths_json: &str,
    params_json: &str,
    baseline: f32,
) -> Result<String, JsValue> {
    let params: AutoSharpParams = serde_json::from_str(params_json)
        .map_err(|e| JsValue::from_str(&format!("invalid params JSON: {e}")))?;
    let strengths: Vec<f32> = serde_json::from_str(strengths_json)
        .map_err(|e| JsValue::from_str(&format!("invalid strengths JSON: {e}")))?;

    let samples = r3sizer_core::run_probes_standalone(
        base_pixels, width, height, luminance, &strengths, &params, baseline,
    ).map_err(|e| JsValue::from_str(&e.to_string()))?;

    serde_json::to_string(&samples)
        .map_err(|e| JsValue::from_str(&format!("serialization failed: {e}")))
}

/// Finish processing using the cached PreparedBase and externally-collected probes.
///
/// Call this in the main worker after collecting probe results from the pool.
/// `pass_diagnostics_json` is optional JSON for `ProbePassDiagnostics` (from
/// TwoPass resolution).  Pass an empty string or `"null"` to omit.
#[wasm_bindgen]
pub fn process_from_probes(
    params_json: &str,
    probes_json: &str,
    probing_us: u32,
    pass_diagnostics_json: &str,
) -> Result<JsValue, JsValue> {
    let params: AutoSharpParams = serde_json::from_str(params_json)
        .map_err(|e| JsValue::from_str(&format!("invalid params JSON: {e}")))?;
    let probe_samples: Vec<r3sizer_core::ProbeSample> = serde_json::from_str(probes_json)
        .map_err(|e| JsValue::from_str(&format!("invalid probes JSON: {e}")))?;
    let pass_diagnostics: Option<r3sizer_core::ProbePassDiagnostics> =
        if pass_diagnostics_json.is_empty() || pass_diagnostics_json == "null" {
            None
        } else {
            Some(serde_json::from_str(pass_diagnostics_json)
                .map_err(|e| JsValue::from_str(&format!("invalid pass diagnostics JSON: {e}")))?)
        };

    // Must have a cached PreparedBase.
    let cached_base = take_matching_base(&params);
    let prepared = cached_base.as_ref()
        .ok_or_else(|| JsValue::from_str("no cached PreparedBase — call prepare_base first"))?;

    let output = r3sizer_core::process_from_prepared_with_probes(
        prepared, &params, probe_samples, probing_us as u64, pass_diagnostics, &post_progress,
    ).map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Re-cache base only (input is not needed for this path).
    if let Some(base) = cached_base {
        CACHED_BASE.with(|c| *c.borrow_mut() = Some(base));
    }

    serialize_output(output)
}

// ---------------------------------------------------------------------------
// Probe strength resolution — for TwoPass parallel probing from JS
// ---------------------------------------------------------------------------

/// Resolve the initial (coarse) probe strengths for the current config.
///
/// For TwoPass, returns the coarse-pass linspace.
/// For Explicit or Range, returns all strengths.
/// Returns a JSON array of f32.
#[wasm_bindgen]
pub fn resolve_initial_strengths(params_json: &str) -> Result<String, JsValue> {
    let params: AutoSharpParams = serde_json::from_str(params_json)
        .map_err(|e| JsValue::from_str(&format!("invalid params JSON: {e}")))?;
    let strengths = r3sizer_core::resolve_initial_strengths(&params)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_json::to_string(&strengths)
        .map_err(|e| JsValue::from_str(&format!("serialization failed: {e}")))
}

/// Resolve the dense (second-pass) probe strengths from coarse results.
///
/// Returns a JSON object `{ "strengths": [...], "diagnostics": {...} }` for
/// TwoPass configs, or `null` for Explicit/Range (no second pass needed).
#[wasm_bindgen]
pub fn resolve_dense_strengths(
    coarse_samples_json: &str,
    params_json: &str,
    effective_p0: f32,
) -> Result<JsValue, JsValue> {
    let params: AutoSharpParams = serde_json::from_str(params_json)
        .map_err(|e| JsValue::from_str(&format!("invalid params JSON: {e}")))?;
    let coarse_samples: Vec<r3sizer_core::ProbeSample> = serde_json::from_str(coarse_samples_json)
        .map_err(|e| JsValue::from_str(&format!("invalid samples JSON: {e}")))?;

    match r3sizer_core::resolve_dense_strengths(&coarse_samples, &params, effective_p0)
        .map_err(|e| JsValue::from_str(&e.to_string()))?
    {
        Some((strengths, diag)) => {
            let result = serde_json::json!({
                "strengths": strengths,
                "diagnostics": diag,
            });
            Ok(JsValue::from_str(&result.to_string()))
        }
        None => Ok(JsValue::NULL),
    }
}
