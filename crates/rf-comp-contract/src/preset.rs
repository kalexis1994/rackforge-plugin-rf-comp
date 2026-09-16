//! Factory settings.
//!
//! A preset here is nothing but a list of parameter values. The packager
//! renders the same table into `metadata/presets.json`, so the catalog
//! RackForge shows and the settings the engine loads cannot disagree.

use crate::Settings;
use crate::index::*;

pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub values: &'static [(u32, f32)],
}

pub const PRESET_COUNT: usize = 7;

pub const PRESETS: [Preset; PRESET_COUNT] = [
    Preset {
        id: "default",
        name: "Default",
        description: "Four to one over a six-decibel knee at minus eighteen: a general-purpose starting point.",
        values: &[],
    },
    Preset {
        id: "piano_glue",
        name: "Piano Glue",
        description: "Two to one over a wide knee, a slow attack and the release left to the programme: holds a piano together without touching its transients.",
        values: &[
            (THRESHOLD, -24.0),
            (RATIO, 2.0),
            (ATTACK, 30.0),
            (RELEASE, 200.0),
            (AUTO_RELEASE, 1.0),
            (KNEE, 12.0),
            (DETECTOR, DETECTOR_RMS as f32),
            (MIX, 100.0),
            (RANGE, 8.0),
        ],
    },
    Preset {
        id: "peak_tamer",
        name: "Peak Tamer",
        description: "Eight to one, a millisecond of attack, a peak detector with the low end filtered out of it: catches what sticks out.",
        values: &[
            (THRESHOLD, -12.0),
            (RATIO, 8.0),
            (ATTACK, 1.0),
            (RELEASE, 80.0),
            (KNEE, 3.0),
            (DETECTOR, DETECTOR_PEAK as f32),
            (SIDECHAIN_HPF, 1.0),
            (RANGE, 6.0),
        ],
    },
    Preset {
        id: "parallel_crush",
        name: "Parallel Crush",
        description: "Ten to one from minus thirty, made up automatically and blended at forty percent: density under the dry signal.",
        values: &[
            (THRESHOLD, -30.0),
            (RATIO, 10.0),
            (ATTACK, 5.0),
            (RELEASE, 150.0),
            (KNEE, 6.0),
            (MIX, 40.0),
            (AUTO_MAKEUP, 1.0),
            (RANGE, 12.0),
        ],
    },
    Preset {
        id: "vocal_smooth",
        name: "Vocal-ish Smooth",
        description: "Three to one over a nine-decibel knee, the release following the programme: level without a hand on the fader.",
        values: &[
            (THRESHOLD, -20.0),
            (RATIO, 3.0),
            (ATTACK, 15.0),
            (RELEASE, 150.0),
            (AUTO_RELEASE, 1.0),
            (KNEE, 9.0),
            (RANGE, 8.0),
        ],
    },
    Preset {
        id: "drum_bus",
        name: "Drum Bus",
        description: "Four to one with a twenty-millisecond attack and six decibels of range: adds movement while keeping the first hit intact.",
        values: &[
            (THRESHOLD, -18.0),
            (RATIO, 4.0),
            (ATTACK, 20.0),
            (RELEASE, 100.0),
            (KNEE, 6.0),
            (DETECTOR, DETECTOR_PEAK as f32),
            (SIDECHAIN_HPF, 1.0),
            (RANGE, 6.0),
        ],
    },
    Preset {
        id: "level_rider",
        name: "Level Rider",
        description: "Two to one over a broad knee with automatic release and eight decibels of range: restrained long-term levelling.",
        values: &[
            (THRESHOLD, -28.0),
            (RATIO, 2.0),
            (ATTACK, 40.0),
            (RELEASE, 300.0),
            (AUTO_RELEASE, 1.0),
            (KNEE, 12.0),
            (RANGE, 8.0),
        ],
    },
];

/// The settings a preset describes: the defaults, with the preset's values
/// over them. `None` for an id no preset carries.
pub fn settings_for(id: &str) -> Option<Settings> {
    let preset = PRESETS.iter().find(|preset| preset.id == id)?;
    let mut settings = Settings::default();
    for (index, value) in preset.values {
        if !settings.set(*index, *value as f64) {
            return None;
        }
    }
    Some(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_loads_and_stays_inside_the_contract() {
        for preset in PRESETS.iter() {
            assert!(
                settings_for(preset.id).is_some(),
                "{} does not load",
                preset.id
            );
        }
        assert!(settings_for("nowhere").is_none());
    }

    #[test]
    fn preset_identifiers_are_unique() {
        for (position, preset) in PRESETS.iter().enumerate() {
            for other in &PRESETS[position + 1..] {
                assert_ne!(preset.id, other.id);
            }
        }
    }

    #[test]
    fn the_default_preset_is_the_defaults() {
        let preset = settings_for("default").unwrap();
        assert_eq!(preset.as_array(), Settings::default().as_array());
    }
}
