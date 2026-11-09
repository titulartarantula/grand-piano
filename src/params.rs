//! Parameters shared between the UI, the MIDI thread and the audio thread.
//! Everything is lock-free (atomics), so the audio thread never blocks.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

#[derive(Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }
    #[inline]
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    #[inline]
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed)
    }
}

/// Things a MIDI controller can be mapped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Target {
    Volume,
    Tone,
    Reverb,
    Modulation,
    Touch,
    Sustain,
    VelocityMode,
    // Added with the synths and metronome. New targets go at the end so
    // existing settings files keep their meaning.
    /// Button: steps to the next instrument (a knob/fader picks one).
    Instrument,
    /// Button: steps to the next preset of the current instrument.
    Preset,
    /// Mono synth glide (portamento) time.
    Glide,
    /// Mono synth filter resonance.
    Resonance,
    /// Button: metronome start/stop.
    MetronomeStartStop,
    /// Metronome tempo, 30..240 BPM.
    Tempo,
    /// Button: tap tempo.
    TapTempo,
    /// Button: the app's octave shift down / up (-3..+3).
    OctaveDown,
    OctaveUp,
    /// Button: the Moog-style synth's Voices switch, Poly <-> Mono (a knob/fader picks one).
    Voices,
    /// Button: the organ's rotating speaker, slow (chorale) <-> fast (tremolo).
    Leslie,
}

impl Target {
    pub const ALL: [Target; 18] = [
        Target::Volume,
        Target::Tone,
        Target::Reverb,
        Target::Modulation,
        Target::Touch,
        Target::Sustain,
        Target::VelocityMode,
        Target::Instrument,
        Target::Preset,
        Target::Glide,
        Target::Resonance,
        Target::MetronomeStartStop,
        Target::Tempo,
        Target::TapTempo,
        Target::OctaveDown,
        Target::OctaveUp,
        Target::Voices,
        Target::Leslie,
    ];
    pub fn id(self) -> u8 {
        Target::ALL.iter().position(|t| *t == self).unwrap() as u8
    }
    pub fn from_id(id: u8) -> Option<Target> {
        Target::ALL.get(id as usize).copied()
    }
    pub fn label(self) -> &'static str {
        match self {
            Target::Volume => "Volume",
            Target::Tone => "Tone",
            Target::Reverb => "Room",
            Target::Modulation => "Shimmer",
            Target::Touch => "Touch",
            Target::Sustain => "Sustain",
            Target::VelocityMode => "Velocity mode",
            Target::Instrument => "Instrument",
            Target::Preset => "Preset",
            Target::Glide => "Glide",
            Target::Resonance => "Resonance",
            Target::MetronomeStartStop => "Metronome start/stop",
            Target::Tempo => "Tempo",
            Target::TapTempo => "Tap tempo",
            Target::OctaveDown => "Octave down",
            Target::OctaveUp => "Octave up",
            Target::Voices => "Synth voices",
            Target::Leslie => "Leslie slow/fast",
        }
    }
    /// Targets driven by a button press (127 = press) rather than a continuous value.
    pub fn is_button(self) -> bool {
        matches!(
            self,
            Target::VelocityMode
                | Target::Instrument
                | Target::Preset
                | Target::MetronomeStartStop
                | Target::TapTempo
                | Target::OctaveDown
                | Target::OctaveUp
                | Target::Voices
                | Target::Leslie
        )
    }
}

/// The instruments on the main screen, in selector order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstrumentKind {
    GrandPiano,
    /// Real-time Moog-style subtractive synth (3 oscillators, 24 dB ladder
    /// filter), poly or mono with glide. (The variant name is kept so existing
    /// settings files still load.)
    MonoSynth,
    /// Real-time divide-down string ensemble with ensemble chorus and phaser.
    StringMachine,
    /// Real-time tine / reed electric piano (pickup model, tremolo).
    ElectricPiano,
    /// Real-time tonewheel organ with percussion and a rotating speaker.
    Organ,
    /// Real-time 10-voice polysynth (Juno-, Prophet- and Oberheim-style voices).
    Polysynth,
}

/// Number of instruments (array sizes for per-instrument settings).
pub const N_INSTRUMENTS: usize = 6;

