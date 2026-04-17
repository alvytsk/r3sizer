//! End-to-end pipeline profiling with per-stage timing breakdown.
//!
//! Not a criterion benchmark — this is a one-shot profiler that prints
//! wall-clock time per pipeline stage for representative workloads.
//! Run with: cargo bench --bench profile_e2e

use r3sizer_core::{
    prepare_base, process_auto_sharp_downscale, process_from_prepared, AutoSharpParams,
    LinearRgbImage, PipelineMode, StageTiming,
};

fn synthetic_image(w: u32, h: u32) -> LinearRgbImage {
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            // Gradient with fine texture (alternating pattern)
            let r = x as f32 / (w - 1) as f32;
            let g = y as f32 / (h - 1) as f32;
            let b = ((x + y) % 2) as f32 * 0.3 + 0.35;
            data.extend_from_slice(&[r, g, b]);
        }
    }
    LinearRgbImage::new(w, h, data).unwrap()
}

fn print_timing(label: &str, timing: &StageTiming) {
    let total = timing.total_us;
    let fmt = |name: &str, us: u64| {
        let pct = if total > 0 {
            us as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        println!("    {name:<28} {us:>8} µs  ({pct:5.1}%)");
    };

    println!("\n  {label}");
    println!("  {}", "-".repeat(54));
    if let Some(us) = timing.ingress_us {
        fmt("ingress (color space)", us);
    }
    fmt("resize", timing.resize_us);
    if let Some(us) = timing.classification_us {
        fmt("classification", us);
    }
    if let Some(us) = timing.base_quality_us {
        fmt("base_quality", us);
    }
    fmt("baseline", timing.baseline_us);
    fmt("contrast", timing.contrast_us);
    fmt("probing", timing.probing_us);
    fmt("fit", timing.fit_us);
    fmt("robustness", timing.robustness_us);
    fmt("final_sharpen", timing.final_sharpen_us);
    if let Some(us) = timing.adaptive_validation_us {
        fmt("adaptive_validation", us);
    }
    if let Some(us) = timing.evaluator_us {
        fmt("evaluator", us);
    }
    fmt("clamp", timing.clamp_us);
    println!("  {}", "-".repeat(54));
    fmt("TOTAL", total);
}

fn run_scenario(label: &str, src: &LinearRgbImage, params: &AutoSharpParams) {
    // Warmup
    let _ = process_auto_sharp_downscale(src, params);

    // Timed run with two-phase split to identify gaps
    let mut timings = Vec::new();
    let mut phase_timings = Vec::new();
    for _ in 0..3 {
        let t_prep_start = std::time::Instant::now();
        let prepared = prepare_base(src, params, &|_| {}).unwrap();
        let prep_wall_us = t_prep_start.elapsed().as_micros() as u64;

        let t_proc_start = std::time::Instant::now();
        let result = process_from_prepared(&prepared, params, &|_| {}).unwrap();
        let proc_wall_us = t_proc_start.elapsed().as_micros() as u64;

        timings.push(result.diagnostics.timing);
        phase_timings.push((prep_wall_us, proc_wall_us));
    }

    // Use the median total_us run
    let mut indices: Vec<usize> = (0..3).collect();
    indices.sort_by_key(|&i| timings[i].total_us);
    let median = indices[1];

    print_timing(label, &timings[median]);

    let (prep_wall, proc_wall) = phase_timings[median];
    let timing = &timings[median];

    // Sum all timed stages from StageTiming
    let timed_in_prepare = timing.resize_us
        + timing.contrast_us
        + timing.classification_us.unwrap_or(0)
        + timing.base_quality_us.unwrap_or(0)
        + timing.baseline_us
        + timing.evaluator_us.unwrap_or(0)
        + timing.ingress_us.unwrap_or(0);
    let timed_in_process = timing.probing_us
        + timing.fit_us
        + timing.robustness_us
        + timing.final_sharpen_us
        + timing.adaptive_validation_us.unwrap_or(0)
        + timing.clamp_us;

    let prep_gap = prep_wall.saturating_sub(timed_in_prepare);
    let proc_gap = proc_wall.saturating_sub(timed_in_process);

    println!("    --- Phase breakdown ---");
    println!(
        "    prepare_base wall:    {:>8} µs  (timed: {} µs, gap: {} µs)",
        prep_wall, timed_in_prepare, prep_gap
    );
    println!(
        "    process_from_prepared: {:>7} µs  (timed: {} µs, gap: {} µs)",
        proc_wall, timed_in_process, proc_gap
    );
}

fn main() {
    println!("=== End-to-end pipeline profiling ===\n");

    // Scenario 1: 1080p → 540p (2× downscale), all modes
    {
        let src = synthetic_image(1920, 1080);
        println!("Source: 1920×1080 (2.1 MP)");
        println!("Target: 960×540 (2× downscale)");

        for (name, mode) in [
            ("Fast", PipelineMode::Fast),
            ("Balanced", PipelineMode::Balanced),
            ("Quality", PipelineMode::Quality),
        ] {
            let params = AutoSharpParams {
                pipeline_mode: Some(mode),
                ..AutoSharpParams::photo(960, 540)
            }
            .resolved();
            run_scenario(&format!("1080p→540p {name}"), &src, &params);
        }
    }

    // Scenario 2: 4K → 720p (3× downscale)
    {
        let src = synthetic_image(3840, 2160);
        println!("\nSource: 3840×2160 (8.3 MP)");
        println!("Target: 1280×720 (3× downscale)");

        for (name, mode) in [
            ("Fast", PipelineMode::Fast),
            ("Balanced", PipelineMode::Balanced),
        ] {
            let params = AutoSharpParams {
                pipeline_mode: Some(mode),
                ..AutoSharpParams::photo(1280, 720)
            }
            .resolved();
            run_scenario(&format!("4K→720p {name}"), &src, &params);
        }
    }

    // Scenario 3: 4K → 360p (6× downscale, triggers staged shrink)
    {
        let src = synthetic_image(3840, 2160);
        println!("\nSource: 3840×2160 (8.3 MP)");
        println!("Target: 640×360 (6× downscale, staged shrink)");

        let params = AutoSharpParams {
            pipeline_mode: Some(PipelineMode::Fast),
            ..AutoSharpParams::photo(640, 360)
        }
        .resolved();
        run_scenario("4K→360p Fast", &src, &params);

        let params = AutoSharpParams {
            pipeline_mode: Some(PipelineMode::Balanced),
            ..AutoSharpParams::photo(640, 360)
        }
        .resolved();
        run_scenario("4K→360p Balanced", &src, &params);
    }

    // Scenario 4: Small image 640×480 → 320×240
    {
        let src = synthetic_image(640, 480);
        println!("\nSource: 640×480 (0.3 MP)");
        println!("Target: 320×240 (2× downscale)");

        let params = AutoSharpParams {
            pipeline_mode: Some(PipelineMode::Balanced),
            ..AutoSharpParams::photo(320, 240)
        }
        .resolved();
        run_scenario("VGA→QVGA Balanced", &src, &params);
    }

    println!();
}
