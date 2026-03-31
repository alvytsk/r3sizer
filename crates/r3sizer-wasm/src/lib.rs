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

/// Take the cached prepared base if target dimensions match the given params.
fn take_matching_base(params: &AutoSharpParams) -> Option<r3sizer_core::PreparedBase> {
    CACHED_BASE.with(|c| {
        let cache = c.borrow();
        match cache.as_ref() {
            Some(b) if b.target.width == params.target_width
                    && b.target.height == params.target_height => {}
            _ => return None,
        }
        drop(cache);
        c.borrow_mut().take()
    })
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