impl InstrumentKind {
    pub const ALL: [InstrumentKind; N_INSTRUMENTS] = [
        InstrumentKind::GrandPiano,
        InstrumentKind::MonoSynth,
        InstrumentKind::StringMachine,
        InstrumentKind::ElectricPiano,
        InstrumentKind::Organ,
        InstrumentKind::Polysynth,
    ];
    pub fn id(self) -> u8 {
        match self {
            InstrumentKind::GrandPiano => 0,
            InstrumentKind::MonoSynth => 1,
            InstrumentKind::StringMachine => 2,
            InstrumentKind::ElectricPiano => 3,
            InstrumentKind::Organ => 4,
            InstrumentKind::Polysynth => 5,
        }
    }
    pub fn from_id(id: u8) -> InstrumentKind {
        InstrumentKind::ALL.get(id as usize).copied().unwrap_or(InstrumentKind::GrandPiano)
    }
    pub fn next(self) -> InstrumentKind {
        InstrumentKind::from_id((self.id() + 1) % N_INSTRUMENTS as u8)
    }
    /// Short name for the selector.
    pub fn label(self) -> &'static str {
        match self {
            InstrumentKind::GrandPiano => "Grand Piano",
            InstrumentKind::MonoSynth => "Moog-style Synth",
            InstrumentKind::StringMachine => "String Machine",
            InstrumentKind::ElectricPiano => "Electric Piano",
            InstrumentKind::Organ => "Organ",
            InstrumentKind::Polysynth => "Polysynth",
        }
    }
    /// Descriptive subtitle.
    pub fn description(self) -> &'static str {
        match self {
            InstrumentKind::GrandPiano => "Salamander Grand · Yamaha C5",
            InstrumentKind::MonoSynth => "Moog-style Synth · 3 oscillators · 24 dB ladder filter · Poly (chords) or Mono (legato, glide)",
            InstrumentKind::StringMachine => "String Machine · late-'70s divide-down string ensemble",
            InstrumentKind::ElectricPiano => "Electric Piano · tine (Rhodes-style) and reed (Wurlitzer-style) model with pickup bark and tremolo",
            InstrumentKind::Organ => "Organ · B3-style tonewheels, 9 drawbars, percussion and a Leslie-style rotating speaker",
            InstrumentKind::Polysynth => "Polysynth · Juno-, Prophet- and Oberheim-style voices · 10 voices",
        }
    }
    pub fn is_synth(self) -> bool {
        self != InstrumentKind::GrandPiano
    }
    pub fn presets(self) -> &'static [&'static str] {
        match self {
            InstrumentKind::GrandPiano => &[],
            InstrumentKind::MonoSynth => &["Bass", "Lead", "Poly"],
            InstrumentKind::StringMachine => &["Strings", "Phased", "Dark"],
            InstrumentKind::ElectricPiano => &["Suitcase", "Bright Tines", "Wurli"],
            InstrumentKind::Organ => &["Jazz", "Gospel", "Rock", "Ballad"],
            InstrumentKind::Polysynth => &["80s Pad", "Brass Stab", "Synthwave Lead", "Juno Strings", "OB Brass"],
        }
    }
    /// Whether velocity changes the sound (the organ's keys are on/off).
    pub fn velocity_sensitive(self) -> bool {
        self != InstrumentKind::Organ
    }
    /// Default pitch-bend range in semitones (the organ's is off, like the original).
    pub fn default_bend(self) -> u8 {
        if self == InstrumentKind::Organ { 0 } else { DEFAULT_BEND_RANGE }
    }
}

pub const DEFAULT_BEND_RANGE: u8 = 2;
pub const MAX_BEND_RANGE: u8 = 12;

/// MIDI pitch-bend message data bytes (LSB, MSB) -> -1..+1. The 14-bit centre
/// 8192 is exactly 0; 0 is -1 and 16383 is +1.
pub fn bend_from_14bit(lsb: u8, msb: u8) -> f32 {
    let v = ((msb as i32 & 0x7F) << 7) | (lsb as i32 & 0x7F);
    let d = v - 8192;
    if d < 0 { d as f32 / 8192.0 } else { d as f32 / 8191.0 }
}

pub const TEMPO_MIN: f32 = 30.0;
pub const TEMPO_MAX: f32 = 240.0;
pub const DEFAULT_TEMPO: f32 = 100.0;
pub const DEFAULT_BEATS_PER_BAR: u8 = 4;
pub const DEFAULT_CLICK_LEVEL: f32 = 0.6;

/// Microseconds since the first call (a process-wide monotonic clock shared by
/// the audio thread and the UI, used to line the beat light up with the click).
pub fn now_us() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_micros() as u64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VelocityMode {
    /// Standard: what the keyboard sends, shaped by the Touch knob (centre = linear).
    Normal,
    /// Easy: soft playing still gives medium-loud notes (concave curve + raised floor).
    Light,
    /// Every note at one level, set by the Touch knob.
    Fixed,
}

impl VelocityMode {
    pub const ALL: [VelocityMode; 3] = [VelocityMode::Normal, VelocityMode::Light, VelocityMode::Fixed];
    pub fn id(self) -> u8 {
        match self {
            VelocityMode::Normal => 0,
            VelocityMode::Light => 1,
            VelocityMode::Fixed => 2,
        }
    }
    pub fn from_id(id: u8) -> VelocityMode {
        match id {
            0 => VelocityMode::Normal,
            2 => VelocityMode::Fixed,
            _ => VelocityMode::Light,
        }
    }
    pub fn next(self) -> VelocityMode {
        VelocityMode::from_id((self.id() + 1) % 3)
    }
    pub fn label(self) -> &'static str {
        match self {
            VelocityMode::Normal => "Normal",
            VelocityMode::Light => "Light",
            VelocityMode::Fixed => "Fixed",
        }
    }
}

