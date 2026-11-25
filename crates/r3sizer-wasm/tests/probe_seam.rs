#![cfg(target_arch = "wasm32")]

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;

/// 8 px dark/light stripes: sharpening overshoot clips the red channel while
/// luminance stays in range, so wrong buffers and color spaces show up.
fn stripe_rgba(w: u32, h: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..h {
        for x in 0..w {
            let v = if (x / 8) % 2 == 0 { 235u8 } else { 8 };
            rgba.extend_from_slice(&[v, v / 2, v / 5, 255]);
        }
    }
    rgba
}

fn params_json(sigma: f32) -> String {
    let mut params = r3sizer_core::AutoSharpParams::photo(32, 24);
    params.sharpen_sigma = sigma;
    serde_json::to_string(&params).unwrap()
}

/// What `get_probe_bundle` minted, as plain Rust values.
struct Bundle {
    base_pixels: Vec<f32>,
    luminance: Vec<f32>,
    detail: Vec<f32>,
    width: u32,
    height: u32,
    baseline: f32,
    effective_p0: f32,
    provenance_json: String,
}

fn floats(obj: &JsValue, key: &str) -> Vec<f32> {
    js_sys::Reflect::get(obj, &key.into())
        .unwrap()
        .dyn_into::<js_sys::Float32Array>()
        .unwrap()
        .to_vec()
}

fn bundle(params_json: &str) -> Bundle {
    let v = r3sizer_wasm::get_probe_bundle(params_json).unwrap();
    let number = |key: &str| {
        js_sys::Reflect::get(&v, &key.into())
            .unwrap()
            .as_f64()
            .unwrap() as f32
    };
    Bundle {
        base_pixels: floats(&v, "basePixels"),
        luminance: floats(&v, "luminance"),
        detail: floats(&v, "detail"),
        width: number("width") as u32,
        height: number("height") as u32,
        baseline: number("baseline"),
        effective_p0: number("effectiveP0"),
        provenance_json: js_sys::Reflect::get(&v, &"provenanceJson".into())
            .unwrap()
            .as_string()
            .unwrap(),
    }
}

/// One worker batch: the bundle's buffers in the binding's argument order.
fn probe(
    b: &Bundle,
    params_json: &str,
    strengths: &[f32],
    baseline: f32,
    provenance: &str,
) -> Result<String, JsValue> {
    r3sizer_wasm::probe_batch_with_detail(
        &b.base_pixels,
        b.width,
        b.height,
        &b.luminance,
        &b.detail,
        &serde_json::to_string(strengths).unwrap(),
        params_json,
        baseline,
        provenance,
    )
}

/// The web client's merge: one stamp, samples sorted by strength.
fn merge(batches: &[String]) -> String {
    let mut merged: serde_json::Value = serde_json::from_str(&batches[0]).unwrap();
    for other in &batches[1..] {
        let v: serde_json::Value = serde_json::from_str(other).unwrap();
        assert_eq!(v["provenance"], merged["provenance"]);
        merged["samples"]
            .as_array_mut()
            .unwrap()
            .extend(v["samples"].as_array().unwrap().iter().cloned());
    }
    merged["samples"].as_array_mut().unwrap().sort_by(|a, b| {
        a["strength"]
            .as_f64()
            .unwrap()
            .total_cmp(&b["strength"].as_f64().unwrap())
    });
    merged.to_string()
}

/// `(strength bits, metric_value bits)` for every probe sample in a result.
fn probe_metrics(result: &JsValue) -> Vec<(u32, u32)> {
    let diagnostics = js_sys::Reflect::get(result, &"diagnostics".into()).unwrap();
    let samples = js_sys::Reflect::get(&diagnostics, &"probe_samples".into()).unwrap();
    js_sys::Array::from(&samples)
        .iter()
        .map(|s| {
            let strength = js_sys::Reflect::get(&s, &"strength".into())
                .unwrap()
                .as_f64()
                .unwrap() as f32;
            let value = js_sys::Reflect::get(&s, &"metric_value".into())
                .unwrap()
                .as_f64()
                .unwrap() as f32;
            (strength.to_bits(), value.to_bits())
        })
        .collect()
}

