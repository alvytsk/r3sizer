//! WASM binding for extracting source metadata and merging it into an
//! already-encoded output image. Stateless: does not touch the processing
//! caches in `lib.rs`.

use serde::Serialize;
use wasm_bindgen::prelude::*;

/// Extract metadata from `request.source` and merge it into `request.encoded`.
///
/// Invalid boundary arguments (malformed `JsValue`) are returned as `Err`.
/// Metadata parse/merge issues found along the way are not errors: they are
/// reported in the successful response's `report` field.
#[wasm_bindgen]
pub fn preserve_metadata(request: JsValue) -> Result<JsValue, JsValue> {
    let request: r3sizer_metadata::MetadataExportRequest = serde_wasm_bindgen::from_value(request)
        .map_err(|err| JsValue::from_str(&err.to_string()))?;

    let limits = r3sizer_metadata::MetadataLimits::default();
    let source = r3sizer_metadata::extract(&request.source, &limits);
    let result = r3sizer_metadata::merge(request.encoded, &source, &request.facts, &limits);

    let response = r3sizer_metadata::MetadataExportResponse {
        bytes: result.bytes,
        report: result.report,
    };

    response
        .serialize(&serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true))
        .map_err(|err| JsValue::from_str(&err.to_string()))
}
