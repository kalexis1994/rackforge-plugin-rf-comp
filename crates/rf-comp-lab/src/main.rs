//! The RF-Comp bench.
//!
//! Three jobs, all of them about keeping the project honest:
//!
//! * `metadata` renders the package's JSON from the contract, so the schema the
//!   host validates and the engine's behaviour come from one place;
//! * `render` puts a file, or a test burst, through the compressor and writes
//!   a file you can listen to;
//! * `curve` measures the static transfer curve: a steady sine from -60 to
//!   0 dB in three-decibel steps, against what the curve says it should take.

mod manifest;
mod metadata;

use std::path::{Path, PathBuf};

use rf_comp_contract::index::{REDUCTION, THRESHOLD};
use rf_comp_contract::{PARAMETERS, PRESETS};
use rf_comp_dsp::Engine;
use rf_comp_dsp::math::{db_to_gain, gain_to_db};

const SAMPLE_RATE: u32 = 48_000;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let command = arguments.first().map(String::as_str).unwrap_or("help");
    let result = match command {
        "metadata" => metadata_command(&arguments[1..]),
        "render" => render_command(&arguments[1..]),
        "curve" => curve_command(&arguments[1..]),
        "presets" => presets_command(),
        "parameters" => parameters_command(),
        _ => {
            usage();
            Ok(())
        }
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn usage() {
    println!(
        "rf-comp-lab <command>

  metadata [--check]              render package metadata from the contract
  parameters                      list every parameter and its range
  presets                         list the factory settings
  render --out <file.wav>         render a signal through the compressor
         [--preset <id>] [--input <file.wav>] [--seconds <n>]
         [--set <id>=<value> ...]
  curve [--preset <id>] [--set <id>=<value> ...]
                                  the static transfer curve: output level and
                                  reduction for a sine from -60 to 0 dB

Values may be given by parameter identifier or index:
  --set comp.threshold=-24 --set comp.ratio=2"
    );
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("the lab crate sits two levels below the repository root")
}

fn metadata_command(arguments: &[String]) -> Result<(), String> {
    let check = arguments.iter().any(|argument| argument == "--check");
    let package = repository_root().join("plugin").join("package");
    let identity = manifest::read(&package.join("rackforge-plugin.toml"))?;
    metadata::write(&package, &identity, check)?;
    if check {
        println!("metadata matches the contract");
    }
    Ok(())
}

fn parameters_command() -> Result<(), String> {
    for parameter in PARAMETERS.iter() {
        println!(
            "{:>3}  {:<22} {:<10} {}",
            parameter.index, parameter.id, parameter.page, parameter.name
        );
    }
    Ok(())
}

fn presets_command() -> Result<(), String> {
    for preset in PRESETS.iter() {
        println!(
            "{:<16} {:<18} {}",
            preset.id, preset.name, preset.description
        );
    }
    Ok(())
}

struct Options {
    preset: Option<String>,
    overrides: Vec<(u32, f64)>,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    seconds: f32,
}

fn parse_options(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options {
        preset: None,
        overrides: Vec::new(),
        input: None,
        output: None,
        seconds: 4.0,
    };
    let mut index = 0;
    while index < arguments.len() {
        let flag = arguments[index].as_str();
        let value = || {
            arguments
                .get(index + 1)
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag {
            "--preset" => options.preset = Some(value()?),
            "--set" => options.overrides.push(parse_assignment(&value()?)?),
            "--input" => options.input = Some(PathBuf::from(value()?)),
            "--out" => options.output = Some(PathBuf::from(value()?)),
            "--seconds" => {
                options.seconds = value()?
                    .parse()
                    .map_err(|_| "--seconds needs a number".to_owned())?
            }
            other => return Err(format!("unknown option {other}")),
        }
        index += 2;
    }
    Ok(options)
}

fn parse_assignment(assignment: &str) -> Result<(u32, f64), String> {
    let (key, value) = assignment
        .split_once('=')
        .ok_or_else(|| format!("expected id=value, got {assignment}"))?;
    let index = if let Ok(index) = key.parse::<u32>() {
        index
    } else {
        PARAMETERS
            .iter()
            .find(|parameter| parameter.id == key)
            .map(|parameter| parameter.index)
            .ok_or_else(|| format!("no parameter is called {key}"))?
    };
    let value = value
        .parse::<f64>()
        .map_err(|_| format!("{value} is not a number"))?;
    Ok((index, value))
}

fn engine_for(options: &Options) -> Result<Engine, String> {
    let mut engine = Engine::default();
    if !engine.prepare(f64::from(SAMPLE_RATE)) {
        return Err("the engine refused the sample rate".into());
    }
    if let Some(preset) = &options.preset
        && !engine.load_preset(preset)
    {
        return Err(format!("no preset is called {preset}"));
    }
    for (index, value) in &options.overrides {
        if !engine.set_parameter(*index, *value) {
            return Err(format!("parameter {index} refused {value}"));
        }
    }
    Ok(engine)
}

/// Reads a WAV as stereo frames at the bench rate, or, with no file, makes a
/// test signal: a tone that steps up six decibels every second.
fn source(options: &Options) -> Result<Vec<(f32, f32)>, String> {
    if let Some(path) = &options.input {
        let mut reader = hound::WavReader::open(path).map_err(|error| error.to_string())?;
        let spec = reader.spec();
        let channels = spec.channels.max(1) as usize;
        let scale = match spec.sample_format {
            hound::SampleFormat::Float => 1.0,
            hound::SampleFormat::Int => 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32,
        };
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .map(|sample| sample.map_err(|error| error.to_string()))
                .collect::<Result<_, _>>()?,
            hound::SampleFormat::Int => reader
                .samples::<i32>()
                .map(|sample| {
                    sample
                        .map(|value| value as f32 * scale)
                        .map_err(|error| error.to_string())
                })
                .collect::<Result<_, _>>()?,
        };
        return Ok(samples
            .chunks(channels)
            .map(|frame| (frame[0], frame.get(1).copied().unwrap_or(frame[0])))
            .collect());
    }
    let total = (options.seconds * SAMPLE_RATE as f32) as usize;
    Ok((0..total)
        .map(|n| {
            let second = n / SAMPLE_RATE as usize;
            let amplitude = db_to_gain(-30.0 + 6.0 * second as f32);
            let x = amplitude
                * (2.0 * core::f32::consts::PI * 220.0 * n as f32 / SAMPLE_RATE as f32).sin();
            (x, x)
        })
        .collect())
}

fn render_command(arguments: &[String]) -> Result<(), String> {
    let options = parse_options(arguments)?;
    let output = options
        .output
        .clone()
        .ok_or("render needs --out <file.wav>")?;
    let mut engine = engine_for(&options)?;
    let frames = source(&options)?;
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&output, spec).map_err(|error| error.to_string())?;
    let mut peak = 0.0_f32;
    for (left, right) in frames {
        let (l, r) = engine.process(left, right);
        peak = peak.max(l.abs()).max(r.abs());
        writer.write_sample(l).map_err(|error| error.to_string())?;
        writer.write_sample(r).map_err(|error| error.to_string())?;
    }
    writer.finalize().map_err(|error| error.to_string())?;
    println!(
        "wrote {} (peak {:.2} dBFS, threshold {:.1} dB, reduction now {:.1} dB)",
        output.display(),
        gain_to_db(peak),
        engine.parameter(THRESHOLD).unwrap_or(0.0),
        engine.parameter(REDUCTION).unwrap_or(0.0)
    );
    Ok(())
}

