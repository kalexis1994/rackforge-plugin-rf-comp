//! The compressor: a level detector, a static curve and ballistics on the
//! reduction, applied to the audio the level was measured from.
//!
//! Feed-forward: the sidechain reads the input, never the output, so the gain
//! does not chase itself. The gain computer works in decibels, where the
//! transfer curve is a straight line and a knee is a parabola. The attack and
//! release are one-poles on the reduction the curve asks for — ballistics on
//! the control signal, which is the classic design: an attack is the same
//! length at any level, and the reduction is a smooth curve in decibels,
//! which the ear reads as level rather than as a click. The gain is applied
//! in the linear domain, once per channel per sample.
//!
//! No latency: the reduction lands on the sample that asked for it.

use rf_comp_contract::index::*;
use rf_comp_contract::{PARAMETER_COUNT, PREVIOUS_PARAMETER_COUNTS, RATIO_MAX, Settings, preset};

use crate::curve::reduction_db;
use crate::math::{abs, clamp, db_to_gain, gain_to_db, one_pole, sanitise, sqrt};
use crate::sidechain::{HighPass, MeanSquare};

/// Length of the serialised state, in bytes.
pub const STATE_BYTES: usize = PARAMETER_COUNT * 4;

/// The highest sample rate accepted. Nothing here is sized by the rate; the
/// bound only keeps the coefficient arithmetic where it was measured.
pub const MAXIMUM_SAMPLE_RATE: f32 = 192_000.0;

/// The RMS detector's memory: ten milliseconds, long enough to average a
/// cycle of anything above the bottom of a bass, short enough to still be a
/// compressor rather than a leveller.
const RMS_WINDOW_S: f32 = 0.01;

/// The programme-dependent release: a fast stage that lets a transient go
/// and a slow one that holds a sustained passage down, averaged. A decade
/// apart, as a bus compressor's "auto" position usually is.
const AUTO_FAST_S: f32 = 0.06;
const AUTO_SLOW_S: f32 = 0.6;

/// How long a threshold or makeup change takes to land: five milliseconds,
/// short enough that automation is felt at once, long enough that a decibel
/// step is a slope rather than a zipper.
const SMOOTHING_S: f32 = 0.005;

/// How fast the gain-reduction meter lets go once the reduction has passed.
/// A display ballistic only; the audio uses the release knob.
const METER_RELEASE_S: f32 = 0.3;

/// Below this the meter reads its floor rather than minus infinity.
const METER_FLOOR_DB: f32 = -40.0;

/// One side's detector and envelopes. The envelopes are in decibels of
/// reduction, at or above zero.
#[derive(Default, Clone, Copy)]
struct Lane {
    high_pass: HighPass,
    mean_square: MeanSquare,
    env_fixed: f32,
    env_fast: f32,
    env_slow: f32,
}

impl Lane {
    fn rest(&mut self) {
        self.high_pass.clear();
        self.mean_square.clear();
        self.env_fixed = 0.0;
        self.env_fast = 0.0;
        self.env_slow = 0.0;
    }
}

pub struct Engine {
    settings: Settings,
    sample_rate: f32,

    /// The threshold and makeup as set, and as smoothed towards.
    threshold_target: f32,
    threshold: f32,
    makeup_target: f32,
    makeup: f32,
    smoothing: f32,

    /// `1/ratio`; zero at the top of the knob, where the slope is a limiter's.
    slope: f32,
    knee: f32,
    attack: f32,
    release_fixed: f32,
    release_fast: f32,
    release_slow: f32,
    auto_release: bool,
    rms: bool,
    /// How much of the louder side each side hears, 0 to 1.
    link: f32,
    mix: f32,
    bypass: bool,
    meter_release: f32,

    lanes: [Lane; 2],
    /// The widest reduction applied lately, in dB, letting go at the
    /// meter's rate.
    meter_reduction: f32,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            sample_rate: 48_000.0,
            threshold_target: 0.0,
            threshold: 0.0,
            makeup_target: 0.0,
            makeup: 0.0,
            smoothing: 1.0,
            slope: 1.0,
            knee: 0.0,
            attack: 1.0,
            release_fixed: 1.0,
            release_fast: 1.0,
            release_slow: 1.0,
            auto_release: false,
            rms: true,
            link: 1.0,
            mix: 1.0,
            bypass: false,
            meter_release: 1.0,
            lanes: [Lane::default(), Lane::default()],
            meter_reduction: 0.0,
        }
    }
}

