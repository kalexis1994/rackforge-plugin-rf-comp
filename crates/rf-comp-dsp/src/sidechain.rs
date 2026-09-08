//! The detector's parts, each without allocation.
//!
//! * [`HighPass`] keeps the low end out of the sidechain, so a bass note
//!   does not pump the whole mix: a second-order Butterworth section;
//! * [`MeanSquare`] is the RMS detector's window: a one-pole on the squared
//!   signal, which is the mean square with an exponential memory.

use crate::math::{PI, sanitise, tan};

/// A second-order high-pass, transposed direct form II. Butterworth (a `Q`
/// of `1/sqrt 2`) because the corner should be the only thing it adds.
#[derive(Default, Clone, Copy)]
pub struct HighPass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    s1: f32,
    s2: f32,
    /// A corner of zero means "off": the section is skipped rather than
    /// asked to be transparent.
    active: bool,
}

impl HighPass {
    /// Sets the corner. Zero, or anything the sample rate cannot represent,
    /// turns the section off.
    pub fn set(&mut self, corner_hz: f32, sample_rate: f32) {
        if corner_hz <= 0.0 || corner_hz >= sample_rate * 0.45 {
            // Off starts clean when it comes back, rather than from
            // whatever the last corner left behind.
            self.active = false;
            self.clear();
            return;
        }
        let k = tan(PI * corner_hz / sample_rate);
        let q = core::f32::consts::FRAC_1_SQRT_2;
        let norm = 1.0 / (1.0 + k / q + k * k);
        self.b0 = norm;
        self.b1 = -2.0 * norm;
        self.b2 = norm;
        self.a1 = 2.0 * (k * k - 1.0) * norm;
        self.a2 = (1.0 - k / q + k * k) * norm;
        self.active = true;
    }

    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        if !self.active {
            return input;
        }
        let output = self.b0 * input + self.s1;
        self.s1 = sanitise(self.b1 * input - self.a1 * output + self.s2);
        self.s2 = sanitise(self.b2 * input - self.a2 * output);
        output
    }

    pub fn clear(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }
}

/// The mean of the squared signal with an exponential memory.
#[derive(Default, Clone, Copy)]
pub struct MeanSquare {
    coefficient: f32,
    state: f32,
}

impl MeanSquare {
    pub fn set(&mut self, coefficient: f32) {
        self.coefficient = coefficient;
    }

    /// Pushes a sample and returns the mean square so far.
    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        self.state = sanitise(self.state + (input * input - self.state) * self.coefficient);
        self.state
    }

    pub fn clear(&mut self) {
        self.state = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{gain_to_db, one_pole, sin, sqrt};

    const RATE: f32 = 48_000.0;

    fn level_after(filter: &mut HighPass, frequency: f32) -> f32 {
        let mut peak = 0.0_f32;
        let total = RATE as usize;
        for n in 0..total {
            let y = filter.process(sin(2.0 * PI * frequency * n as f32 / RATE));
            if n > total / 2 {
                peak = peak.max(y.abs());
            }
        }
        gain_to_db(peak)
    }

    #[test]
    fn the_high_pass_is_three_decibels_down_at_its_corner_and_flat_above() {
        let mut filter = HighPass::default();
        filter.set(120.0, RATE);
        let corner = level_after(&mut filter, 120.0);
        assert!((corner + 3.0).abs() < 0.2, "corner {corner}");
        filter.clear();
        let above = level_after(&mut filter, 2_000.0);
        assert!(above.abs() < 0.05, "above {above}");
        filter.clear();
        // Second order: twelve decibels per octave, an octave below.
        let below = level_after(&mut filter, 60.0);
        assert!((-13.5..=-11.5).contains(&below), "below {below}");
    }

    #[test]
    fn off_is_the_signal_itself() {
        let mut filter = HighPass::default();
        filter.set(0.0, RATE);
        assert_eq!(filter.process(0.3), 0.3);
    }

    #[test]
    fn the_mean_square_of_a_sine_is_half_its_amplitude_squared() {
        let mut window = MeanSquare::default();
        window.set(one_pole(0.01, RATE));
        let mut mean = 0.0;
        for n in 0..RATE as usize {
            mean = window.process(0.5 * sin(2.0 * PI * 440.0 * n as f32 / RATE));
        }
        let rms = sqrt(mean);
        assert!(
            (rms - 0.5 * core::f32::consts::FRAC_1_SQRT_2).abs() < 0.01,
            "rms {rms}"
        );
    }
}