/// Velocity curve. `touch` is the Touch knob (0..1) for Normal/Light; `fixed` is the
/// Fixed-mode level (1..127). Returns 1..=127.
pub fn shape_velocity(mode: VelocityMode, touch: f32, fixed: f32, vel: u8) -> u8 {
    let v = (vel.max(1) as f32) / 127.0;
    let out = match mode {
        VelocityMode::Normal => {
            // touch 0.5 = linear; higher = lighter (concave), lower = heavier (convex)
            let gamma = 2f32.powf((0.5 - touch) * 2.0);
            127.0 * v.powf(gamma)
        }
        VelocityMode::Light => {
            // touch 0.5 (default): floor 50, gamma 0.5. Range: floor 32..68, gamma 0.75..0.25
            let floor = 32.0 + 36.0 * touch;
            let gamma = 0.75 - 0.5 * touch;
            floor + (127.0 - floor) * v.powf(gamma)
        }
        VelocityMode::Fixed => fixed,
    };
    out.round().clamp(1.0, 127.0) as u8
}

/// Master volume taper. A smooth curve in dB, `-40 + 66p - 20p²`, through
/// 25 % ≈ -25 dB, 50 % = -12 dB, 80 % = 0 dB (unity, the default) and 100 % =
/// +6 dB. It gets gentler towards the top (about 0.2 dB per MIDI step at the
/// end), so the last part of the travel no longer slams into the limiter.
/// Below 10 % the gain also fades linearly to silence, so there is no jump to
/// "off" at the bottom. Adjacent CC values are never more than ~1.4 dB apart
/// above CC 10.
pub fn volume_db(pos: f32) -> f32 {
    if pos <= 0.0005 {
        return f32::NEG_INFINITY;
    }
    let p = pos.clamp(0.0, 1.0);
    let curve = -40.0 + 66.0 * p - 20.0 * p * p;
    let fade = if p < 0.1 { 20.0 * (p / 0.1).log10() } else { 0.0 };
    curve + fade
}

/// Master volume knob position (0..1) -> linear gain. See `volume_db`.
pub fn volume_gain(pos: f32) -> f32 {
    let db = volume_db(pos);
    if db.is_infinite() { 0.0 } else { 10f32.powf(db / 20.0) }
}

pub struct Params {
    pub volume: AtomicF32,
    pub tone: AtomicF32,
    pub reverb: AtomicF32,
    pub modulation: AtomicF32,
    pub touch_normal: AtomicF32,
    pub touch_light: AtomicF32,
    pub fixed_velocity: AtomicF32, // 1..127
    pub velocity_mode: AtomicU8,
    pub sustain: AtomicF32, // 0..1, last value received (display only; engine gets events)

    // ---- instruments ----
    pub instrument: AtomicU8,
    /// preset index per instrument
    pub preset: [AtomicU8; N_INSTRUMENTS],
    /// the Tone knob per non-piano instrument (index = instrument id; 0 unused):
    /// filter cutoff on the synths, brightness on the electric piano, drive on the organ
    pub synth_cutoff: [AtomicF32; N_INSTRUMENTS],
    /// mod wheel for the non-piano instruments: vibrato (synths), tremolo depth
    /// (electric piano); on the organ, crossing the middle switches the Leslie
    pub vibrato: AtomicF32,
    pub glide: AtomicF32,
    /// Moog-style synth resonance
    pub resonance: AtomicF32,
    /// Polysynth resonance
    pub poly_resonance: AtomicF32,
    /// organ: drawbars (0..8 each, 16' first), percussion switches, Leslie speed
    pub drawbars: [AtomicU8; 9],
    pub perc_on: AtomicBool,
    pub perc_third: AtomicBool,
    pub perc_fast: AtomicBool,
    pub perc_soft: AtomicBool,
    pub leslie_fast: AtomicBool,
    /// Moog-style synth: Voices switch (VoiceMode id; 0 = Poly, the default)
    /// and mono note priority (NotePriority id; 0 = Last)
    pub synth_voices: AtomicU8,
    pub note_priority: AtomicU8,

    // ---- pitch bend ----
    /// the wheel, -1..+1 (0 = centre)
    pub pitch_bend: AtomicF32,
    /// bend range in semitones per instrument, up and down (0..12)
    pub bend_up: [AtomicU8; N_INSTRUMENTS],
    pub bend_down: [AtomicU8; N_INSTRUMENTS],
    /// the piano ignores pitch bend unless this is on
    pub piano_bend: AtomicBool,

    // ---- octave / transpose (applied to incoming MIDI notes) ----
    pub octave: AtomicI32,
    pub transpose: AtomicI32,
    /// MIDI -> UI: inferred octave shift of the hardware keyboard, and whether
    /// any note has been played yet (until then it's the default assumption)
    pub hw_octave: AtomicI32,
    pub hw_seen: AtomicBool,

