//! Grand Piano: a sampled grand piano for MIDI keyboards.
//!
//! The library holds everything that doesn't touch the OS (SFZ parsing, sample
//! loading, the sampler engine, effects, MIDI mapping, settings, the sample
//! download and offline analysis), so it builds and tests on any platform.

pub mod analysis;
pub mod download;
pub mod dsp;
pub mod engine;
pub mod library;
pub mod mapping;
pub mod params;
pub mod settings;
pub mod sfz;

pub const APP_NAME: &str = "Grand Piano";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const ATTRIBUTION: &str = "Piano samples: Salamander Grand Piano V3 by Alexander Holm \
(Yamaha C5, 16 velocity layers, 48 kHz/24-bit), FLAC edition assembled by Roberto \
(roberto@zenvoid.org) for the FreePats project. Licensed under Creative Commons \
Attribution 3.0 (https://creativecommons.org/licenses/by/3.0/). \
Source: https://freepats.zenvoid.org/Piano/acoustic-grand-piano.html. \
The samples are used unmodified; this app trims trailing silence and \
normalises them in memory when it loads them.";
