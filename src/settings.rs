//! Settings and mappings, saved as JSON in %APPDATA%\GrandPiano\settings.json.

use crate::mapping::{Mapper, MappingEntry};
use crate::params::{InstrumentKind, Params, VelocityMode};
use std::sync::atomic::Ordering;
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
    // ---- instruments (added later; older files fall back to these defaults)
    pub instrument: InstrumentKind,
    pub mono_preset: u8,
    pub strings_preset: u8,
    /// filter cutoff knob of each synth
    pub mono_cutoff: f32,
    pub strings_cutoff: f32,
    pub mono_resonance: f32,
    pub mono_glide: f32,
    /// Moog-style synth Voices switch. Files from before it existed load as
    /// Poly, whatever the preset (the Bass and Lead presets used to be mono-only).
    pub mono_voices: crate::synth::VoiceMode,
    /// note priority in Mono voice mode
    pub mono_priority: crate::synth::NotePriority,
    /// mod wheel position for the synths (vibrato)
    pub synth_vibrato: f32,
    // ---- pitch bend: range in semitones per instrument (piano, mono, strings)
    pub bend_up: [u8; 3],
    pub bend_down: [u8; 3],
    /// the piano ignores the pitch wheel unless this is on
    pub piano_bend: bool,
    // ---- the app's octave shift (-3..+3) and transpose (-12..+12 semitones)
    pub octave: i32,
    pub transpose: i32,
    // ---- metronome (running/stopped is not saved: it is always off at launch)
    pub tempo: f32,
    pub beats_per_bar: u8,
    pub click_on: bool,
    pub blink_on: bool,
    pub click_level: f32,
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
            instrument: InstrumentKind::GrandPiano,
            mono_preset: 0,
            strings_preset: 0,
            mono_cutoff: crate::synth::MONO_PRESETS[0].cutoff,
            strings_cutoff: crate::strings::STRING_PRESETS[0].cutoff,
            mono_resonance: crate::synth::MONO_PRESETS[0].resonance,
            mono_glide: crate::synth::MONO_PRESETS[0].glide,
            mono_voices: crate::synth::VoiceMode::Poly,
            mono_priority: crate::synth::NotePriority::Last,
            synth_vibrato: 0.0,
            bend_up: [crate::params::DEFAULT_BEND_RANGE; 3],
            bend_down: [crate::params::DEFAULT_BEND_RANGE; 3],
            piano_bend: false,
            octave: 0,
            transpose: 0,
            tempo: crate::params::DEFAULT_TEMPO,
            beats_per_bar: crate::params::DEFAULT_BEATS_PER_BAR,
            click_on: true,
            blink_on: true,
            click_level: crate::params::DEFAULT_CLICK_LEVEL,
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
        p.set_instrument(self.instrument);
        p.preset[InstrumentKind::MonoSynth.id() as usize].store(self.mono_preset.min(2), Ordering::Relaxed);
        p.preset[InstrumentKind::StringMachine.id() as usize].store(self.strings_preset.min(2), Ordering::Relaxed);
        p.synth_cutoff[1].set(self.mono_cutoff.clamp(0.0, 1.0));
        p.synth_cutoff[2].set(self.strings_cutoff.clamp(0.0, 1.0));
        p.resonance.set(self.mono_resonance.clamp(0.0, 1.0));
        p.glide.set(self.mono_glide.clamp(0.0, 1.0));
        p.set_voice_mode(self.mono_voices);
        p.set_note_priority(self.mono_priority);
        p.vibrato.set(self.synth_vibrato.clamp(0.0, 1.0));
        for i in 0..3 {
            p.bend_up[i].store(self.bend_up[i].min(crate::params::MAX_BEND_RANGE), Ordering::Relaxed);
            p.bend_down[i].store(self.bend_down[i].min(crate::params::MAX_BEND_RANGE), Ordering::Relaxed);
        }
        p.piano_bend.store(self.piano_bend, Ordering::Relaxed);
        p.octave.store(self.octave.clamp(crate::keyrange::OCTAVE_MIN, crate::keyrange::OCTAVE_MAX), Ordering::Relaxed);
        p.transpose.store(self.transpose.clamp(-crate::keyrange::TRANSPOSE_MAX, crate::keyrange::TRANSPOSE_MAX), Ordering::Relaxed);
        p.set_tempo_bpm(self.tempo);
        p.beats_per_bar.store(self.beats_per_bar.clamp(2, 6), Ordering::Relaxed);
        p.click_on.store(self.click_on, Ordering::Relaxed);
        p.blink_on.store(self.blink_on, Ordering::Relaxed);
        p.click_level.set(self.click_level.clamp(0.0, 1.0));
        p.metro_running.store(false, Ordering::Relaxed);
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
        self.instrument = p.instrument();
        self.mono_preset = p.preset_of(InstrumentKind::MonoSynth) as u8;
        self.strings_preset = p.preset_of(InstrumentKind::StringMachine) as u8;
        self.mono_cutoff = p.synth_cutoff[1].get();
        self.strings_cutoff = p.synth_cutoff[2].get();
        self.mono_resonance = p.resonance.get();
        self.mono_glide = p.glide.get();
        self.mono_voices = p.voice_mode();
        self.mono_priority = p.note_priority();
        self.synth_vibrato = p.vibrato.get();
        for i in 0..3 {
            self.bend_up[i] = p.bend_up[i].load(Ordering::Relaxed);
            self.bend_down[i] = p.bend_down[i].load(Ordering::Relaxed);
        }
        self.piano_bend = p.piano_bend.load(Ordering::Relaxed);
        self.octave = p.octave.load(Ordering::Relaxed);
        self.transpose = p.transpose.load(Ordering::Relaxed);
        self.tempo = p.tempo_bpm();
        self.beats_per_bar = p.beats_per_bar.load(Ordering::Relaxed);
        self.click_on = p.click_on.load(Ordering::Relaxed);
        self.blink_on = p.blink_on.load(Ordering::Relaxed);
        self.click_level = p.click_level.get();
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
        // a settings file from before the synths: piano, metronome defaults
        assert_eq!(partial.instrument, InstrumentKind::GrandPiano);
        assert_eq!(partial.tempo, 100.0);
        assert_eq!(partial.beats_per_bar, 4);
        assert_eq!(partial.bend_up, [2, 2, 2]);
        assert!(!partial.piano_bend);
    }

    #[test]
    fn settings_from_before_the_voices_switch_load_as_poly() {
        use crate::synth::{NotePriority, VoiceMode};
        // a settings file from before the switch: Bass preset (mono-only then)
        let old = r#"{"volume":0.8,"instrument":"MonoSynth","mono_preset":0,"mono_cutoff":0.36,"mono_glide":0.0}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(s.mono_voices, VoiceMode::Poly);
        assert_eq!(s.mono_priority, NotePriority::Last);
        let p = Params::default();
        p.set_voice_mode(VoiceMode::Mono);
        s.apply_to(&p, &Mapper::with_defaults());
        assert_eq!(p.voice_mode(), VoiceMode::Poly);
        assert_eq!(p.preset_of(InstrumentKind::MonoSynth), 0);
        // and the new fields round-trip
        p.set_voice_mode(VoiceMode::Mono);
        p.set_note_priority(NotePriority::Low);
        let mut t = s.clone();
        t.capture(&p, &Mapper::with_defaults());
        let j = serde_json::to_string(&t).unwrap();
        assert!(j.contains(r#""mono_voices":"Mono""#) && j.contains(r#""mono_priority":"Low""#), "{j}");
        let u: Settings = serde_json::from_str(&j).unwrap();
        assert_eq!((u.mono_voices, u.mono_priority), (VoiceMode::Mono, NotePriority::Low));
    }

    #[test]
    fn instrument_and_metronome_settings_roundtrip() {
        let p = Params::default();
        let m = Mapper::with_defaults();
        p.set_instrument(InstrumentKind::StringMachine);
        p.select_preset(InstrumentKind::StringMachine, 1);
        p.set_tempo_bpm(87.0);
        p.beats_per_bar.store(3, Ordering::Relaxed);
        p.click_on.store(false, Ordering::Relaxed);
        p.metro_running.store(true, Ordering::Relaxed);
        let mut s = Settings::default();
        s.capture(&p, &m);
        let j = serde_json::to_string(&s).unwrap();
        assert!(!j.contains("running"), "running state isn't saved");
        let t: Settings = serde_json::from_str(&j).unwrap();
        let q = Params::default();
        t.apply_to(&q, &m);
        assert_eq!(q.instrument(), InstrumentKind::StringMachine);
        assert_eq!(q.preset_of(InstrumentKind::StringMachine), 1);
        assert_eq!(q.tempo_bpm(), 87.0);
        assert_eq!(q.beats_per_bar.load(Ordering::Relaxed), 3);
        assert!(!q.click_on.load(Ordering::Relaxed));
        assert!(!q.metro_running.load(Ordering::Relaxed), "metronome is off at launch");
    }
}