/// One step of the envelope towards `wanted`: at the attack rate when the
/// reduction is growing, at the release rate when it is letting go.
#[inline]
fn follow(envelope: f32, wanted: f32, attack: f32, release: f32) -> f32 {
    let coefficient = if wanted > envelope { attack } else { release };
    sanitise(envelope + (wanted - envelope) * coefficient)
}

impl Engine {
    /// Sets the sample rate and puts every stage at rest. Refuses a rate the
    /// coefficients were not designed for.
    pub fn prepare(&mut self, sample_rate: f64) -> bool {
        if !sample_rate.is_finite()
            || sample_rate <= 0.0
            || sample_rate > f64::from(MAXIMUM_SAMPLE_RATE)
        {
            return false;
        }
        self.sample_rate = sample_rate as f32;
        self.apply_settings();
        self.reset();
        true
    }

    /// Puts the detectors and envelopes at rest and lands the smoothed
    /// settings on their targets.
    pub fn reset(&mut self) {
        for lane in &mut self.lanes {
            lane.rest();
        }
        self.threshold = self.threshold_target;
        self.makeup = self.makeup_target;
        self.meter_reduction = 0.0;
    }

    pub fn set_parameter(&mut self, index: u32, value: f64) -> bool {
        if !self.settings.set(index, value) {
            return false;
        }
        self.apply_settings();
        true
    }