    // ---- metronome ----
    pub metro_running: AtomicBool,
    /// BPM, 30..240 (rounded to whole BPM by the engine)
    pub tempo: AtomicF32,
    pub beats_per_bar: AtomicU8,
    pub click_on: AtomicBool,
    pub blink_on: AtomicBool,
    pub click_level: AtomicF32,
    /// last tap time (now_us) and the averaged tap interval in µs (0 = none)
    tap_last_us: AtomicU64,
    tap_interval_us: AtomicU64,
    /// Audio -> UI: beat counter, beat-in-bar (0 = downbeat) and when that beat
    /// leaves the speakers (now_us clock, output latency included).
    pub beat_seq: AtomicU32,
    pub beat_in_bar: AtomicU8,
    pub beat_audible_us: AtomicU64,
    /// UI -> audio: estimated output latency in ms, for the beat light.
    pub output_latency_ms: AtomicF32,

    // ---- telemetry (audio/MIDI -> UI) ----
    pub keys_down: [AtomicU64; 2],
    pub keys_sounding: [AtomicU64; 2],
    pub active_voices: AtomicU32,
    pub out_peak: AtomicF32,
    pub limiter_gr_db: AtomicF32,
    pub midi_events: AtomicU32,
    pub last_velocity_in: AtomicU8,
    pub last_velocity_out: AtomicU8,
}

pub const DEFAULT_VOLUME: f32 = 0.8;
pub const DEFAULT_TONE: f32 = 0.5;
pub const DEFAULT_REVERB: f32 = 0.22;
pub const DEFAULT_MOD: f32 = 0.0;
pub const DEFAULT_TOUCH: f32 = 0.5;
pub const DEFAULT_FIXED: f32 = 100.0;

impl Default for Params {
    fn default() -> Self {
        Params {
            volume: AtomicF32::new(DEFAULT_VOLUME),
            tone: AtomicF32::new(DEFAULT_TONE),
            reverb: AtomicF32::new(DEFAULT_REVERB),
            modulation: AtomicF32::new(DEFAULT_MOD),
            touch_normal: AtomicF32::new(DEFAULT_TOUCH),
            touch_light: AtomicF32::new(DEFAULT_TOUCH),
            fixed_velocity: AtomicF32::new(DEFAULT_FIXED),
            velocity_mode: AtomicU8::new(VelocityMode::Light.id()),
            sustain: AtomicF32::new(0.0),
            instrument: AtomicU8::new(0),
            preset: Default::default(),
            synth_cutoff: [
                AtomicF32::new(0.5),
                AtomicF32::new(crate::synth::MONO_PRESETS[0].cutoff),
                AtomicF32::new(crate::strings::STRING_PRESETS[0].cutoff),
                AtomicF32::new(crate::ep::EP_PRESETS[0].tone),
                AtomicF32::new(crate::organ::ORGAN_PRESETS[0].drive),
                AtomicF32::new(crate::poly::POLY_PRESETS[0].cutoff),
            ],
            vibrato: AtomicF32::new(0.0),
            glide: AtomicF32::new(crate::synth::MONO_PRESETS[0].glide),
            resonance: AtomicF32::new(crate::synth::MONO_PRESETS[0].resonance),
            poly_resonance: AtomicF32::new(crate::poly::POLY_PRESETS[0].resonance),
            drawbars: std::array::from_fn(|i| AtomicU8::new(crate::organ::ORGAN_PRESETS[0].drawbars[i])),
            perc_on: AtomicBool::new(crate::organ::ORGAN_PRESETS[0].perc),
            perc_third: AtomicBool::new(crate::organ::ORGAN_PRESETS[0].perc_third),
            perc_fast: AtomicBool::new(crate::organ::ORGAN_PRESETS[0].perc_fast),
            perc_soft: AtomicBool::new(crate::organ::ORGAN_PRESETS[0].perc_soft),
            leslie_fast: AtomicBool::new(false),
            synth_voices: AtomicU8::new(crate::synth::VoiceMode::Poly.id()),
            note_priority: AtomicU8::new(crate::synth::NotePriority::Last.id()),
            pitch_bend: AtomicF32::new(0.0),
            bend_up: std::array::from_fn(|i| AtomicU8::new(InstrumentKind::from_id(i as u8).default_bend())),
            bend_down: std::array::from_fn(|i| AtomicU8::new(InstrumentKind::from_id(i as u8).default_bend())),
            piano_bend: AtomicBool::new(false),
            octave: AtomicI32::new(0),
            transpose: AtomicI32::new(0),
            hw_octave: AtomicI32::new(0),
            hw_seen: AtomicBool::new(false),
            metro_running: AtomicBool::new(false),
            tempo: AtomicF32::new(DEFAULT_TEMPO),
            beats_per_bar: AtomicU8::new(DEFAULT_BEATS_PER_BAR),
            click_on: AtomicBool::new(true),
            blink_on: AtomicBool::new(true),
            click_level: AtomicF32::new(DEFAULT_CLICK_LEVEL),
            tap_last_us: AtomicU64::new(0),
            tap_interval_us: AtomicU64::new(0),
            beat_seq: AtomicU32::new(0),
            beat_in_bar: AtomicU8::new(0),
            beat_audible_us: AtomicU64::new(0),
            output_latency_ms: AtomicF32::new(0.0),
            keys_down: Default::default(),
            keys_sounding: Default::default(),
            active_voices: AtomicU32::new(0),
            out_peak: AtomicF32::new(0.0),
            limiter_gr_db: AtomicF32::new(0.0),
            midi_events: AtomicU32::new(0),
            last_velocity_in: AtomicU8::new(0),
            last_velocity_out: AtomicU8::new(0),
        }
    }
}