#[wasm_bindgen_test]
fn probes_through_the_wasm_boundary_match_the_in_process_pipeline() {
    r3sizer_wasm::clear_cache();
    let rgba = stripe_rgba(128, 96);
    let params = params_json(1.0);
    r3sizer_wasm::prepare_base(&rgba, 128, 96, &params).unwrap();
    let b = bundle(&params);
    assert_eq!(
        (b.width, b.height, b.luminance.len(), b.detail.len()),
        (32, 24, 32 * 24, 32 * 24)
    );

    // Coarse round, dense window, dense round, merge: the web client's sequence.
    let coarse_strengths: Vec<f32> =
        serde_json::from_str(&r3sizer_wasm::resolve_initial_strengths(&params).unwrap()).unwrap();
    let coarse = probe(
        &b,
        &params,
        &coarse_strengths,
        b.baseline,
        &b.provenance_json,
    )
    .unwrap();
    let coarse_samples = serde_json::to_string(
        &serde_json::from_str::<serde_json::Value>(&coarse).unwrap()["samples"],
    )
    .unwrap();
    // The export returns its result as a JSON string; the web parses it too.
    let dense =
        r3sizer_wasm::resolve_dense_strengths(&coarse_samples, &params, b.effective_p0).unwrap();
    let dense: serde_json::Value = serde_json::from_str(&dense.as_string().unwrap()).unwrap();
    let dense_strengths: Vec<f32> = serde_json::from_value(dense["strengths"].clone()).unwrap();
    let dense_batch = probe(
        &b,
        &params,
        &dense_strengths,
        b.baseline,
        &b.provenance_json,
    )
    .unwrap();
    let via_workers =
        r3sizer_wasm::process_from_probes(&params, &merge(&[coarse, dense_batch]), 0, "").unwrap();

    // The web path probes every coarse strength; the in-process path may stop
    // early.  Every in-process sample must appear, bit for bit, in the batch.
    let in_process = r3sizer_wasm::process_image(&rgba, 128, 96, &params).unwrap();
    let (via, inproc) = (probe_metrics(&via_workers), probe_metrics(&in_process));
    assert!(
        via.len() >= inproc.len(),
        "{} via workers, {} in process",
        via.len(),
        inproc.len()
    );
    for sample in &inproc {
        assert!(
            via.contains(sample),
            "in-process sample {sample:?} differs or is missing through the worker path"
        );
    }
}

#[wasm_bindgen_test]
fn process_from_probes_refuses_a_stale_batch() {
    r3sizer_wasm::clear_cache();
    let rgba = stripe_rgba(128, 96);
    let params = params_json(1.0);
    r3sizer_wasm::prepare_base(&rgba, 128, 96, &params).unwrap();
    let b = bundle(&params);
    let batch = probe(&b, &params, &[0.5, 1.0], b.baseline, &b.provenance_json).unwrap();
    r3sizer_wasm::process_from_probes(&params, &batch, 0, "").unwrap();

    // Another sigma keeps the cached base (fast path) but changes the detail.
    let other_sigma = params_json(1.5);
    r3sizer_wasm::prepare_base(&rgba, 128, 96, &other_sigma).unwrap();
    let err = r3sizer_wasm::process_from_probes(&other_sigma, &batch, 0, "").unwrap_err();
    assert!(
        err.as_string()
            .unwrap()
            .contains("not measured on this prepared base"),
        "{:?}",
        err.as_string()
    );

    // An identical re-preparation is a new base instance.
    r3sizer_wasm::clear_cache();
    r3sizer_wasm::prepare_base(&rgba, 128, 96, &params).unwrap();
    let err = r3sizer_wasm::process_from_probes(&params, &batch, 0, "").unwrap_err();
    assert!(
        err.as_string()
            .unwrap()
            .contains("not measured on this prepared base"),
        "{:?}",
        err.as_string()
    );
}

#[wasm_bindgen_test]
fn a_worker_rejects_wrong_buffers_and_foreign_stamps() {
    r3sizer_wasm::clear_cache();
    let rgba = stripe_rgba(128, 96);
    let params = params_json(1.0);
    r3sizer_wasm::prepare_base(&rgba, 128, 96, &params).unwrap();
    let b = bundle(&params);
    let foreign = bundle(&params_json(1.5)).provenance_json;
    let short = &b.detail[..b.detail.len() - 1];

    let wrong_detail = r3sizer_wasm::probe_batch_with_detail(
        &b.base_pixels,
        b.width,
        b.height,
        &b.luminance,
        short,
        "[0.5]",
        &params,
        b.baseline,
        &b.provenance_json,
    )
    .unwrap_err();
    assert!(
        wrong_detail
            .as_string()
            .unwrap()
            .contains("buffer length mismatch: expected 768"),
        "{:?}",
        wrong_detail.as_string()
    );
    for err in [
        probe(&b, &params, &[0.5], b.baseline, &foreign).unwrap_err(),
        probe(&b, &params, &[0.5], b.baseline + 0.125, &b.provenance_json).unwrap_err(),
    ] {
        assert!(
            err.as_string()
                .unwrap()
                .contains("not measured on this prepared base"),
            "{:?}",
            err.as_string()
        );
    }
    let garbage = probe(&b, &params, &[0.5], b.baseline, "not json").unwrap_err();
    assert!(
        garbage
            .as_string()
            .unwrap()
            .starts_with("invalid provenance JSON:"),
        "{:?}",
        garbage.as_string()
    );
}
