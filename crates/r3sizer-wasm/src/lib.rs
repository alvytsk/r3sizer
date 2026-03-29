mod convert;

use wasm_bindgen::prelude::*;
use r3sizer_core::{AutoSharpParams, process_auto_sharp_downscale};

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

    let input = convert::rgba_u8_to_linear(srgb_rgba_data, width, height)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    let output = process_auto_sharp_downscale(&input, &params)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

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
