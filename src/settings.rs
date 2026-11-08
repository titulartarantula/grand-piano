//! Settings and mappings, saved as JSON in %APPDATA%\GrandPiano\settings.json.

use crate::mapping::{Mapper, MappingEntry};
use crate::params::{Params, VelocityMode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioMode {
    /// Lowest latency available: low-latency shared mode if the driver supports
    /// small periods, otherwise exclusive mode, otherwise standard shared mode.
    Auto,
    /// WASAPI shared mode with the smallest engine period the driver allows
    /// (IAudioClient3). Other apps keep playing sound.
    SharedLowLatency,
    /// WASAPI exclusive mode: lowest latency, takes over the device.
    Exclusive,
    /// Plain WASAPI shared mode (10 ms engine period); most compatible.
    Shared,
}

impl AudioMode {
    pub const ALL: [AudioMode; 4] = [AudioMode::Auto, AudioMode::SharedLowLatency, AudioMode::Exclusive, AudioMode::Shared];
    pub fn label(self) -> &'static str {
        match self {
            AudioMode::Auto => "Auto (lowest latency)",
            AudioMode::SharedLowLatency => "Shared, low latency",
            AudioMode::Exclusive => "Exclusive",
            AudioMode::Shared => "Shared, standard",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub volume: f32,
    pub tone: f32,
    pub reverb: f32,
    pub modulation: f32,
    pub touch_normal: f32,
    pub touch_light: f32,
    pub fixed_velocity: f32,
    pub velocity_mode: VelocityMode,
    /// WASAPI endpoint id; None = Windows default output
    pub audio_device: Option<String>,
    pub audio_mode: AudioMode,
    /// requested buffer in frames (exclusive mode, and the target for low-latency shared)
    pub buffer_frames: u32,
    /// preferred MIDI input port name; None = auto (Oxygen, else first port)
    pub midi_port: Option<String>,
    pub mappings: Vec<MappingEntry>,
    pub samples_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        let m = Mapper::with_defaults();
        Settings {
            volume: crate::params::DEFAULT_VOLUME,
            tone: crate::params::DEFAULT_TONE,
            reverb: crate::params::DEFAULT_REVERB,
            modulation: crate::params::DEFAULT_MOD,
            touch_normal: crate::params::DEFAULT_TOUCH,
            touch_light: crate::params::DEFAULT_TOUCH,
            fixed_velocity: crate::params::DEFAULT_FIXED,
            velocity_mode: VelocityMode::Light,
            audio_device: None,
            audio_mode: AudioMode::Auto,
            buffer_frames: 128,
            midi_port: None,
            mappings: m.entries(),
            samples_dir: None,
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("GrandPiano")
}

/// Where samples live: %LOCALAPPDATA%\GrandPiano\samples (large, machine-local).
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("GrandPiano")
}

pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

impl Settings {
    pub fn load() -> Settings {
        match std::fs::read_to_string(settings_path()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
                log::warn!("settings.json unreadable ({e}); using defaults");
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let dir = config_dir();
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join("settings.json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, settings_path())?;
        Ok(())
    }

    pub fn apply_to(&self, p: &Params, m: &Mapper) {
        p.volume.set(self.volume);
        p.tone.set(self.tone);
        p.reverb.set(self.reverb);
        p.modulation.set(self.modulation);
        p.touch_normal.set(self.touch_normal);
        p.touch_light.set(self.touch_light);
        p.fixed_velocity.set(self.fixed_velocity.clamp(1.0, 127.0));
        p.set_mode(self.velocity_mode);
        m.load_entries(&self.mappings);
    }

    pub fn capture(&mut self, p: &Params, m: &Mapper) {
        self.volume = p.volume.get();
        self.tone = p.tone.get();
        self.reverb = p.reverb.get();
        self.modulation = p.modulation.get();
        self.touch_normal = p.touch_normal.get();
        self.touch_light = p.touch_light.get();
        self.fixed_velocity = p.fixed_velocity.get();
        self.velocity_mode = p.mode();
        self.mappings = m.entries();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_json() {
        let mut s = Settings::default();
        s.velocity_mode = VelocityMode::Fixed;
        s.audio_device = Some("{0.0.0.00000000}.{abc}".into());
        let j = serde_json::to_string(&s).unwrap();
        let t: Settings = serde_json::from_str(&j).unwrap();
        assert_eq!(t.velocity_mode, VelocityMode::Fixed);
        assert_eq!(t.mappings, s.mappings);
        // unknown / missing fields fall back to defaults
        let partial: Settings = serde_json::from_str(r#"{"volume":0.5}"#).unwrap();
        assert_eq!(partial.volume, 0.5);
        assert_eq!(partial.buffer_frames, 128);
    }
}
