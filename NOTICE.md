# Notices

RF-Comp is an independent RackForge plugin implemented in Rust and
distributed under GPL-3.0-only.

It implements a feed-forward compressor from first principles: a peak or
mean-square level detector, a second-order Butterworth sidechain high-pass,
a log-domain gain computer with a quadratic knee, and one-pole attack and
release on the reduction. These are textbook signal-processing
constructions; no third-party code, artwork, trademark or brand name is
included in this repository or in the plugin package.

Third-party code: the plugin depends on `rackforge-plugin-sdk` (MIT OR
Apache-2.0) and on `libm` (MIT/Apache-2.0).
