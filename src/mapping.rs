//! MIDI controller mapping and MIDI-learn. Lock-free so the MIDI callback can
//! use it directly.

use crate::engine::Event;
use crate::params::{Params, Target, now_us};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI16, AtomicU8, Ordering};

/// Factory defaults for the M-Audio Oxygen 49 (4th generation / MKIV, USB PID 0x3102)
/// in a General MIDI preset (presets 7-10):
/// - mod wheel = CC1 (documented in the user guide)
/// - right-most fader (C9) = CC7 channel volume, left-most fader (C1) = CC74 cutoff
///   (both shown on preset 10 in M-Audio's MKIV programming walkthrough)
/// - sustain pedal = CC64
/// The knob CCs aren't listed in the manual, so the knobs default to the
/// General MIDI conventions most GM presets use (CC91 reverb, CC74/71 tone,
/// CC93 chorus); use MIDI learn if a knob doesn't respond.
pub const OXYGEN_DEFAULTS: &[(u8, Target)] = &[
    (1, Target::Modulation),
    (7, Target::Volume),
    (11, Target::Volume),
    (74, Target::Tone),
    (71, Target::Tone),
    (91, Target::Reverb),
    (93, Target::Modulation),
    (64, Target::Sustain),
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MappingEntry {
    pub cc: u8,
    pub target: Target,
}

pub struct Mapper {
    /// cc -> target id + 1 (0 = unmapped)
    table: [AtomicU8; 128],
    /// target id + 1 being learned (0 = none)
    learning: AtomicU8,
    /// last CC number received (-1 none), for the UI
    pub last_cc: AtomicI16,
    last_value: [AtomicU8; 128],
}

impl Default for Mapper {
    fn default() -> Self {
        Mapper {
            table: std::array::from_fn(|_| AtomicU8::new(0)),
            learning: AtomicU8::new(0),
            last_cc: AtomicI16::new(-1),
            last_value: std::array::from_fn(|_| AtomicU8::new(0)),
        }
    }
}

impl Mapper {
    pub fn with_defaults() -> Self {
        let m = Mapper::default();
        m.reset_to_defaults();
        m
    }
    pub fn reset_to_defaults(&self) {
        for t in self.table.iter() {
            t.store(0, Ordering::Relaxed);
        }
        for (cc, t) in OXYGEN_DEFAULTS {
            self.table[*cc as usize].store(t.id() + 1, Ordering::Relaxed);
        }
    }
    pub fn set(&self, cc: u8, target: Option<Target>) {
        self.table[cc as usize & 127].store(target.map(|t| t.id() + 1).unwrap_or(0), Ordering::Relaxed);
    }
    pub fn get(&self, cc: u8) -> Option<Target> {
        let v = self.table[cc as usize & 127].load(Ordering::Relaxed);
        if v == 0 { None } else { Target::from_id(v - 1) }
    }
    /// All CCs mapped to a target.
    pub fn ccs_for(&self, t: Target) -> Vec<u8> {
        (0..128u8).filter(|cc| self.get(*cc) == Some(t)).collect()
    }
    pub fn clear_target(&self, t: Target) {
        for cc in self.ccs_for(t) {
            self.set(cc, None);
        }
    }
    pub fn start_learn(&self, t: Target) {
        self.learning.store(t.id() + 1, Ordering::Relaxed);
    }
    pub fn cancel_learn(&self) {
        self.learning.store(0, Ordering::Relaxed);
    }
    pub fn learning(&self) -> Option<Target> {
        let v = self.learning.load(Ordering::Relaxed);
        if v == 0 { None } else { Target::from_id(v - 1) }
    }
    pub fn entries(&self) -> Vec<MappingEntry> {
        (0..128u8).filter_map(|cc| self.get(cc).map(|target| MappingEntry { cc, target })).collect()
    }
    pub fn load_entries(&self, e: &[MappingEntry]) {
        for t in self.table.iter() {
            t.store(0, Ordering::Relaxed);
        }
        for m in e {
            self.set(m.cc, Some(m.target));
        }
    }

    /// Handles one control-change message. Returns an engine event if needed.
    pub fn handle_cc(&self, params: &Params, cc: u8, value: u8) -> Option<Event> {
        let cc = cc & 127;
        let prev = self.last_value[cc as usize].swap(value, Ordering::Relaxed);
        self.last_cc.store(cc as i16, Ordering::Relaxed);
        if cc == 120 || cc == 123 {
            return Some(Event::AllNotesOff);
        }
        if let Some(t) = self.learning() {
            // A learned control takes over this CC; one control per target keeps it simple.
            self.clear_target(t);
            self.set(cc, Some(t));
            self.cancel_learn();
            if t.is_button() {
                return None; // don't also act on the learning press
            }
        }
        let t = self.get(cc)?;
        let v = value as f32 / 127.0;
        match t {
            Target::Sustain => {
                params.sustain.set(v);
                Some(Event::Sustain(v))
            }
            Target::VelocityMode
            | Target::Instrument
            | Target::Preset
            | Target::MetronomeStartStop
            | Target::TapTempo
            | Target::OctaveDown
            | Target::OctaveUp
            | Target::Voices
            | Target::Leslie => {
                // A button: each press (rising past the middle) acts once.
                // Buttons send 127 on press and 0 on release.
                if value == 127 && prev < 64 {
                    match t {
                        Target::VelocityMode => params.set_mode(params.mode().next()),
                        Target::Instrument => params.set_instrument(params.instrument().next()),
                        Target::Preset => params.next_preset(),
                        Target::MetronomeStartStop => params.toggle_metronome(),
                        Target::OctaveDown => params.step_octave(-1),
                        Target::OctaveUp => params.step_octave(1),
                        Target::Voices => params.set_voice_mode(params.voice_mode().toggled()),
                        Target::Leslie => params.toggle_leslie(),
                        _ => params.tap(now_us()),
                    }
                } else if value != 0 && value != 127 && t != Target::TapTempo {
                    // a knob/fader instead: picks a position directly
                    params.set_target(t, v);
                }
                None
            }
            other => {
                params.set_target(other, v);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::VelocityMode;

    #[test]
    fn defaults_map_oxygen_controls() {
        let m = Mapper::with_defaults();
        assert_eq!(m.get(1), Some(Target::Modulation));
        assert_eq!(m.get(7), Some(Target::Volume));
        assert_eq!(m.get(64), Some(Target::Sustain));
        assert_eq!(m.get(91), Some(Target::Reverb));
        assert_eq!(m.get(74), Some(Target::Tone));
    }

    #[test]
    fn learn_maps_next_cc() {
        let m = Mapper::with_defaults();
        let p = Params::default();
        m.start_learn(Target::Reverb);
        m.handle_cc(&p, 22, 100);
        assert_eq!(m.get(22), Some(Target::Reverb));
        assert_eq!(m.get(91), None, "previous reverb mapping replaced");
        assert!(m.learning().is_none());
        m.handle_cc(&p, 22, 127);
        assert!((p.reverb.get() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sustain_produces_engine_event() {
        let m = Mapper::with_defaults();
        let p = Params::default();
        assert_eq!(m.handle_cc(&p, 64, 127), Some(Event::Sustain(1.0)));
    }

    #[test]
    fn velocity_mode_button_cycles() {
        let m = Mapper::with_defaults();
        let p = Params::default();
        m.start_learn(Target::VelocityMode);
        m.handle_cc(&p, 113, 127); // learning press
        m.handle_cc(&p, 113, 0);
        assert_eq!(p.mode(), VelocityMode::Light);
        m.handle_cc(&p, 113, 127);
        m.handle_cc(&p, 113, 0);
        assert_eq!(p.mode(), VelocityMode::Fixed);
        m.handle_cc(&p, 113, 127);
        assert_eq!(p.mode(), VelocityMode::Normal);
    }

    #[test]
    fn instrument_and_metronome_buttons() {
        use crate::params::InstrumentKind;
        let m = Mapper::with_defaults();
        let p = Params::default();
        m.start_learn(Target::Instrument);
        m.handle_cc(&p, 102, 127); // learning press does nothing else
        m.handle_cc(&p, 102, 0);
        assert_eq!(p.instrument(), InstrumentKind::GrandPiano);
        for want in [
            InstrumentKind::MonoSynth,
            InstrumentKind::StringMachine,
            InstrumentKind::ElectricPiano,
            InstrumentKind::Organ,
            InstrumentKind::Polysynth,
            InstrumentKind::GrandPiano,
        ] {
            m.handle_cc(&p, 102, 127);
            m.handle_cc(&p, 102, 0);
            assert_eq!(p.instrument(), want);
        }
        m.set(103, Some(Target::MetronomeStartStop));
        m.handle_cc(&p, 103, 127);
        assert!(p.metro_running.load(Ordering::Relaxed));
        m.handle_cc(&p, 103, 0);
        m.handle_cc(&p, 103, 127);
        assert!(!p.metro_running.load(Ordering::Relaxed));
        m.set(20, Some(Target::Tempo));
        m.handle_cc(&p, 20, 127);
        assert_eq!(p.tempo_bpm(), 240.0);
        m.handle_cc(&p, 20, 0);
        assert_eq!(p.tempo_bpm(), 30.0);
    }

    #[test]
    fn existing_mappings_still_drive_tone_and_wheel_on_synths() {
        use crate::params::InstrumentKind;
        let m = Mapper::with_defaults();
        let p = Params::default();
        p.set_instrument(InstrumentKind::MonoSynth);
        m.handle_cc(&p, 74, 127); // Oxygen fader C1 -> Tone -> cutoff
        m.handle_cc(&p, 1, 64); // mod wheel -> vibrato
        assert_eq!(p.synth_cutoff[1].get(), 1.0);
        assert!((p.vibrato.get() - 64.0 / 127.0).abs() < 1e-6);
        assert_eq!(p.tone.get(), crate::params::DEFAULT_TONE);
    }

    #[test]
    fn voices_button_toggles_poly_and_mono() {
        use crate::synth::VoiceMode;
        let m = Mapper::with_defaults();
        let p = Params::default();
        assert_eq!(p.voice_mode(), VoiceMode::Poly);
        m.start_learn(Target::Voices);
        m.handle_cc(&p, 104, 127); // learning press does nothing else
        m.handle_cc(&p, 104, 0);
        assert_eq!(m.get(104), Some(Target::Voices));
        assert_eq!(p.voice_mode(), VoiceMode::Poly);
        m.handle_cc(&p, 104, 127);
        m.handle_cc(&p, 104, 0);
        assert_eq!(p.voice_mode(), VoiceMode::Mono);
        m.handle_cc(&p, 104, 127);
        m.handle_cc(&p, 104, 0);
        assert_eq!(p.voice_mode(), VoiceMode::Poly);
        // a fader picks directly
        m.set(21, Some(Target::Voices));
        m.handle_cc(&p, 21, 100);
        assert_eq!(p.voice_mode(), VoiceMode::Mono);
        m.handle_cc(&p, 21, 10);
        assert_eq!(p.voice_mode(), VoiceMode::Poly);
    }

    #[test]
    fn leslie_button_and_mod_wheel_on_the_organ() {
        use crate::params::InstrumentKind;
        let m = Mapper::with_defaults();
        let p = Params::default();
        p.set_instrument(InstrumentKind::Organ);
        let fast = || p.leslie_fast.load(Ordering::Relaxed);
        assert!(!fast(), "chorale at start");
        // the mod wheel (CC1): crossing 64 switches it, in both directions
        m.handle_cc(&p, 1, 40);
        assert!(!fast());
        m.handle_cc(&p, 1, 64);
        assert!(fast());
        m.handle_cc(&p, 1, 100);
        assert!(fast());
        m.handle_cc(&p, 1, 63);
        assert!(!fast());
        // a learned button toggles it; a resting wheel doesn't fight the button
        m.start_learn(Target::Leslie);
        m.handle_cc(&p, 105, 127);
        m.handle_cc(&p, 105, 0);
        assert!(!fast(), "the learning press does nothing else");
        m.handle_cc(&p, 105, 127);
        m.handle_cc(&p, 105, 0);
        assert!(fast());
        m.handle_cc(&p, 1, 20); // wheel moves but stays below the middle
        assert!(fast());
        m.handle_cc(&p, 105, 127);
        m.handle_cc(&p, 105, 0);
        assert!(!fast());
        // on the other synths the wheel is vibrato / tremolo, the Leslie isn't touched
        p.set_instrument(InstrumentKind::Polysynth);
        m.handle_cc(&p, 1, 127);
        assert!(!fast());
        assert_eq!(p.vibrato.get(), 1.0);
    }

    #[test]
    fn resonance_is_per_synth() {
        use crate::params::InstrumentKind;
        let m = Mapper::with_defaults();
        let p = Params::default();
        m.set(70, Some(Target::Resonance));
        p.set_instrument(InstrumentKind::Polysynth);
        m.handle_cc(&p, 70, 127);
        assert_eq!(p.poly_resonance.get(), 1.0);
        assert_eq!(p.resonance.get(), crate::synth::MONO_PRESETS[0].resonance, "Moog-style resonance untouched");
    }

    #[test]
    fn all_notes_off() {
        let m = Mapper::default();
        let p = Params::default();
        assert_eq!(m.handle_cc(&p, 123, 0), Some(Event::AllNotesOff));
    }
}
