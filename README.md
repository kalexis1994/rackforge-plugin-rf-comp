# RF-Comp

A feed-forward stereo compressor for [RackForge](https://github.com/kalexis1994/rackforge):
a threshold, a ratio and a soft knee, attack and release with a
programme-dependent option, a peak or RMS detector behind a high-pass
sidechain, bounded reduction, stereo link, makeup, a parallel mix, and live
input, output and gain-reduction meters. Its PLAY surface draws the transfer
curve from the same equation as the engine and lets the threshold move directly
on the graph.
The effect that holds a piano's dynamics together before the limiter catches
what is left.

> `v0.2.0` keeps every `v0.1.0` parameter index and migrates its fourteen-value
> state block. Range, detector listen and the two level meters are appended, so
> existing sessions reopen with the original unlimited reduction behaviour.

## How it decides

The sidechain reads the input — never the output, so the gain does not chase
itself — and measures it in decibels: the sample itself for *Peak*, or the
square root of a ten-millisecond mean square for *RMS*, calibrated so a sine
reads the same on both. The static curve then says how much to take: nothing
below the knee, `(1 − 1/ratio)` of the overshoot above it, and across the
knee the parabola that joins the two with the same slope, so the reduction
grows smoothly. At the threshold itself a knee takes an eighth of its width
times `(1 − 1/ratio)`.

The attack and release are one-poles on that reduction, in decibels — the
classic feed-forward design, where the ballistics follow the control signal
rather than the audio, so an attack is the same length at any level and a
reduction lands as a slope the ear reads as level. *Auto Release* averages
a fast stage (60 ms) and a slow one (600 ms): a transient is let go quickly,
a sustained passage held down. The gain is applied in the linear domain,
once per side per sample, with no latency.

*Stereo Link* is how much of the louder side each side hears: fully linked,
both get the same gain and the image stays where it was; at zero each side
has its own detector and envelope. *Sidechain HPF* is a second-order
Butterworth on the detector path only, so a bass note does not pump the
whole mix. *Sidechain Listen* routes that filtered detector signal to the
output for setup; bypass still returns the exact input. *Range* caps the
steady-state reduction before the attack and release stages, which makes a
strong ratio useful without allowing the compressor to disappear deep into a
sound. *Auto Makeup* adds half the reduction the curve would take from a
full-scale signal — the common rule — to whatever *Makeup* says. *Mix*
blends the compressed signal under the dry one for parallel compression;
either end of the knob is exact. The ratio knob's top reads `∞:1` and the
engine treats it so: a limiter's slope.

Threshold and makeup changes land over five milliseconds, so automation
does not zipper.

## Controls

| Control | Range | What it is |
| --- | --- | --- |
| Threshold | −60 … 0 dB | Where the curve starts to bend. |
| Ratio | 1 … ∞:1 | The slope above the knee. |
| Knee | 0 … 24 dB | The width over which the bend is spread. |
| Range | 0 … 60 dB | Maximum gain reduction; 60 dB preserves the original behaviour. |
| Attack | 0.1 … 100 ms | How fast the reduction sets in. |
| Release | 10 … 2000 ms | How fast it lets go, when Auto Release is off. |
| Auto Release | on/off | Programme-dependent release. |
| Detector | Peak / RMS | What the sidechain measures. |
| Sidechain HPF | Off / 60 / 120 / 250 Hz | The low end kept out of the detector. |
| Sidechain Listen | on/off | Auditions the filtered detector path while setting it. |
| Stereo Link | 0 … 100 % | How much of the louder side each side hears. |
| Makeup | −24 … +24 dB | Gain after the reduction. |
| Auto Makeup | on/off | Adds half the reduction at full scale. |
| Mix | 0 … 100 % | Compressed signal under the dry one. |
| Bypass | on/off | The input, untouched. |
| Gain Reduction | meter | How much is being taken away, in dB. |
| Input / Output | meters | Live peak levels around the compressor, in dBFS. |

## Factory settings

| Setting | Threshold | Ratio | Attack | Release | Knee | Range | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Default | −18 dB | 4:1 | 10 ms | 120 ms | 6 dB | 60 dB | RMS, linked. |
| Piano Glue | −24 dB | 2:1 | 30 ms | 200 ms, auto | 12 dB | 8 dB | Transparent cohesion. |
| Peak Tamer | −12 dB | 8:1 | 1 ms | 80 ms | 3 dB | 6 dB | Peak, sidechain HPF 60 Hz. |
| Parallel Crush | −30 dB | 10:1 | 5 ms | 150 ms | 6 dB | 12 dB | Mix 40 %, auto makeup. |
| Vocal-ish Smooth | −20 dB | 3:1 | 15 ms | 150 ms, auto | 9 dB | 8 dB | Programme release. |
| Drum Bus | −18 dB | 4:1 | 20 ms | 100 ms | 6 dB | 6 dB | Peak, sidechain HPF 60 Hz. |
| Level Rider | −28 dB | 2:1 | 40 ms | 300 ms, auto | 12 dB | 8 dB | Restrained long-term levelling. |

## Build and install

The RackForge checkout must sit next to this one, because the packager lives
there.

```bash
pwsh tools/build-package.ps1
```

```bash
bash tools/build-package.sh
```

Either script regenerates the package metadata from the contract, builds the
WebAssembly component, runs the tests and packs
`artifacts/RF-Comp.rfplugin`. Install it the way you install any RackForge
plugin — the desktop's Plugin Manager, or:

```bash
./target/release/rackforge-desktop.exe --install-plugin ../rackforge-plugin-rf-comp/artifacts/RF-Comp.rfplugin
```

In PLAY, open the FX drawer and add it after the instrument. In LIVE it is
an ordinary effect Slot.

## The bench

```bash
cargo run -p rf-comp-lab -- curve --preset piano_glue
cargo run -p rf-comp-lab -- curve --set comp.threshold=-12 --set comp.ratio=8
cargo run -p rf-comp-lab -- render --out compressed.wav --input piano.wav --preset piano_glue
```

`curve` holds a sine at every level from −60 to 0 dB in three-decibel steps
and prints what came out, what the static curve asked for, and what the
meter read; `render` puts a file, or a test tone that steps up six decibels
a second, through the compressor so it can be listened to.