    /// A parameter's value: the setting, or, for the meter, the reading.
    pub fn parameter(&self, index: u32) -> Option<f64> {
        if index == REDUCTION {
            // Subtracted from zero rather than negated, so a meter at rest
            // reads 0 and not -0.
            return Some(f64::from(clamp(
                0.0 - self.meter_reduction,
                METER_FLOOR_DB,
                0.0,
            )));
        }
        self.settings.get(index)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The reduction the current settings would take from a steady level,
    /// once the ballistics have settled: the static transfer curve.
    pub fn static_reduction_db(&self, level_db: f32) -> f32 {
        reduction_db(level_db, self.threshold_target, self.slope, self.knee)
    }

    /// The makeup in force, manual and automatic together, in dB.
    pub fn makeup_db(&self) -> f32 {
        self.makeup_target
    }

    pub fn load_preset(&mut self, id: &str) -> bool {
        let Some(settings) = preset::settings_for(id) else {
            return false;
        };
        self.settings = settings;
        self.apply_settings();
        self.reset();
        true
    }

    pub fn save_state(&self, destination: &mut [u8]) -> Option<usize> {
        let target = destination.get_mut(..STATE_BYTES)?;
        for (slot, value) in target
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(self.settings.as_array())
        {
            *slot = value.to_le_bytes();
        }
        Some(STATE_BYTES)
    }

    /// Restores a saved block, including one written by an earlier build.
    /// Its length identifies its layout; anything else is refused whole.
    pub fn load_state(&mut self, state: &[u8]) -> bool {
        if !state.len().is_multiple_of(4) {
            return false;
        }
        let count = state.len() / 4;
        if count != PARAMETER_COUNT && !PREVIOUS_PARAMETER_COUNTS.contains(&count) {
            return false;
        }
        let mut values = [0.0_f32; PARAMETER_COUNT];
        for (value, word) in values.iter_mut().zip(state.as_chunks::<4>().0) {
            *value = f32::from_le_bytes(*word);
        }
        let Some(settings) = Settings::from_slice(&values[..count]) else {
            return false;
        };
        self.settings = settings;
        self.apply_settings();
        self.reset();
        true
    }

    fn apply_settings(&mut self) {
        let settings = self.settings;
        let rate = self.sample_rate;
        self.threshold_target = settings.value(THRESHOLD);
        let ratio = settings.value(RATIO);
        self.slope = if ratio >= RATIO_MAX { 0.0 } else { 1.0 / ratio };
        self.knee = settings.value(KNEE);
        self.attack = one_pole(settings.value(ATTACK) * 0.001, rate);
        self.release_fixed = one_pole(settings.value(RELEASE) * 0.001, rate);
        self.release_fast = one_pole(AUTO_FAST_S, rate);
        self.release_slow = one_pole(AUTO_SLOW_S, rate);
        self.auto_release = settings.engaged(AUTO_RELEASE);
        self.rms = settings.value(DETECTOR) as u32 == DETECTOR_RMS;
        let corner = SIDECHAIN_HPF_HZ
            .get(settings.value(SIDECHAIN_HPF) as usize)
            .copied()
            .unwrap_or(0.0);
        let window = one_pole(RMS_WINDOW_S, rate);
        for lane in &mut self.lanes {
            lane.high_pass.set(corner, rate);
            lane.mean_square.set(window);
        }
        self.link = settings.value(LINK) * 0.01;
        // Auto makeup: the common rule, half of what the curve would take
        // from a full-scale signal — `makeup = -(gain at 0 dB in) * 0.5` —
        // added to whatever the knob says.
        let automatic = if settings.engaged(AUTO_MAKEUP) {
            0.5 * reduction_db(0.0, self.threshold_target, self.slope, self.knee)
        } else {
            0.0
        };
        self.makeup_target = settings.value(MAKEUP) + automatic;
        self.mix = settings.value(MIX) * 0.01;
        self.bypass = settings.engaged(BYPASS);
        self.smoothing = one_pole(SMOOTHING_S, rate);
        self.meter_release = one_pole(METER_RELEASE_S, rate);
    }

    /// One stereo frame in, one out. A mono input arrives on both sides.
    ///
    /// Bypass still runs the sidechain, so taking it off does not start
    /// from a cold envelope; it applies unity and the meter reads nothing.
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let inputs = [sanitise(left), sanitise(right)];
        self.threshold += (self.threshold_target - self.threshold) * self.smoothing;
        self.makeup += (self.makeup_target - self.makeup) * self.smoothing;

        // Detect: each side's level in dB, after the sidechain filter.
        let mut levels = [0.0_f32; 2];
        for ((lane, input), level) in self.lanes.iter_mut().zip(inputs).zip(levels.iter_mut()) {
            let side = lane.high_pass.process(input);
            *level = if self.rms {
                // Sine-calibrated: a full-scale sine reads 0 dB, as it does
                // on the peak detector, so the threshold means the same
                // level on both.
                gain_to_db(sqrt(2.0 * lane.mean_square.process(side)))
            } else {
                gain_to_db(abs(side))
            };
        }
        // Link: each side hears the louder of the two, by the link amount.
        // Fully linked, both sides hear the same level and get the same gain.
        let louder = if levels[0] > levels[1] {
            levels[0]
        } else {
            levels[1]
        };
        let heard = [
            levels[0] + (louder - levels[0]) * self.link,
            levels[1] + (louder - levels[1]) * self.link,
        ];

        let mut outputs = [0.0_f32; 2];
        let mut widest = 0.0_f32;
        for ((lane, input), (level, output)) in self
            .lanes
            .iter_mut()
            .zip(inputs)
            .zip(heard.into_iter().zip(outputs.iter_mut()))
        {
            let wanted = reduction_db(level, self.threshold, self.slope, self.knee);
            lane.env_fixed = follow(lane.env_fixed, wanted, self.attack, self.release_fixed);
            lane.env_fast = follow(lane.env_fast, wanted, self.attack, self.release_fast);
            lane.env_slow = follow(lane.env_slow, wanted, self.attack, self.release_slow);
            let reduction = if self.auto_release {
                0.5 * (lane.env_fast + lane.env_slow)
            } else {
                lane.env_fixed
            };
            if reduction > widest {
                widest = reduction;
            }
            *output = if self.bypass {
                input
            } else {
                let wet = input * db_to_gain(self.makeup - reduction);
                // Weighted so that either end of the knob is exact: all dry
                // is the input itself, all wet is the compressed signal.
                sanitise(input * (1.0 - self.mix) + wet * self.mix)
            };
        }

        if self.bypass {
            widest = 0.0;
        }
        self.meter_reduction =
            sanitise(self.meter_reduction + (widest - self.meter_reduction) * self.meter_release);
        if widest > self.meter_reduction {
            self.meter_reduction = widest;
        }
        (outputs[0], outputs[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{PI, sin};

    const RATE: f32 = 48_000.0;

    fn prepared() -> Engine {
        let mut engine = Engine::default();
        assert!(engine.prepare(f64::from(RATE)));
        engine
    }

    /// A hard-kneed four-to-one at minus eighteen on the RMS detector: the
    /// textbook case most tests measure against.
    fn textbook() -> Engine {
        let mut engine = prepared();
        assert!(engine.set_parameter(THRESHOLD, -18.0));
        assert!(engine.set_parameter(RATIO, 4.0));
        assert!(engine.set_parameter(KNEE, 0.0));
        assert!(engine.set_parameter(DETECTOR, f64::from(DETECTOR_RMS)));
        engine
    }

    /// Runs a sine of `amplitude` at `frequency` on both sides for `seconds`
    /// and returns each side's output peak over the second half.
    fn peaks_of(
        engine: &mut Engine,
        amplitude: (f32, f32),
        frequency: f32,
        seconds: f32,
    ) -> (f32, f32) {
        let total = (seconds * RATE) as usize;
        let mut peaks = (0.0_f32, 0.0_f32);
        for n in 0..total {
            let x = sin(2.0 * PI * frequency * n as f32 / RATE);
            let (l, r) = engine.process(amplitude.0 * x, amplitude.1 * x);
            if n > total / 2 {
                peaks.0 = peaks.0.max(abs(l));
                peaks.1 = peaks.1.max(abs(r));
            }
        }
        peaks
    }

    /// The reduction the audio shows, in dB, for a sine at `amplitude_db`,
    /// from rest: the previous measurement's release is not still in it.
    fn measured_reduction(engine: &mut Engine, amplitude_db: f32) -> f32 {
        engine.reset();
        let amplitude = db_to_gain(amplitude_db);
        let (peak, _) = peaks_of(engine, (amplitude, amplitude), 1_000.0, 0.6);
        amplitude_db - gain_to_db(peak)
    }

    #[test]
    fn it_refuses_a_rate_it_was_not_designed_for() {
        let mut engine = Engine::default();
        assert!(!engine.prepare(384_000.0));
        assert!(!engine.prepare(0.0));
        assert!(engine.prepare(192_000.0));
    }

    #[test]
    fn the_static_curve_takes_what_the_ratio_says() {
        let mut engine = textbook();
        // Twelve decibels over at four to one: nine come off.
        let reduction = measured_reduction(&mut engine, -6.0);
        assert!((reduction - 9.0).abs() < 0.3, "reduction {reduction}");

        assert!(engine.set_parameter(RATIO, 1.0));
        let unity = measured_reduction(&mut engine, -6.0);
        assert!(unity.abs() < 0.05, "ratio 1 took {unity}");

        assert!(engine.set_parameter(RATIO, 4.0));
        let quiet = measured_reduction(&mut engine, -38.0);
        assert!(quiet.abs() < 0.05, "a quiet signal lost {quiet}");
    }

    #[test]
    fn the_top_of_the_ratio_knob_is_a_limiter() {
        let mut engine = textbook();
        assert!(engine.set_parameter(RATIO, f64::from(RATIO_MAX)));
        let reduction = measured_reduction(&mut engine, -6.0);
        assert!((reduction - 12.0).abs() < 0.3, "reduction {reduction}");
    }

    #[test]
    fn the_soft_knee_takes_an_eighth_of_its_width_at_the_threshold() {
        let mut engine = textbook();
        assert!(engine.set_parameter(KNEE, 12.0));
        let at_threshold = measured_reduction(&mut engine, -18.0);
        let expected = 0.75 * 12.0 / 8.0;
        assert!(
            (at_threshold - expected).abs() < 0.1,
            "at the threshold {at_threshold}, expected {expected}"
        );
        // Outside the knee the curve is the hard one.
        let over = measured_reduction(&mut engine, -6.0);
        assert!((over - 9.0).abs() < 0.3, "over {over}");
        let under = measured_reduction(&mut engine, -30.0);
        assert!(under.abs() < 0.05, "under {under}");
        // And through it, monotonic.
        let mut previous = 0.0;
        for level in [-24.0, -21.0, -18.0, -15.0, -12.0] {
            let reduction = engine.static_reduction_db(level);
            assert!(reduction >= previous, "at {level}");
            previous = reduction;
        }
    }

    /// A step of direct current on the peak detector: the level is the
    /// sample itself, so the only ballistic in the way is the one under
    /// test.
    fn reduction_trace(engine: &mut Engine, amplitude: f32, samples: usize) -> std::vec::Vec<f32> {
        (0..samples)
            .map(|_| {
                let (l, _) = engine.process(amplitude, amplitude);
                gain_to_db(amplitude) - gain_to_db(l)
            })
            .collect()
    }

    fn first_crossing(trace: &[f32], target: f32, rising: bool) -> usize {
        trace
            .iter()
            .position(|value| {
                if rising {
                    *value >= target
                } else {
                    *value <= target
                }
            })
            .expect("the envelope never got there")
    }

    #[test]
    fn the_attack_and_release_reach_their_time_constants() {
        let mut engine = textbook();
        assert!(engine.set_parameter(DETECTOR, f64::from(DETECTOR_PEAK)));
        assert!(engine.set_parameter(ATTACK, 10.0));
        assert!(engine.set_parameter(RELEASE, 100.0));

        let loud = db_to_gain(-6.0);
        let attack = reduction_trace(&mut engine, loud, RATE as usize);
        let settled = *attack.last().unwrap();
        assert!((settled - 9.0).abs() < 0.05, "settled at {settled}");
        let at_63 = first_crossing(&attack, 0.632 * settled, true) as f32 / RATE * 1000.0;
        assert!((8.0..=12.0).contains(&at_63), "attack took {at_63} ms");

        // Twenty-two decibels under the threshold: the envelope lets go.
        let quiet = db_to_gain(-40.0);
        let release = reduction_trace(&mut engine, quiet, RATE as usize);
        let at_37 = first_crossing(&release, 0.368 * settled, false) as f32 / RATE * 1000.0;
        assert!((80.0..=120.0).contains(&at_37), "release took {at_37} ms");
        assert!(release.last().unwrap().abs() < 0.01);
    }

    #[test]
    fn the_auto_release_settles_where_the_fixed_one_does() {
        let mut engine = textbook();
        assert!(engine.set_parameter(AUTO_RELEASE, 1.0));
        let reduction = measured_reduction(&mut engine, -6.0);
        assert!((reduction - 9.0).abs() < 0.3, "reduction {reduction}");
    }

    #[test]
    fn all_dry_and_bypass_are_the_input_itself() {
        for (index, value) in [(MIX, 0.0), (BYPASS, 1.0)] {
            let mut engine = textbook();
            assert!(engine.set_parameter(index, value));
            assert!(engine.set_parameter(MAKEUP, 6.0));
            for n in 0..(RATE as usize / 4) {
                let x = 0.9 * sin(2.0 * PI * 440.0 * n as f32 / RATE);
                let (l, r) = engine.process(x, 0.5 * x);
                assert_eq!(l, x, "parameter {index}");
                assert_eq!(r, 0.5 * x, "parameter {index}");
            }
        }
    }

    #[test]
    fn makeup_without_compression_is_a_plain_gain() {
        let mut engine = textbook();
        assert!(engine.set_parameter(MAKEUP, 6.0));
        let quiet = db_to_gain(-40.0);
        let (peak, _) = peaks_of(&mut engine, (quiet, quiet), 1_000.0, 0.5);
        let gain = gain_to_db(peak / quiet);
        assert!((gain - 6.0).abs() < 0.01, "gain {gain}");
    }

    #[test]
    fn auto_makeup_is_half_the_reduction_at_full_scale() {
        let mut engine = textbook();
        assert!(engine.set_parameter(AUTO_MAKEUP, 1.0));
        // Eighteen over at four to one is thirteen and a half; half of that.
        assert!((engine.makeup_db() - 6.75).abs() < 1.0e-4);
        let quiet = db_to_gain(-40.0);
        let (peak, _) = peaks_of(&mut engine, (quiet, quiet), 1_000.0, 0.5);
        let gain = gain_to_db(peak / quiet);
        assert!((gain - 6.75).abs() < 0.05, "gain {gain}");
    }

    #[test]
    fn unlinked_sides_are_compressed_on_their_own() {
        let loud = db_to_gain(-6.0);
        let quiet = db_to_gain(-40.0);

        let mut engine = textbook();
        assert!(engine.set_parameter(LINK, 0.0));
        let (left, right) = peaks_of(&mut engine, (loud, quiet), 1_000.0, 0.6);
        let left_reduction = -6.0 - gain_to_db(left);
        assert!((left_reduction - 9.0).abs() < 0.3, "left {left_reduction}");
        assert!(
            (right - quiet).abs() < 1.0e-6,
            "right {right} against {quiet}"
        );

        assert!(engine.set_parameter(LINK, 100.0));
        let (_, right) = peaks_of(&mut engine, (loud, quiet), 1_000.0, 0.6);
        let right_reduction = -40.0 - gain_to_db(right);
        assert!(
            (right_reduction - 9.0).abs() < 0.3,
            "linked right {right_reduction}"
        );
    }

    #[test]
    fn the_meter_reads_what_the_audio_shows() {
        let mut engine = textbook();
        let reduction = measured_reduction(&mut engine, -6.0);
        let meter = -engine.parameter(REDUCTION).unwrap() as f32;
        assert!(
            (meter - reduction).abs() < 0.5,
            "meter {meter}, audio {reduction}"
        );
        for _ in 0..(3.0 * RATE) as usize {
            engine.process(0.0, 0.0);
        }
        assert!(engine.parameter(REDUCTION).unwrap() > -0.1);
    }

    #[test]
    fn the_sidechain_filter_keeps_the_bass_out_of_the_detector_only() {
        let mut engine = textbook();
        assert!(engine.set_parameter(SIDECHAIN_HPF, 3.0));
        let loud = db_to_gain(-6.0);
        // At 40 Hz the detector hears a signal thirty decibels down; the
        // audio path hears all of it.
        let (bass, _) = peaks_of(&mut engine, (loud, loud), 40.0, 1.0);
        assert!((bass - loud).abs() < 0.01, "bass {bass} against {loud}");
        let reduction = measured_reduction(&mut engine, -6.0);
        assert!((reduction - 9.0).abs() < 0.3, "at 1 kHz {reduction}");
    }

    #[test]
    fn ten_seconds_of_silence_leave_nothing_behind() {
        let mut engine = textbook();
        measured_reduction(&mut engine, -3.0);
        for _ in 0..(10.0 * RATE) as usize {
            let (l, r) = engine.process(0.0, 0.0);
            assert_eq!(l, 0.0);
            assert_eq!(r, 0.0);
        }
        assert!(engine.parameter(REDUCTION).unwrap().is_finite());
        let reduction = measured_reduction(&mut engine, -6.0);
        assert!((reduction - 9.0).abs() < 0.3, "after silence {reduction}");
    }

    #[test]
    fn the_engine_is_deterministic() {
        let mut first = prepared();
        let mut second = prepared();
        let mut seed = 0x2545_F491_u32;
        for n in 0..(RATE as usize) {
            if n == 12_000 {
                assert!(first.set_parameter(THRESHOLD, -30.0));
                assert!(second.set_parameter(THRESHOLD, -30.0));
            }
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let x = (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
            assert_eq!(first.process(x, -x), second.process(x, -x));
        }
        assert_eq!(first.parameter(REDUCTION), second.parameter(REDUCTION));
    }

    #[test]
    fn state_round_trips_and_a_bad_length_is_refused() {
        let mut engine = prepared();
        assert!(engine.set_parameter(THRESHOLD, -24.5));
        assert!(engine.set_parameter(SIDECHAIN_HPF, 2.0));
        let mut block = [0_u8; STATE_BYTES];
        assert_eq!(engine.save_state(&mut block), Some(STATE_BYTES));
        let mut other = prepared();
        assert!(other.load_state(&block));
        assert_eq!(other.parameter(THRESHOLD), Some(-24.5));
        assert_eq!(other.parameter(SIDECHAIN_HPF), Some(2.0));
        assert!(!other.load_state(&block[..7]));
        assert!(!other.load_state(&block[..8]));
    }

    #[test]
    fn every_preset_loads() {
        let mut engine = prepared();
        for preset in rf_comp_contract::PRESETS.iter() {
            assert!(engine.load_preset(preset.id), "{}", preset.id);
        }
        assert!(!engine.load_preset("nowhere"));
    }
}