/// The static transfer curve, measured: a 1 kHz sine held for half a second
/// at each level, its output peak over the second quarter-second, against
/// the reduction the curve asks for at that level.
pub fn transfer_curve(engine: &mut Engine) -> Vec<CurvePoint> {
    let mut points = Vec::new();
    let mut level = -60.0_f32;
    while level <= 0.0 {
        let amplitude = db_to_gain(level);
        let mut peak = 0.0_f32;
        let total = SAMPLE_RATE as usize / 2;
        for n in 0..total {
            let x = amplitude
                * (2.0 * core::f32::consts::PI * 1_000.0 * n as f32 / SAMPLE_RATE as f32).sin();
            let (l, r) = engine.process(x, x);
            if n > total / 2 {
                peak = peak.max(l.abs()).max(r.abs());
            }
        }
        points.push(CurvePoint {
            input_db: level,
            output_db: gain_to_db(peak),
            expected_reduction_db: engine.static_reduction_db(level),
            meter_db: engine.parameter(REDUCTION).unwrap_or(0.0) as f32,
        });
        level += 3.0;
    }
    points
}

pub struct CurvePoint {
    pub input_db: f32,
    pub output_db: f32,
    /// What the curve says it should take, before makeup.
    pub expected_reduction_db: f32,
    /// What the meter says it took.
    pub meter_db: f32,
}

