//! Named parameter indexes.
//!
//! The engine and the packager both address parameters by number. Naming them
//! here — and asserting in tests that each name still points at the identifier
//! it claims — keeps a renumbering from quietly rewiring a knob.

pub const THRESHOLD: u32 = 0;
pub const RATIO: u32 = 1;
pub const ATTACK: u32 = 2;
pub const RELEASE: u32 = 3;
pub const AUTO_RELEASE: u32 = 4;
pub const KNEE: u32 = 5;
pub const DETECTOR: u32 = 6;
pub const SIDECHAIN_HPF: u32 = 7;
pub const LINK: u32 = 8;
pub const MAKEUP: u32 = 9;
pub const AUTO_MAKEUP: u32 = 10;
pub const MIX: u32 = 11;
pub const BYPASS: u32 = 12;
/// Read-only: how much gain the compressor is taking away, in dB at or below 0.
pub const REDUCTION: u32 = 13;

/// The `Detector` choices, by value.
pub const DETECTOR_PEAK: u32 = 0;
pub const DETECTOR_RMS: u32 = 1;

/// The `Sidechain HPF` choices, by value; `SIDECHAIN_HPF_HZ` gives each its
/// corner, with `0.0` standing for off.
pub const SIDECHAIN_HPF_OFF: u32 = 0;
pub const SIDECHAIN_HPF_HZ: [f32; 4] = [0.0, 60.0, 120.0, 250.0];