impl Params {
    pub fn mode(&self) -> VelocityMode {
        VelocityMode::from_id(self.velocity_mode.load(Ordering::Relaxed))
    }
    pub fn set_mode(&self, m: VelocityMode) {
        self.velocity_mode.store(m.id(), Ordering::Relaxed)
    }
    /// The Touch knob position (0..1) for the current mode.
    pub fn touch_pos(&self) -> f32 {
        match self.mode() {
            VelocityMode::Normal => self.touch_normal.get(),
            VelocityMode::Light => self.touch_light.get(),
            VelocityMode::Fixed => (self.fixed_velocity.get() - 1.0) / 126.0,
        }
    }
    pub fn set_touch_pos(&self, p: f32) {
        let p = p.clamp(0.0, 1.0);
        match self.mode() {
            VelocityMode::Normal => self.touch_normal.set(p),
            VelocityMode::Light => self.touch_light.set(p),
            VelocityMode::Fixed => self.fixed_velocity.set(1.0 + 126.0 * p),
        }
    }
    pub fn shape(&self, vel: u8) -> u8 {
        let m = self.mode();
        let touch = match m {
            VelocityMode::Normal => self.touch_normal.get(),
            _ => self.touch_light.get(),
        };
        shape_velocity(m, touch, self.fixed_velocity.get(), vel)
    }
    pub fn instrument(&self) -> InstrumentKind {
        InstrumentKind::from_id(self.instrument.load(Ordering::Relaxed))
    }
    pub fn set_instrument(&self, i: InstrumentKind) {
        self.instrument.store(i.id(), Ordering::Relaxed)
    }
    pub fn preset_of(&self, i: InstrumentKind) -> usize {
        let n = i.presets().len();
        if n == 0 { 0 } else { (self.preset[i.id() as usize].load(Ordering::Relaxed) as usize).min(n - 1) }
    }
    /// Chooses a preset and loads its knob defaults (cutoff, and for the mono
    /// synth resonance and glide), like recalling a patch on a hardware synth.
    pub fn select_preset(&self, i: InstrumentKind, idx: usize) {
        let n = i.presets().len();
        if n == 0 {
            return;
        }
        let idx = idx % n;
        self.preset[i.id() as usize].store(idx as u8, Ordering::Relaxed);
        match i {
            InstrumentKind::MonoSynth => {
                let p = &crate::synth::MONO_PRESETS[idx];
                self.synth_cutoff[1].set(p.cutoff);
                self.resonance.set(p.resonance);
                self.glide.set(p.glide);
            }
            InstrumentKind::StringMachine => {
                self.synth_cutoff[2].set(crate::strings::STRING_PRESETS[idx].cutoff);
            }
            InstrumentKind::ElectricPiano => {
                self.synth_cutoff[3].set(crate::ep::EP_PRESETS[idx].tone);
            }
            InstrumentKind::Organ => {
                let p = &crate::organ::ORGAN_PRESETS[idx];
                self.synth_cutoff[4].set(p.drive);
                for (d, v) in self.drawbars.iter().zip(p.drawbars) {
                    d.store(v, Ordering::Relaxed);
                }
                self.perc_on.store(p.perc, Ordering::Relaxed);
                self.perc_third.store(p.perc_third, Ordering::Relaxed);
                self.perc_fast.store(p.perc_fast, Ordering::Relaxed);
                self.perc_soft.store(p.perc_soft, Ordering::Relaxed);
            }
            InstrumentKind::Polysynth => {
                let p = &crate::poly::POLY_PRESETS[idx];
                self.synth_cutoff[5].set(p.cutoff);
                self.poly_resonance.set(p.resonance);
            }
            InstrumentKind::GrandPiano => {}
        }
    }
    /// The organ's live settings (drawbars, percussion, drive, Leslie speed).
    pub fn organ_controls(&self) -> crate::organ::OrganControls {
        crate::organ::OrganControls {
            drawbars: std::array::from_fn(|i| self.drawbars[i].load(Ordering::Relaxed).min(8)),
            perc: self.perc_on.load(Ordering::Relaxed),
            perc_third: self.perc_third.load(Ordering::Relaxed),
            perc_fast: self.perc_fast.load(Ordering::Relaxed),
            perc_soft: self.perc_soft.load(Ordering::Relaxed),
            drive: self.synth_cutoff[4].get(),
            leslie_fast: self.leslie_fast.load(Ordering::Relaxed),
            bend: self.bend_semis(InstrumentKind::Organ),
        }
    }
    pub fn toggle_leslie(&self) {
        self.leslie_fast.fetch_xor(true, Ordering::Relaxed);
    }
    pub fn voice_mode(&self) -> crate::synth::VoiceMode {
        crate::synth::VoiceMode::from_id(self.synth_voices.load(Ordering::Relaxed))
    }
    pub fn set_voice_mode(&self, m: crate::synth::VoiceMode) {
        self.synth_voices.store(m.id(), Ordering::Relaxed)
    }
    pub fn note_priority(&self) -> crate::synth::NotePriority {
        crate::synth::NotePriority::from_id(self.note_priority.load(Ordering::Relaxed))
    }
    pub fn set_note_priority(&self, pr: crate::synth::NotePriority) {
        self.note_priority.store(pr.id(), Ordering::Relaxed)
    }
    pub fn next_preset(&self) {
        let i = self.instrument();
        self.select_preset(i, self.preset_of(i) + 1);
    }

