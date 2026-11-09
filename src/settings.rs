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
    // ---- electric piano, organ, polysynth (added later; older files load these defaults)
    pub ep_preset: u8,
    pub organ_preset: u8,
    pub poly_preset: u8,
    /// Tone knob of each: brightness, drive, filter cutoff
    pub ep_tone: f32,
    pub organ_drive: f32,
    pub poly_cutoff: f32,
    pub poly_resonance: f32,
    /// organ drawbars (16' first, 0..8) and percussion switches
    pub organ_drawbars: [u8; 9],
    pub organ_perc: bool,
    pub organ_perc_third: bool,
    pub organ_perc_fast: bool,
    pub organ_perc_soft: bool,
    /// Leslie speed: false = chorale (slow), true = tremolo (fast)
    pub leslie_fast: bool,
    // ---- pitch bend: range in semitones per instrument, in selector order
    // (piano, Moog-style, strings, electric piano, organ, polysynth). Older
    // files have three entries; the rest get their defaults (organ: off).
    pub bend_up: Vec<u8>,
    pub bend_down: Vec<u8>,
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
            ep_preset: 0,
            organ_preset: 0,
            poly_preset: 0,
            ep_tone: crate::ep::EP_PRESETS[0].tone,
            organ_drive: crate::organ::ORGAN_PRESETS[0].drive,
            poly_cutoff: crate::poly::POLY_PRESETS[0].cutoff,
            poly_resonance: crate::poly::POLY_PRESETS[0].resonance,
            organ_drawbars: crate::organ::ORGAN_PRESETS[0].drawbars,
            organ_perc: crate::organ::ORGAN_PRESETS[0].perc,
            organ_perc_third: crate::organ::ORGAN_PRESETS[0].perc_third,
            organ_perc_fast: crate::organ::ORGAN_PRESETS[0].perc_fast,
            organ_perc_soft: crate::organ::ORGAN_PRESETS[0].perc_soft,
            leslie_fast: false,
            bend_up: InstrumentKind::ALL.iter().map(|i| i.default_bend()).collect(),
            bend_down: InstrumentKind::ALL.iter().map(|i| i.default_bend()).collect(),
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
        p.preset[InstrumentKind::ElectricPiano.id() as usize].store(self.ep_preset.min(crate::ep::EP_PRESETS.len() as u8 - 1), Ordering::Relaxed);
        p.preset[InstrumentKind::Organ.id() as usize].store(self.organ_preset.min(crate::organ::ORGAN_PRESETS.len() as u8 - 1), Ordering::Relaxed);
        p.preset[InstrumentKind::Polysynth.id() as usize].store(self.poly_preset.min(crate::poly::POLY_PRESETS.len() as u8 - 1), Ordering::Relaxed);
        p.synth_cutoff[3].set(self.ep_tone.clamp(0.0, 1.0));
        p.synth_cutoff[4].set(self.organ_drive.clamp(0.0, 1.0));
        p.synth_cutoff[5].set(self.poly_cutoff.clamp(0.0, 1.0));
        p.poly_resonance.set(self.poly_resonance.clamp(0.0, 1.0));
        for (d, v) in p.drawbars.iter().zip(self.organ_drawbars) {
            d.store(v.min(8), Ordering::Relaxed);
        }
        p.perc_on.store(self.organ_perc, Ordering::Relaxed);
        p.perc_third.store(self.organ_perc_third, Ordering::Relaxed);
        p.perc_fast.store(self.organ_perc_fast, Ordering::Relaxed);
        p.perc_soft.store(self.organ_perc_soft, Ordering::Relaxed);
        p.leslie_fast.store(self.leslie_fast, Ordering::Relaxed);
        p.synth_cutoff[1].set(self.mono_cutoff.clamp(0.0, 1.0));
        p.synth_cutoff[2].set(self.strings_cutoff.clamp(0.0, 1.0));
        p.resonance.set(self.mono_resonance.clamp(0.0, 1.0));
        p.glide.set(self.mono_glide.clamp(0.0, 1.0));
        p.set_voice_mode(self.mono_voices);
        p.set_note_priority(self.mono_priority);
        p.vibrato.set(self.synth_vibrato.clamp(0.0, 1.0));
        for (i, inst) in InstrumentKind::ALL.iter().enumerate() {
            let up = self.bend_up.get(i).copied().unwrap_or(inst.default_bend());
            let down = self.bend_down.get(i).copied().unwrap_or(inst.default_bend());
            p.bend_up[i].store(up.min(crate::params::MAX_BEND_RANGE), Ordering::Relaxed);
            p.bend_down[i].store(down.min(crate::params::MAX_BEND_RANGE), Ordering::Relaxed);
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
        self.ep_preset = p.preset_of(InstrumentKind::ElectricPiano) as u8;
        self.organ_preset = p.preset_of(InstrumentKind::Organ) as u8;
        self.poly_preset = p.preset_of(InstrumentKind::Polysynth) as u8;
        self.ep_tone = p.synth_cutoff[3].get();
        self.organ_drive = p.synth_cutoff[4].get();
        self.poly_cutoff = p.synth_cutoff[5].get();
        self.poly_resonance = p.poly_resonance.get();
        self.organ_drawbars = std::array::from_fn(|i| p.drawbars[i].load(Ordering::Relaxed));
        self.organ_perc = p.perc_on.load(Ordering::Relaxed);
        self.organ_perc_third = p.perc_third.load(Ordering::Relaxed);
        self.organ_perc_fast = p.perc_fast.load(Ordering::Relaxed);
        self.organ_perc_soft = p.perc_soft.load(Ordering::Relaxed);
        self.leslie_fast = p.leslie_fast.load(Ordering::Relaxed);
        self.bend_up = p.bend_up.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        self.bend_down = p.bend_down.iter().map(|x| x.load(Ordering::Relaxed)).collect();
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
        assert_eq!(partial.bend_up, vec![2, 2, 2, 2, 0, 2], "organ bend is off by default");
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
    fn older_files_with_three_bend_ranges_still_load() {
        let old = r#"{"volume":0.8,"bend_up":[2,5,7],"bend_down":[2,12,3],"instrument":"StringMachine"}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(s.instrument, InstrumentKind::StringMachine, "the rest of the file isn't lost");
        let p = Params::default();
        s.apply_to(&p, &Mapper::with_defaults());
        let up: Vec<u8> = p.bend_up.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        let down: Vec<u8> = p.bend_down.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        assert_eq!(up, vec![2, 5, 7, 2, 0, 2]);
        assert_eq!(down, vec![2, 12, 3, 2, 0, 2]);
    }

    #[test]
    fn new_instrument_settings_roundtrip() {
        let p = Params::default();
        let m = Mapper::with_defaults();
        p.set_instrument(InstrumentKind::Organ);
        p.select_preset(InstrumentKind::Organ, 1); // Gospel: percussion on
        p.drawbars[8].store(5, Ordering::Relaxed);
        p.leslie_fast.store(true, Ordering::Relaxed);
        p.select_preset(InstrumentKind::Polysynth, 4);
        p.poly_resonance.set(0.77);
        p.select_preset(InstrumentKind::ElectricPiano, 2);
        p.synth_cutoff[3].set(0.9);
        let mut s = Settings::default();
        s.capture(&p, &m);
        let t: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        let q = Params::default();
        t.apply_to(&q, &m);
        assert_eq!(q.instrument(), InstrumentKind::Organ);
        assert_eq!(q.preset_of(InstrumentKind::Organ), 1);
        assert!(q.perc_on.load(Ordering::Relaxed));
        assert_eq!(q.drawbars[8].load(Ordering::Relaxed), 5);
        assert!(q.leslie_fast.load(Ordering::Relaxed));
        assert_eq!(q.preset_of(InstrumentKind::Polysynth), 4);
        assert!((q.poly_resonance.get() - 0.77).abs() < 1e-6);
        assert_eq!(q.preset_of(InstrumentKind::ElectricPiano), 2);
        assert!((q.synth_cutoff[3].get() - 0.9).abs() < 1e-6);
        // and the Moog-style synth's resonance is separate from the polysynth's
        assert_eq!(q.resonance.get(), crate::synth::MONO_PRESETS[0].resonance);
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
