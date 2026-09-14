#![cfg(target_arch = "wasm32")]

#[wasm_bindgen_test::wasm_bindgen_test]
fn metadata_response_uses_typed_bytes_and_nullable_fields() {
    use wasm_bindgen::JsCast;
    let request = r3sizer_metadata::MetadataExportRequest {
        source: b"unknown".to_vec(),
        encoded: include_bytes!("../../r3sizer-metadata/tests/fixtures/plain.png").to_vec(),
        facts: r3sizer_metadata::OutputFacts {
            width: 32,
            height: 16,
            orientation: r3sizer_metadata::OrientationAction::Normalize,
            color: r3sizer_metadata::ColorAction::Srgb,
        },
    };
    let result = r3sizer_wasm::preserve_metadata(serde_wasm_bindgen::to_value(&request).unwrap())
        .unwrap();
    let bytes = js_sys::Reflect::get(&result, &"bytes".into()).unwrap();
    assert!(bytes.is_instance_of::<js_sys::Uint8Array>());
    let report = js_sys::Reflect::get(&result, &"report".into()).unwrap();
    let issues = js_sys::Reflect::get(&report, &"issues".into()).unwrap();
    let first = js_sys::Array::from(&issues).get(0);
    assert!(js_sys::Reflect::get(&first, &"field".into()).unwrap().is_null());
}