    /// Current pitch bend in semitones for `inst` (0 for the piano unless allowed).
    pub fn bend_semis(&self, inst: InstrumentKind) -> f32 {
        if inst == InstrumentKind::GrandPiano && !self.piano_bend.load(Ordering::Relaxed) {
            return 0.0;
        }
        let w = self.pitch_bend.get().clamp(-1.0, 1.0);
        let i = inst.id() as usize;
        if w >= 0.0 {
            w * self.bend_up[i].load(Ordering::Relaxed).min(MAX_BEND_RANGE) as f32
        } else {
            w * self.bend_down[i].load(Ordering::Relaxed).min(MAX_BEND_RANGE) as f32
        }
    }

    /// Semitones added to incoming notes: octave * 12 + transpose.
    pub fn note_shift(&self) -> i32 {
        self.octave.load(Ordering::Relaxed) * 12 + self.transpose.load(Ordering::Relaxed)
    }
    pub fn step_octave(&self, d: i32) {
        let o = (self.octave.load(Ordering::Relaxed) + d).clamp(crate::keyrange::OCTAVE_MIN, crate::keyrange::OCTAVE_MAX);
        self.octave.store(o, Ordering::Relaxed);
    }

    pub fn tempo_bpm(&self) -> f32 {
        self.tempo.get().clamp(TEMPO_MIN, TEMPO_MAX)
    }
    pub fn set_tempo_bpm(&self, bpm: f32) {
        self.tempo.set(bpm.round().clamp(TEMPO_MIN, TEMPO_MAX));
    }
    pub fn toggle_metronome(&self) {
        self.metro_running.fetch_xor(true, Ordering::Relaxed);
    }
    /// Tap tempo. Taps more than 2 s apart start over; the tempo follows the
    /// average of the recent intervals once two taps are in.
    pub fn tap(&self, now_us: u64) {
        let last = self.tap_last_us.load(Ordering::Relaxed);
        let dt = now_us.saturating_sub(last);
        if last != 0 && dt < 200_000 {
            return; // switch bounce or a double press; faster than 300 BPM anyway
        }
        self.tap_last_us.store(now_us, Ordering::Relaxed);
        if last == 0 || dt > 2_000_000 {
            self.tap_interval_us.store(0, Ordering::Relaxed);
            return;
        }
        let prev = self.tap_interval_us.load(Ordering::Relaxed);
        // follow tempo changes quickly but smooth out jitter
        let avg = if prev == 0 || (dt as f64 / prev as f64 - 1.0).abs() > 0.3 { dt } else { (prev * 2 + dt) / 3 };
        self.tap_interval_us.store(avg, Ordering::Relaxed);
        self.set_tempo_bpm(60_000_000.0 / avg as f32);
    }