fn curve_command(arguments: &[String]) -> Result<(), String> {
    let options = parse_options(arguments)?;
    let mut engine = engine_for(&options)?;
    println!(
        "threshold {:.1} dB, makeup {:.2} dB, 1 kHz, half a second per step",
        engine.parameter(THRESHOLD).unwrap_or(0.0),
        engine.makeup_db()
    );
    println!(
        "{:>8} {:>10} {:>12} {:>10}",
        "in dB", "out dB", "curve GR dB", "meter dB"
    );
    for point in transfer_curve(&mut engine) {
        println!(
            "{:>8.1} {:>10.2} {:>12.2} {:>10.2}",
            point.input_db, point.output_db, point.expected_reduction_db, point.meter_db
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rf_comp_contract::index::{DETECTOR, DETECTOR_RMS, KNEE, RATIO};

    /// The bench agrees with the curve: at every step, the measured
    /// reduction is the static one, within the detector's ripple.
    #[test]
    fn the_measured_curve_follows_the_static_one() {
        let mut engine = Engine::default();
        assert!(engine.prepare(f64::from(SAMPLE_RATE)));
        assert!(engine.set_parameter(THRESHOLD, -18.0));
        assert!(engine.set_parameter(RATIO, 4.0));
        assert!(engine.set_parameter(KNEE, 0.0));
        assert!(engine.set_parameter(DETECTOR, f64::from(DETECTOR_RMS)));
        for point in transfer_curve(&mut engine) {
            let measured = point.input_db - point.output_db;
            assert!(
                (measured - point.expected_reduction_db).abs() < 0.3,
                "at {} dB: measured {measured}, curve {}",
                point.input_db,
                point.expected_reduction_db
            );
            assert!(
                (-point.meter_db - measured).abs() < 0.5,
                "at {} dB: meter {}, audio {measured}",
                point.input_db,
                point.meter_db
            );
        }
        // Twelve over at four to one: nine off, as the last point shows.
        let last = transfer_curve(&mut engine).pop().unwrap();
        assert!((last.expected_reduction_db - 13.5).abs() < 1.0e-4);
    }

    #[test]
    fn a_soft_knee_is_continuous_across_the_bench() {
        let mut engine = Engine::default();
        assert!(engine.prepare(f64::from(SAMPLE_RATE)));
        assert!(engine.set_parameter(THRESHOLD, -18.0));
        assert!(engine.set_parameter(RATIO, 4.0));
        assert!(engine.set_parameter(KNEE, 12.0));
        let points = transfer_curve(&mut engine);
        for pair in points.windows(2) {
            let step =
                (pair[1].input_db - pair[1].output_db) - (pair[0].input_db - pair[0].output_db);
            assert!(
                (-0.3..=3.0).contains(&step),
                "a jump between {} and {}",
                pair[0].input_db,
                pair[1].input_db
            );
        }
        let at_threshold = points
            .iter()
            .find(|point| point.input_db == -18.0)
            .map(|point| point.input_db - point.output_db)
            .unwrap();
        assert!(
            (at_threshold - 0.75 * 12.0 / 8.0).abs() < 0.1,
            "at the threshold {at_threshold}"
        );
    }
}
