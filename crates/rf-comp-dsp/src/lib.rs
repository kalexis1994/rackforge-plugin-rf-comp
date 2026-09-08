//! RF-Comp: a feed-forward stereo compressor with a log-domain gain computer.
//!
//! * [`sidechain`] holds the detector parts — the sidechain high-pass and the
//!   mean-square window — neither of which allocates;
//! * [`curve`] is the static transfer curve: level in, reduction out, with a
//!   quadratic knee;
//! * [`Engine`] is the compressor: it measures, decides and applies.
//!
//! Two rules hold everywhere. Nothing allocates after activation. And every
//! constant that shapes the behaviour is named for what it is — a time, a
//! window, a floor — so that changing it means changing a decision rather
//! than nudging a number.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod curve;
pub mod engine;
pub mod math;
pub mod sidechain;

pub use engine::{Engine, MAXIMUM_SAMPLE_RATE, STATE_BYTES};