    /// Get a knob-style target's position (0..1). Tone and the mod wheel follow
    /// the current instrument: on the synths they are filter cutoff and vibrato.
    pub fn get_target(&self, t: Target) -> f32 {
        let inst = self.instrument();
        match t {
            Target::Volume => self.volume.get(),
            Target::Tone if inst.is_synth() => self.synth_cutoff[inst.id() as usize].get(),
            Target::Tone => self.tone.get(),
            Target::Reverb => self.reverb.get(),
            Target::Modulation if inst.is_synth() => self.vibrato.get(),
            Target::Modulation => self.modulation.get(),
            Target::Touch => self.touch_pos(),
            Target::Sustain => self.sustain.get(),
            Target::VelocityMode => self.mode().id() as f32 / 2.0,
            Target::Instrument => inst.id() as f32 / (N_INSTRUMENTS - 1) as f32,
            Target::Preset => {
                let n = inst.presets().len();
                if n > 1 { self.preset_of(inst) as f32 / (n - 1) as f32 } else { 0.0 }
            }
            Target::Glide => self.glide.get(),
            Target::Resonance if inst == InstrumentKind::Polysynth => self.poly_resonance.get(),
            Target::Resonance => self.resonance.get(),
            Target::MetronomeStartStop => if self.metro_running.load(Ordering::Relaxed) { 1.0 } else { 0.0 },
            Target::Tempo => (self.tempo_bpm() - TEMPO_MIN) / (TEMPO_MAX - TEMPO_MIN),
            Target::TapTempo => 0.0,
            Target::OctaveDown | Target::OctaveUp => (self.octave.load(Ordering::Relaxed) + 3) as f32 / 6.0,
            Target::Voices => self.voice_mode().id() as f32,
            Target::Leslie => if self.leslie_fast.load(Ordering::Relaxed) { 1.0 } else { 0.0 },
        }
    }
    pub fn set_target(&self, t: Target, v: f32) {
        let v = v.clamp(0.0, 1.0);
        let inst = self.instrument();
        match t {
            Target::Volume => self.volume.set(v),
            Target::Tone if inst.is_synth() => self.synth_cutoff[inst.id() as usize].set(v),
            Target::Tone => self.tone.set(v),
            Target::Reverb => self.reverb.set(v),
            Target::Modulation if inst == InstrumentKind::Organ => {
                // the wheel switches the Leslie when it crosses the middle (so a
                // mapped button can still toggle it while the wheel rests)
                let old = self.vibrato.get();
                self.vibrato.set(v);
                if (old >= 0.5) != (v >= 0.5) {
                    self.leslie_fast.store(v >= 0.5, Ordering::Relaxed);
                }
            }
            Target::Modulation if inst.is_synth() => self.vibrato.set(v),
            Target::Modulation => self.modulation.set(v),
            Target::Touch => self.set_touch_pos(v),
            Target::Sustain => self.sustain.set(v),
            Target::VelocityMode => self.set_mode(VelocityMode::from_id((v * 2.0).round() as u8)),
            Target::Instrument => self.set_instrument(InstrumentKind::from_id((v * (N_INSTRUMENTS - 1) as f32).round() as u8)),
            Target::Preset => {
                let n = inst.presets().len();
                if n > 0 {
                    let idx = (v * (n - 1) as f32).round() as usize;
                    if idx != self.preset_of(inst) {
                        self.select_preset(inst, idx);
                    }
                }
            }
            Target::Glide => self.glide.set(v),
            Target::Resonance if inst == InstrumentKind::Polysynth => self.poly_resonance.set(v),
            Target::Resonance => self.resonance.set(v),
            Target::MetronomeStartStop => self.metro_running.store(v >= 0.5, Ordering::Relaxed),
            Target::Tempo => self.set_tempo_bpm(TEMPO_MIN + v * (TEMPO_MAX - TEMPO_MIN)),
            Target::TapTempo => {}
            Target::OctaveDown | Target::OctaveUp => self.octave.store((v * 6.0).round() as i32 - 3, Ordering::Relaxed),
            Target::Voices => self.set_voice_mode(crate::synth::VoiceMode::from_id((v >= 0.5) as u8)),
            Target::Leslie => self.leslie_fast.store(v >= 0.5, Ordering::Relaxed),
        }
    }
    pub fn key_bit(arr: &[AtomicU64; 2], key: u8, on: bool) {
        let (w, b) = ((key / 64) as usize, key % 64);
        if on {
            arr[w].fetch_or(1 << b, Ordering::Relaxed);
        } else {
            arr[w].fetch_and(!(1 << b), Ordering::Relaxed);
        }
    }
    pub fn key_is(arr: &[AtomicU64; 2], key: u8) -> bool {
        let (w, b) = ((key / 64) as usize, key % 64);
        arr[w].load(Ordering::Relaxed) & (1 << b) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_mode_is_loud_for_soft_playing() {
        // soft playing (vel 30) in Light mode lands in the medium-loud range
        let v = shape_velocity(VelocityMode::Light, 0.5, 100.0, 30);
        assert!(v >= 85, "{v}");
        // still dynamic: hard playing is louder than soft
        assert!(shape_velocity(VelocityMode::Light, 0.5, 100.0, 127) > v);
        assert_eq!(shape_velocity(VelocityMode::Light, 0.5, 100.0, 127), 127);
        // monotonic
        let mut last = 0;
        for vel in 1..=127 {
            let o = shape_velocity(VelocityMode::Light, 0.5, 100.0, vel);
            assert!(o >= last);
            last = o;
        }
    }

    #[test]
    fn normal_mode_centre_is_linear() {
        for vel in [1u8, 20, 64, 100, 127] {
            assert_eq!(shape_velocity(VelocityMode::Normal, 0.5, 100.0, vel), vel);
        }
        // lighter touch raises soft notes
        assert!(shape_velocity(VelocityMode::Normal, 1.0, 100.0, 40) > 40);
        assert!(shape_velocity(VelocityMode::Normal, 0.0, 100.0, 40) < 40);
    }

    #[test]
    fn fixed_mode() {
        for vel in [1u8, 64, 127] {
            assert_eq!(shape_velocity(VelocityMode::Fixed, 0.5, 100.0, vel), 100);
        }
    }

    #[test]
    fn volume_taper() {
        // unity at the default position (saved settings keep their loudness)
        assert!((volume_gain(0.8) - 1.0).abs() < 1e-4);
        assert!((volume_db(1.0) - 6.0).abs() < 1e-3, "max +6 dB");
        assert!((volume_db(0.5) + 12.0).abs() < 0.05);
        assert!((volume_db(0.25) + 24.75).abs() < 0.05);
        assert_eq!(volume_gain(0.0), 0.0);
        // no lurch at the bottom: the first step is already very quiet
        assert!(volume_db(1.0 / 127.0) < -55.0);
    }

    #[test]
    fn volume_is_monotonic_with_small_cc_steps() {
        let mut last = -1.0f32;
        for cc in 0..=127u32 {
            let g = volume_gain(cc as f32 / 127.0);
            assert!(g > last || (cc == 0 && g == 0.0), "not monotonic at CC {cc}");
            last = g;
            if cc > 10 {
                let step = volume_db(cc as f32 / 127.0) - volume_db((cc - 1) as f32 / 127.0);
                assert!(step > 0.0 && step <= 1.5, "CC {} -> {cc}: {step:.2} dB", cc - 1);
            }
        }
    }

    #[test]
    fn tone_and_wheel_follow_the_instrument() {
        let p = Params::default();
        p.set_target(Target::Tone, 0.3);
        p.set_instrument(InstrumentKind::MonoSynth);
        p.set_target(Target::Tone, 0.9);
        p.set_target(Target::Modulation, 0.7);
        assert!((p.tone.get() - 0.3).abs() < 1e-6, "piano tone untouched");
        assert!((p.synth_cutoff[1].get() - 0.9).abs() < 1e-6);
        assert!((p.vibrato.get() - 0.7).abs() < 1e-6);
        assert_eq!(p.modulation.get(), DEFAULT_MOD, "piano shimmer untouched");
        p.set_instrument(InstrumentKind::GrandPiano);
        assert!((p.get_target(Target::Tone) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn presets_load_knob_defaults() {
        let p = Params::default();
        p.select_preset(InstrumentKind::MonoSynth, 1);
        assert_eq!(p.preset_of(InstrumentKind::MonoSynth), 1);
        assert_eq!(p.glide.get(), crate::synth::MONO_PRESETS[1].glide);
        p.set_instrument(InstrumentKind::MonoSynth);
        p.next_preset();
        p.next_preset();
        assert_eq!(p.preset_of(InstrumentKind::MonoSynth), 0, "wraps around");
    }

    #[test]
    fn pitch_bend_14bit() {
        // centre 8192 = 0x2000: LSB 0, MSB 64 -> exactly no bend
        assert_eq!(bend_from_14bit(0, 64), 0.0);
        assert_eq!(bend_from_14bit(0, 0), -1.0);
        assert_eq!(bend_from_14bit(127, 127), 1.0);
        assert!(bend_from_14bit(1, 64) > 0.0 && bend_from_14bit(127, 63) < 0.0);
        let p = Params::default();
        let mono = InstrumentKind::MonoSynth;
        // default ±2
        p.pitch_bend.set(bend_from_14bit(127, 127));
        assert_eq!(p.bend_semis(mono), 2.0);
        p.pitch_bend.set(bend_from_14bit(0, 0));
        assert_eq!(p.bend_semis(mono), -2.0);
        p.pitch_bend.set(bend_from_14bit(0, 64));
        assert_eq!(p.bend_semis(mono), 0.0);
        // asymmetric: up 2, down 12
        p.bend_down[mono.id() as usize].store(12, Ordering::Relaxed);
        p.pitch_bend.set(bend_from_14bit(0, 0));
        assert_eq!(p.bend_semis(mono), -12.0);
        p.pitch_bend.set(bend_from_14bit(127, 127));
        assert_eq!(p.bend_semis(mono), 2.0);
        // the piano ignores bend unless allowed
        assert_eq!(p.bend_semis(InstrumentKind::GrandPiano), 0.0);
        p.piano_bend.store(true, Ordering::Relaxed);
        assert_eq!(p.bend_semis(InstrumentKind::GrandPiano), 2.0);
    }

    #[test]
    fn tap_tempo() {
        let p = Params::default();
        let t0 = 10_000_000u64;
        for i in 0..4 {
            p.tap(t0 + i * 500_000); // 120 BPM
        }
        assert_eq!(p.tempo_bpm(), 120.0);
        // a long pause starts over; the first new tap changes nothing
        p.tap(t0 + 10_000_000);
        assert_eq!(p.tempo_bpm(), 120.0);
        p.tap(t0 + 10_750_000); // 80 BPM
        assert_eq!(p.tempo_bpm(), 80.0);
        // tempo is clamped
        p.tap(t0 + 12_700_000);
        assert!(p.tempo_bpm() >= TEMPO_MIN);
    }

    #[test]
    fn touch_knob_follows_mode() {
        let p = Params::default();
        p.set_mode(VelocityMode::Fixed);
        p.set_touch_pos(1.0);
        assert_eq!(p.shape(10), 127);
        p.set_mode(VelocityMode::Light);
        assert!((p.touch_pos() - 0.5).abs() < 1e-6);
    }
}
