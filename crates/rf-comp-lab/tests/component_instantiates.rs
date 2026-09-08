//! The packaged component must be able to start, prepare and produce audio.
//!
//! Native tests prove the engine; they do not prove the *component*. A wasm
//! module has a small shadow stack, and a processor that builds large state on
//! it traps inside `default()` with an out-of-bounds access — while every
//! native test still passes. This test runs the real host runtime, so a pass
//! here means the plugin starts on desktop, Raspberry Pi and Android alike.
//! It skips quietly when the wasm has not been built, so it costs nothing
//! during ordinary work.

use std::path::PathBuf;

use rackforge_plugin_runtime::{PortableEngine, RuntimeLimits};
use rf_comp_contract::index::{KNEE, RATIO, REDUCTION, THRESHOLD};

fn wasm_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-unknown-unknown/release/rackforge_rf_comp.wasm")
}

#[test]
fn the_packaged_compressor_can_start_and_take_what_the_ratio_says() {
    let path = wasm_path();
    if !path.is_file() {
        eprintln!(
            "skipping: no wasm at {}. Build it with\n  \
             cargo build --release --target wasm32-unknown-unknown -p rackforge-rf-comp",
            path.display()
        );
        return;
    }

    let runtime = PortableEngine::new(RuntimeLimits::default()).expect("runtime");
    let module = runtime
        .compile(&std::fs::read(&path).expect("read wasm"))
        .expect("the component must compile");

    let mut instance = module
        .instantiate()
        .expect("the compressor must instantiate; a trap here means the shadow stack overflowed");
    instance
        .prepare(48_000.0, 512, 2, 2)
        .expect("the compressor must prepare for a stereo input and output");
    instance
        .set_parameter(THRESHOLD, -18.0)
        .expect("a threshold must be accepted");
    instance
        .set_parameter(RATIO, 4.0)
        .expect("a ratio must be accepted");
    instance
        .set_parameter(KNEE, 0.0)
        .expect("a knee must be accepted");

    let frames = 512;
    let mut input = vec![0.0_f32; frames * 2];
    let mut output = vec![0.0_f32; frames * 2];
    let mut peak = 0.0_f32;
    // Minus six: twelve over the threshold, nine off at four to one.
    let amplitude = 10.0_f32.powf(-6.0 / 20.0);
    for block in 0..60 {
        for frame in 0..frames {
            let n = (block * frames + frame) as f32;
            let x = amplitude * (2.0 * std::f32::consts::PI * 1_000.0 * n / 48_000.0).sin();
            input[frame * 2] = x;
            input[frame * 2 + 1] = x;
        }
        instance
            .process_interleaved(&input, &mut output, frames as u32)
            .expect("the compressor must process a block");
        if block > 30 {
            for sample in &output {
                peak = peak.max(sample.abs());
            }
        }
    }
    assert!(
        output.iter().all(|sample| sample.is_finite()),
        "the component produced a non-finite sample"
    );
    let reduction = -6.0 - 20.0 * peak.log10();
    assert!(
        (reduction - 9.0).abs() < 0.3,
        "the component took {reduction} dB, not nine"
    );
    let meter = instance
        .get_parameter(REDUCTION)
        .expect("the meter must be readable");
    assert!(
        (-meter as f32 - reduction).abs() < 0.5,
        "the meter says {meter}, the audio {reduction}"
    );
}
