//! Real-time synth building blocks and the Moog-style synth (poly or mono).
//!
//! Everything here runs on the audio thread: no allocation, no locks.
//!
//! - Oscillators are band-limited with PolyBLEP (saw, square) and PolyBLAMP
//!   (triangle), so high notes don't alias.
//! - The filter is a 4-pole (24 dB/oct) resonant ladder in zero-delay-feedback
//!   (topology-preserving transform) form, with soft saturation (tanh) at its
//!   input. It stays stable up to and past self-oscillation.
//! - Envelopes are analogue-style ADSRs: an RC-like attack, exponential decay
//!   and release, and retriggering from the current level (no clicks).
//! - Denormals: filter states and envelopes are flushed to zero when they
//!   become tiny, and idle voices are skipped.

use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

// ------------------------------------------------------------ building blocks

/// PolyBLEP residual for a discontinuity at phase 0 (t in 0..1, dt = f/sr).
#[inline(always)]
pub fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// PolyBLAMP residual (integrated PolyBLEP) for a slope change at phase 0.
#[inline(always)]
pub fn poly_blamp(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt - 1.0;
        -x * x * x / 3.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt + 1.0;
        x * x * x / 3.0
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    Saw,
    Square,
    Triangle,
}

/// One band-limited sample of `wave` at phase `t` (0..1) with increment `dt`.
#[inline(always)]
pub fn osc_sample(wave: Wave, t: f32, dt: f32) -> f32 {
    match wave {
        Wave::Saw => (2.0 * t - 1.0) - poly_blep(t, dt),
        Wave::Square => {
            let naive = if t < 0.5 { 1.0 } else { -1.0 };
            let t2 = if t < 0.5 { t + 0.5 } else { t - 0.5 };
            naive + poly_blep(t, dt) - poly_blep(t2, dt)
        }
        Wave::Triangle => {
            // trough -1 at t = 0, peak +1 at t = 0.5; slope ±4 per cycle, so the
            // slope changes by +8 at t = 0 and -8 at t = 0.5 (8·dt per sample)
            let naive = if t < 0.5 { 4.0 * t - 1.0 } else { 3.0 - 4.0 * t };
            let th = if t < 0.5 { t + 0.5 } else { t - 0.5 };
            naive + 8.0 * dt * (poly_blamp(t, dt) - poly_blamp(th, dt))
        }
    }
}

/// Fast tanh (Padé-style), exact enough for soft saturation; |out| <= 1.
#[inline(always)]
pub fn tanh_fast(x: f32) -> f32 {
    if x > 3.0 {
        1.0
    } else if x < -3.0 {
        -1.0
    } else {
        let x2 = x * x;
        x * (27.0 + x2) / (27.0 + 9.0 * x2)
    }
}

#[inline(always)]
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-15 { 0.0 } else { x }
}

/// 4-pole resonant ladder low-pass, zero-delay-feedback (TPT) form with a
/// tanh-saturated input stage. `k` is the feedback (0 = none, 4 = self-oscillation).
#[derive(Clone, Copy, Default)]
pub struct Ladder {
    s: [f32; 4],
}

impl Ladder {
    pub fn reset(&mut self) {
        self.s = [0.0; 4];
    }
    /// `g` = tan(pi * fc / sr), precomputed by the caller.
    #[inline(always)]
    pub fn tick(&mut self, x: f32, g: f32, k: f32) -> f32 {
        let big_g = g / (1.0 + g);
        let beta = 1.0 / (1.0 + g);
        let g2 = big_g * big_g;
        let g3 = g2 * big_g;
        let g4 = g3 * big_g;
        let s = &mut self.s;
        let sum = g3 * beta * s[0] + g2 * beta * s[1] + big_g * beta * s[2] + beta * s[3];
        // partial bass-loss compensation, as many ladder designs do
        let xin = x * (1.0 + 0.45 * k);
        let u = tanh_fast((xin - k * sum) / (1.0 + k * g4));
        let mut y = u;
        for st in s.iter_mut() {
            let v = (y - *st) * big_g;
            let out = v + *st;
            *st = out + v;
            y = out;
        }
        y
    }
    pub fn flush_denormals(&mut self) {
        for s in self.s.iter_mut() {
            *s = flush(*s);
        }
    }
    pub fn state(&self) -> [f32; 4] {
        self.s
    }
}

/// 2-pole state-variable filter (TPT), low-pass and high-pass outputs.
#[derive(Clone, Copy, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    /// Returns (low, high). `g` = tan(pi*fc/sr), `r2` = 1/Q.
    #[inline(always)]
    pub fn tick(&mut self, x: f32, g: f32, r2: f32) -> (f32, f32) {
        let h = 1.0 / (1.0 + r2 * g + g * g);
        let hp = (x - (r2 + g) * self.ic1 - self.ic2) * h;
        let bp = g * hp + self.ic1;
        self.ic1 = g * hp + bp;
        let lp = g * bp + self.ic2;
        self.ic2 = g * bp + lp;
        (lp, hp)
    }
    pub fn flush_denormals(&mut self) {
        self.ic1 = flush(self.ic1);
        self.ic2 = flush(self.ic2);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdsrTimes {
    /// seconds
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
}

/// Analogue-style ADSR. Attack charges towards 1.25 and stops at 1.0 (RC curve
/// like a capacitor); decay and release are exponential. Retriggers from the
/// current level. Times are "time to (almost) get there".
#[derive(Clone, Copy)]
pub struct Adsr {
    pub level: f32,
    pub stage: Stage,
    att: f32,
    dec: f32,
    rel: f32,
    sus: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Adsr { level: 0.0, stage: Stage::Idle, att: 1.0, dec: 1.0, rel: 1.0, sus: 1.0 }
    }
}

impl Adsr {
    pub fn set(&mut self, t: &AdsrTimes, sr: f32) {
        // attack: RC towards 1.25 reaching 1.0 after `attack` s: 1.25(1-e^-x)=1 -> x = ln 5
        self.att = 1.0 - (-(5f32.ln()) / (t.attack.max(0.0015) * sr)).exp();
        // decay / release: reach -60 dB of the distance after the given time
        self.dec = 1.0 - (-6.9 / (t.decay.max(0.005) * sr)).exp();
        self.rel = 1.0 - (-6.9 / (t.release.max(0.006) * sr)).exp();
        self.sus = t.sustain.clamp(0.0, 1.0);
    }
    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }
    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }
    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }
    #[inline(always)]
    pub fn tick(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += (1.25 - self.level) * self.att;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level += (self.sus - self.level) * self.dec;
            }
            Stage::Release => {
                self.level -= self.level * self.rel;
                if self.level < 1e-4 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

/// Small xorshift for noise / random detune.
#[derive(Clone, Copy)]
pub struct Rng(pub u32);
impl Rng {
    #[inline(always)]
    pub fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
    }
}

/// Knob position (0..1) -> filter cutoff in Hz: 40 Hz .. 20.5 kHz, exponential.
pub fn cutoff_hz(pos: f32) -> f32 {
    40.0 * 2f32.powf(pos.clamp(0.0, 1.0) * 9.0)
}

/// Knob position (0..1) -> ladder feedback k. 1.0 = 4.0 (self-oscillation edge).
pub fn resonance_k(pos: f32) -> f32 {
    4.0 * pos.clamp(0.0, 1.0)
}

/// Knob position (0..1) -> glide time in seconds (0 .. 1.2 s, quadratic taper).
pub fn glide_secs(pos: f32) -> f32 {
    let p = pos.clamp(0.0, 1.0);
    1.2 * p * p
}

/// Mod wheel (0..1) -> vibrato depth in semitones (0 .. ±0.6).
pub fn vibrato_semis(wheel: f32) -> f32 {
    let w = wheel.clamp(0.0, 1.0);
    0.6 * w * w.sqrt()
}

pub const VIBRATO_HZ: f32 = 5.6;

// ------------------------------------------------------------ mono synth

/// How many notes the synth plays at once. Chosen on the main screen,
/// independent of the preset (every preset can play either way).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceMode {
    /// Up to POLY_VOICES voices: chords work. Glide is off. The default.
    #[default]
    Poly,
    /// One voice, legato (no retrigger while a key is held), with glide. Which
    /// held key sounds is set by the note priority.
    Mono,
}

impl VoiceMode {
    pub const ALL: [VoiceMode; 2] = [VoiceMode::Poly, VoiceMode::Mono];
    pub fn id(self) -> u8 {
        match self {
            VoiceMode::Poly => 0,
            VoiceMode::Mono => 1,
        }
    }
    pub fn from_id(id: u8) -> VoiceMode {
        if id == 1 { VoiceMode::Mono } else { VoiceMode::Poly }
    }
    pub fn toggled(self) -> VoiceMode {
        match self {
            VoiceMode::Poly => VoiceMode::Mono,
            VoiceMode::Mono => VoiceMode::Poly,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            VoiceMode::Poly => "Poly",
            VoiceMode::Mono => "Mono",
        }
    }
}

/// Which held key the mono voice plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotePriority {
    /// The most recently pressed key (releasing it falls back to the previous one).
    #[default]
    Last,
    /// The highest held key (good for leads over a held bass note).
    High,
    /// The lowest held key (the classic Minimoog-style bass behaviour).
    Low,
}

impl NotePriority {
    pub const ALL: [NotePriority; 3] = [NotePriority::Last, NotePriority::High, NotePriority::Low];
    pub fn id(self) -> u8 {
        match self {
            NotePriority::Last => 0,
            NotePriority::High => 1,
            NotePriority::Low => 2,
        }
    }
    pub fn from_id(id: u8) -> NotePriority {
        match id {
            1 => NotePriority::High,
            2 => NotePriority::Low,
            _ => NotePriority::Last,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            NotePriority::Last => "Last",
            NotePriority::High => "High",
            NotePriority::Low => "Low",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OscSpec {
    pub wave: Wave,
    /// semitones relative to the played key (e.g. -12 = one octave down)
    pub semis: f32,
    /// fine detune in cents
    pub cents: f32,
    pub level: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct MonoPreset {
    pub name: &'static str,
    pub osc: [OscSpec; 3],
    pub noise: f32,
    /// filter envelope amount in octaves
    pub env_amount: f32,
    /// filter keyboard tracking (1 = cutoff follows the key exactly)
    pub key_track: f32,
    /// level into the ladder (higher = more saturation)
    pub drive: f32,
    pub filter_env: AdsrTimes,
    pub amp_env: AdsrTimes,
    /// knob defaults loaded when the preset is chosen
    pub cutoff: f32,
    pub resonance: f32,
    pub glide: f32,
    /// output gain in Mono voice mode (one note)
    pub gain: f32,
    /// per-voice output gain in Poly voice mode. Fixed (it never depends on how
    /// many notes are held), so adding a note never makes the others duck;
    /// chosen so a triad sits at about the same level as one mono note.
    pub poly_gain: f32,
}

pub const MONO_PRESETS: [MonoPreset; 3] = [
    MonoPreset {
        name: "Bass",
        osc: [
            OscSpec { wave: Wave::Saw, semis: 0.0, cents: 0.0, level: 1.0 },
            OscSpec { wave: Wave::Saw, semis: 0.0, cents: 7.0, level: 0.8 },
            OscSpec { wave: Wave::Square, semis: -12.0, cents: -4.0, level: 0.6 },
        ],
        noise: 0.0,
        env_amount: 3.2,
        key_track: 0.75,
        drive: 0.75,
        filter_env: AdsrTimes { attack: 0.001, decay: 0.24, sustain: 0.18, release: 0.12 },
        amp_env: AdsrTimes { attack: 0.002, decay: 0.7, sustain: 0.8, release: 0.07 },
        cutoff: 0.36,
        resonance: 0.28,
        glide: 0.0,
        gain: 0.95,
        poly_gain: 0.4,
    },
    MonoPreset {
        name: "Lead",
        osc: [
            OscSpec { wave: Wave::Saw, semis: 0.0, cents: 0.0, level: 1.0 },
            OscSpec { wave: Wave::Square, semis: 0.0, cents: 6.0, level: 0.65 },
            OscSpec { wave: Wave::Saw, semis: 12.0, cents: -5.0, level: 0.4 },
        ],
        noise: 0.0,
        env_amount: 2.0,
        key_track: 0.8,
        drive: 0.7,
        filter_env: AdsrTimes { attack: 0.008, decay: 0.7, sustain: 0.45, release: 0.3 },
        amp_env: AdsrTimes { attack: 0.004, decay: 1.0, sustain: 1.0, release: 0.18 },
        cutoff: 0.5,
        resonance: 0.38,
        glide: 0.35,
        gain: 0.8,
        poly_gain: 0.34,
    },
    MonoPreset {
        name: "Poly",
        osc: [
            OscSpec { wave: Wave::Saw, semis: 0.0, cents: -6.0, level: 1.0 },
            OscSpec { wave: Wave::Saw, semis: 0.0, cents: 6.0, level: 1.0 },
            OscSpec { wave: Wave::Triangle, semis: -12.0, cents: 0.0, level: 0.5 },
        ],
        noise: 0.0,
        env_amount: 1.8,
        key_track: 0.75,
        drive: 0.55,
        filter_env: AdsrTimes { attack: 0.03, decay: 0.9, sustain: 0.4, release: 0.4 },
        amp_env: AdsrTimes { attack: 0.012, decay: 1.2, sustain: 0.85, release: 0.45 },
        cutoff: 0.48,
        resonance: 0.15,
        glide: 0.0,
        gain: 1.2,
        poly_gain: 0.5,
    },
];

pub const POLY_VOICES: usize = 8;

#[derive(Clone, Copy)]
struct SynthVoice {
    active: bool,
    key: u8,
    /// pitch = target + bend; `bend` decays to zero for glide (exponential,
    /// in semitones, like an RC portamento)
    target: f32,
    bend: f32,
    vel: f32,
    /// small per-note tuning offset (frequency ratio), like an analogue
    /// voice's drift: keeps octave-related notes' coinciding harmonics from
    /// locking at a fixed phase, where they could partly cancel for as long
    /// as the chord is held (they beat slowly instead)
    drift: f32,
    phase: [f32; 3],
    filter: Ladder,
    amp: Adsr,
    fenv: Adsr,
    /// key released while the sustain pedal was down
    sustained: bool,
    age: u64,
}

impl SynthVoice {
    const EMPTY: SynthVoice = SynthVoice {
        active: false,
        key: 0,
        target: 60.0,
        bend: 0.0,
        vel: 0.0,
        drift: 1.0,
        phase: [0.0; 3],
        filter: Ladder { s: [0.0; 4] },
        amp: Adsr { level: 0.0, stage: Stage::Idle, att: 1.0, dec: 1.0, rel: 1.0, sus: 1.0 },
        fenv: Adsr { level: 0.0, stage: Stage::Idle, att: 1.0, dec: 1.0, rel: 1.0, sus: 1.0 },
        sustained: false,
        age: 0,
    };
}

/// Per-block control values for the mono synth.
#[derive(Clone, Copy, Debug)]
pub struct MonoControls {
    /// cutoff knob 0..1
    pub cutoff: f32,
    /// resonance knob 0..1
    pub resonance: f32,
    /// glide knob 0..1
    pub glide: f32,
    /// mod wheel 0..1 (vibrato)
    pub vibrato: f32,
    /// pitch bend target in semitones (smoothed per sample here)
    pub bend: f32,
}

/// The Moog-style synth: poly (chords) or mono (one voice, legato, glide).
pub struct MonoSynth {
    sr: f32,
    preset: usize,
    mode: VoiceMode,
    priority: NotePriority,
    /// a panic shortened the envelopes; restore them once everything is silent
    panicked: bool,
    voices: [SynthVoice; POLY_VOICES],
    /// held keys in mono mode, oldest first
    stack: [u8; 16],
    stack_len: usize,
    pedal: bool,
    clock: u64,
    vib_phase: f32,
    rng: Rng,
    /// per-oscillator frequency ratios for the preset
    ratio: [f32; 3],
    /// smoothed controls
    cut_s: f32,
    res_s: f32,
    /// pitch bend in semitones, smoothed per sample
    bend_s: f32,
    /// last rendered pitch of the mono voice (with glide and bend; tests/UI)
    last_pitch: f32,
}

/// Maximum analogue-style tuning drift per note, in cents (±).
pub const DRIFT_CENTS: f32 = 1.5;

fn drift_ratio(r: &mut Rng) -> f32 {
    2f32.powf(r.next_f32() * DRIFT_CENTS / 1200.0)
}

impl MonoSynth {
    pub fn new(sr: f32) -> Self {
        let mut s = MonoSynth {
            sr,
            preset: 0,
            mode: VoiceMode::Poly,
            priority: NotePriority::Last,
            panicked: false,
            voices: [SynthVoice::EMPTY; POLY_VOICES],
            stack: [0; 16],
            stack_len: 0,
            pedal: false,
            clock: 0,
            vib_phase: 0.0,
            rng: Rng(0x1234_5678),
            ratio: [1.0; 3],
            cut_s: -1.0,
            res_s: 0.0,
            bend_s: 0.0,
            last_pitch: 0.0,
        };
        s.set_preset(0);
        s
    }

    pub fn preset(&self) -> usize {
        self.preset
    }

    fn p(&self) -> &'static MonoPreset {
        &MONO_PRESETS[self.preset]
    }

    pub fn voice_mode(&self) -> VoiceMode {
        self.mode
    }

    /// Switches between Poly and Mono. Sounding notes are released first (they
    /// ring out), so no voice is left stuck when the other mode takes over.
    pub fn set_voice_mode(&mut self, m: VoiceMode) {
        if m != self.mode {
            self.release_all();
            self.mode = m;
        }
    }

    pub fn priority(&self) -> NotePriority {
        self.priority
    }

    /// Note priority for Mono mode. Takes effect at once if keys are held.
    pub fn set_priority(&mut self, pr: NotePriority) {
        if pr == self.priority {
            return;
        }
        self.priority = pr;
        if self.mode == VoiceMode::Mono && self.stack_len > 0 {
            let k = self.pick();
            let v = &mut self.voices[0];
            if v.active && v.key != k {
                let cur = v.target + v.bend;
                v.key = k;
                v.target = k as f32;
                v.bend = cur - k as f32;
            }
        }
    }

    /// The held key that should sound in mono mode (stack must not be empty).
    fn pick(&self) -> u8 {
        let held = &self.stack[..self.stack_len];
        match self.priority {
            NotePriority::Last => held[held.len() - 1],
            NotePriority::High => *held.iter().max().unwrap(),
            NotePriority::Low => *held.iter().min().unwrap(),
        }
    }

    /// Switches preset. Sounding notes are released first.
    pub fn set_preset(&mut self, idx: usize) {
        let idx = idx.min(MONO_PRESETS.len() - 1);
        if idx != self.preset {
            self.release_all();
        }
        self.preset = idx;
        let p = &MONO_PRESETS[idx];
        for i in 0..3 {
            self.ratio[i] = 2f32.powf((p.osc[i].semis + p.osc[i].cents / 100.0) / 12.0);
        }
        let sr = self.sr;
        for v in self.voices.iter_mut() {
            v.amp.set(&p.amp_env, sr);
            v.fenv.set(&p.filter_env, sr);
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Keys currently sounding (for the on-screen keyboard).
    pub fn sounding(&self, out: &mut [u64; 2]) {
        for v in self.voices.iter().filter(|v| v.active && v.amp.stage != Stage::Release && v.amp.stage != Stage::Idle) {
            out[(v.key / 64) as usize] |= 1 << (v.key % 64);
        }
    }

    fn stack_remove(&mut self, key: u8) {
        if let Some(i) = self.stack[..self.stack_len].iter().position(|k| *k == key) {
            self.stack.copy_within(i + 1..self.stack_len, i);
            self.stack_len -= 1;
        }
    }

    fn stack_push(&mut self, key: u8) {
        self.stack_remove(key);
        if self.stack_len == self.stack.len() {
            self.stack.copy_within(1.., 0);
            self.stack_len -= 1;
        }
        self.stack[self.stack_len] = key;
        self.stack_len += 1;
    }

    /// `vel` is the shaped velocity, 1..127.
    pub fn note_on(&mut self, key: u8, vel: u8, glide_pos: f32) {
        let vn = vel as f32 / 127.0;
        self.clock += 1;
        match self.mode {
            VoiceMode::Mono => {
                let legato = self.stack_len > 0;
                self.stack_push(key);
                let key = self.pick();
                let v = &mut self.voices[0];
                if legato && v.active && v.key == key {
                    // e.g. High priority and a lower key: the sounding note carries on
                    return;
                }
                let sounding = v.active && v.amp.level > 1e-3;
                let cur = v.target + v.bend;
                v.key = key;
                v.target = key as f32;
                v.bend = if glide_pos <= 0.0005 || !sounding { 0.0 } else { cur - key as f32 };
                v.sustained = false;
                v.age = self.clock;
                if !legato {
                    // a new phrase: retrigger both envelopes from their current level
                    v.vel = vn;
                    v.drift = drift_ratio(&mut self.rng);
                    v.amp.gate_on();
                    v.fenv.gate_on();
                    if !v.active {
                        v.filter.reset();
                        let r = &mut self.rng;
                        v.phase = [0.0, (r.next_f32() * 0.5 + 0.5), (r.next_f32() * 0.5 + 0.5)];
                    }
                }
                v.active = true;
            }
            VoiceMode::Poly => {
                // same key again: reuse its voice
                let slot = self
                    .voices
                    .iter()
                    .position(|v| v.active && v.key == key)
                    .or_else(|| self.voices.iter().position(|v| !v.active))
                    .unwrap_or_else(|| {
                        // steal: a released voice with the lowest level, else the oldest
                        let released = self
                            .voices
                            .iter()
                            .enumerate()
                            .filter(|(_, v)| v.amp.stage == Stage::Release)
                            .min_by(|a, b| a.1.amp.level.total_cmp(&b.1.amp.level))
                            .map(|(i, _)| i);
                        released.unwrap_or_else(|| self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap())
                    });
                let v = &mut self.voices[slot];
                if !v.active {
                    v.filter.reset();
                    let r = &mut self.rng;
                    v.phase = [r.next_f32() * 0.5 + 0.5, r.next_f32() * 0.5 + 0.5, r.next_f32() * 0.5 + 0.5];
                }
                v.active = true;
                v.key = key;
                v.target = key as f32;
                v.bend = 0.0;
                v.vel = vn;
                v.drift = drift_ratio(&mut self.rng);
                v.sustained = false;
                v.age = self.clock;
                v.amp.gate_on();
                v.fenv.gate_on();
            }
        }
    }

    pub fn note_off(&mut self, key: u8) {
        match self.mode {
            VoiceMode::Mono => {
                if !self.stack[..self.stack_len].contains(&key) {
                    return;
                }
                self.stack_remove(key);
                if self.stack_len > 0 {
                    // fall back to the held key the priority picks (legato, glides if glide is on)
                    let k = self.pick();
                    let v = &mut self.voices[0];
                    if v.key == k {
                        return; // released a key that wasn't sounding
                    }
                    let cur = v.target + v.bend;
                    v.key = k;
                    v.target = k as f32;
                    v.bend = cur - k as f32;
                    return;
                }
                let v = &mut self.voices[0];
                if self.pedal {
                    v.sustained = true;
                } else {
                    v.amp.gate_off();
                    v.fenv.gate_off();
                }
            }
            VoiceMode::Poly => {
                let pedal = self.pedal;
                for v in self.voices.iter_mut().filter(|v| v.active && v.key == key && v.amp.stage != Stage::Release) {
                    if pedal {
                        v.sustained = true;
                    } else {
                        v.amp.gate_off();
                        v.fenv.gate_off();
                    }
                }
            }
        }
    }

    /// Glide jumps immediately when the glide knob is at zero.
    pub fn sustain(&mut self, down: bool) {
        self.pedal = down;
        if !down {
            let held = self.stack_len > 0;
            let mono = self.mode == VoiceMode::Mono;
            for v in self.voices.iter_mut().filter(|v| v.sustained) {
                v.sustained = false;
                if !(mono && held) {
                    v.amp.gate_off();
                    v.fenv.gate_off();
                }
            }
        }
    }

    pub fn release_all(&mut self) {
        self.stack_len = 0;
        for v in self.voices.iter_mut() {
            v.sustained = false;
            v.amp.gate_off();
            v.fenv.gate_off();
        }
    }

    /// Fast fade of everything (panic): a 10 ms release.
    pub fn panic(&mut self) {
        self.stack_len = 0;
        self.panicked = true;
        let t = AdsrTimes { attack: 0.01, decay: 0.01, sustain: 0.0, release: 0.01 };
        for v in self.voices.iter_mut() {
            v.sustained = false;
            v.amp.set(&t, self.sr);
            v.amp.gate_off();
            v.fenv.gate_off();
        }
    }

    /// Current pitch of the mono voice (tests / UI).
    pub fn mono_pitch(&self) -> Option<f32> {
        let v = &self.voices[0];
        if v.active { Some(v.target + v.bend) } else { None }
    }

    /// Pitch the mono voice last played at, including glide and pitch bend.
    pub fn sounding_pitch(&self) -> f32 {
        self.last_pitch
    }

    pub fn mono_amp_stage(&self) -> Stage {
        self.voices[0].amp.stage
    }

    /// Adds `n` stereo frames into `out` (interleaved).
    pub fn render(&mut self, out: &mut [f32], n: usize, c: MonoControls) {
        // pitch bend: one-pole smoothing (~3 ms) per sample, so the wheel's
        // 14-bit steps and the message rate never cause zipper noise
        let bend_a = 1.0 - (-1.0 / (0.003 * self.sr)).exp();
        if !self.voices.iter().any(|v| v.active) {
            if self.panicked {
                // the 10 ms panic fade is done: restore the preset's envelopes
                self.panicked = false;
                let p = self.p();
                let sr = self.sr;
                for v in self.voices.iter_mut() {
                    v.amp.set(&p.amp_env, sr);
                    v.fenv.set(&p.filter_env, sr);
                }
            }
            // keep the vibrato LFO running so it doesn't restart in sync with notes
            self.vib_phase = (self.vib_phase + VIBRATO_HZ * n as f32 / self.sr).fract();
            self.bend_s = c.bend;
            return;
        }
        let bend0 = self.bend_s;
        let mut bend_end = bend0;
        let p = *self.p();
        let sr = self.sr;
        let inv_sr = 1.0 / sr;
        // controls, smoothed per block (~5 ms)
        if self.cut_s < 0.0 {
            self.cut_s = c.cutoff;
            self.res_s = c.resonance;
        }
        let a = 1.0 - (-(n as f32) / (0.005 * sr)).exp();
        let cut0 = self.cut_s;
        let res0 = self.res_s;
        self.cut_s += (c.cutoff - self.cut_s) * a;
        self.res_s += (c.resonance - self.res_s) * a;
        // cutoff knob in octaves above 40 Hz (interpolated across the block)
        let oct0 = cut0 * 9.0;
        let oct1 = self.cut_s * 9.0;
        let k0 = resonance_k(res0);
        let k1 = resonance_k(self.res_s);
        let glide_t = glide_secs(c.glide);
        // bend multiplier per sample: ~98 % of the way after the glide time
        let mono = self.mode == VoiceMode::Mono;
        let glide_keep = if mono && glide_t > 0.0 { (-1.0 / (glide_t / 4.0 * sr)).exp() } else { 0.0 };
        let vib_depth = vibrato_semis(c.vibrato);
        let vib_inc = VIBRATO_HZ * inv_sr;
        let nyq = 0.45 * sr;
        let mut vib_phase = self.vib_phase;
        let ratio = self.ratio;
        let inv_n = 1.0 / n as f32;
        // fixed per-voice gain: never depends on how many voices are sounding
        let gain = if mono { p.gain } else { p.poly_gain };
        for (vi, v) in self.voices.iter_mut().enumerate().filter(|(_, v)| v.active) {
            let mut ph = vib_phase;
            let mut bend = bend0;
            let vel_amp = 0.35 + 0.65 * v.vel * v.vel;
            let vel_env = 0.55 + 0.45 * v.vel;
            let env_oct = p.env_amount * vel_env;
            for i in 0..n {
                let f = i as f32 * inv_n;
                // pitch with glide (exponential, in semitones) and vibrato
                v.bend *= glide_keep;
                bend += (c.bend - bend) * bend_a;
                // pitch bend rides on top of glide
                let pitch = v.target + v.bend + bend;
                let vib = if vib_depth > 0.0 { vib_depth * fast_sin(ph) } else { 0.0 };
                ph += vib_inc;
                if ph >= 1.0 {
                    ph -= 1.0;
                }
                let base = 440.0 * exp2((pitch + vib - 69.0) / 12.0) * v.drift;
                let aenv = v.amp.tick();
                let fenv = v.fenv.tick();
                // oscillators
                let mut mix = 0.0;
                for o in 0..3 {
                    let dt = (base * ratio[o] * inv_sr).min(0.45);
                    let t = v.phase[o];
                    mix += p.osc[o].level * osc_sample(p.osc[o].wave, t, dt);
                    let mut t2 = t + dt;
                    if t2 >= 1.0 {
                        t2 -= 1.0;
                    }
                    v.phase[o] = t2;
                }
                // filter cutoff: knob + key tracking + envelope (octaves)
                let oct = oct0 + (oct1 - oct0) * f + p.key_track * (pitch - 60.0) / 12.0 + env_oct * fenv;
                let fc = (40.0 * exp2(oct)).clamp(20.0, nyq);
                let g = fast_tan(PI * fc * inv_sr);
                let k = k0 + (k1 - k0) * f;
                let y = v.filter.tick(mix * 0.42 * p.drive, g, k);
                let s = y * aenv * vel_amp * gain;
                out[i * 2] += s;
                out[i * 2 + 1] += s;
            }
            bend_end = bend;
            if vi == 0 {
                self.last_pitch = v.target + v.bend + bend;
            }
            v.filter.flush_denormals();
            if v.bend.abs() < 1e-5 {
                v.bend = 0.0;
            }
            if v.amp.is_idle() {
                v.active = false;
                v.filter.reset();
            }
        }
        vib_phase += vib_inc * n as f32;
        self.vib_phase = vib_phase.fract();
        self.bend_s = if (bend_end - c.bend).abs() < 1e-6 { c.bend } else { bend_end };
    }
}

/// sin(2π·x) for x in 0..1, parabolic approximation (error < 0.1 %).
#[inline(always)]
pub fn fast_sin(x: f32) -> f32 {
    // map to -0.5..0.5
    let x = if x > 0.5 { x - 1.0 } else { x };
    let y = 8.0 * x - 16.0 * x * x.abs();
    0.225 * (y * y.abs() - y) + y
}

/// 2^x, accurate to ~1e-4 relative; fine for pitch and cutoff.
#[inline(always)]
pub fn exp2(x: f32) -> f32 {
    let x = x.clamp(-60.0, 60.0);
    let fl = x.floor();
    let f = x - fl;
    // 2^f on [0,1): 5th-order polynomial
    let p = 1.0 + f * (0.693_147_2 + f * (0.240_226_5 + f * (0.055_504_1 + f * (0.009_618_1 + f * 0.001_333_6))));
    p * f32::from_bits((((fl as i32) + 127) as u32) << 23)
}

/// tan(x) for 0 <= x < ~1.42 (cutoff below 0.45·sr), Padé approximant with
/// one correction; relative error < 0.2 % in the audio range.
#[inline(always)]
pub fn fast_tan(x: f32) -> f32 {
    if x < 1.2 {
        let x2 = x * x;
        x * (945.0 - 105.0 * x2 + x2 * x2) / (945.0 - 420.0 * x2 + 15.0 * x2 * x2)
    } else {
        x.tan()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    fn controls() -> MonoControls {
        MonoControls { cutoff: 0.5, resonance: 0.3, glide: 0.0, vibrato: 0.0, bend: 0.0 }
    }

    fn run(s: &mut MonoSynth, frames: usize, c: MonoControls) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        for ch in out.chunks_mut(128) {
            let n = ch.len() / 2;
            s.render(ch, n, c);
        }
        out
    }

    fn assert_clean(buf: &[f32]) {
        for (i, x) in buf.iter().enumerate() {
            assert!(x.is_finite(), "non-finite sample at {i}");
            assert!(*x == 0.0 || x.abs() >= f32::MIN_POSITIVE, "denormal sample {x:e} at {i}");
        }
    }

    #[test]
    fn approximations_are_accurate() {
        for i in 0..1000 {
            let x = -10.0 + 20.0 * i as f32 / 1000.0;
            assert!((exp2(x) / 2f32.powf(x) - 1.0).abs() < 2e-4, "exp2({x})");
            let p = i as f32 / 1000.0;
            assert!((fast_sin(p) - (2.0 * PI * p).sin()).abs() < 2e-3, "sin({p})");
            let t = 1.4 * i as f32 / 1000.0;
            assert!((fast_tan(t) / t.tan().max(1e-9) - 1.0).abs() < 3e-3 || t == 0.0, "tan({t})");
            assert!((tanh_fast(x * 0.3) - (x * 0.3).tanh()).abs() < 0.03);
        }
    }

    /// Energy at non-harmonic frequencies (aliases) relative to total.
    fn alias_ratio(wave: Wave, f: f32, blep: bool) -> f32 {
        let n = 4800;
        let dt = f / SR;
        let mut t = 0.0f32;
        let mut x = Vec::with_capacity(n);
        for _ in 0..n {
            let s = if blep {
                osc_sample(wave, t, dt)
            } else {
                match wave {
                    Wave::Saw => 2.0 * t - 1.0,
                    Wave::Square => if t < 0.5 { 1.0 } else { -1.0 },
                    Wave::Triangle => if t < 0.5 { 4.0 * t - 1.0 } else { 3.0 - 4.0 * t },
                }
            };
            x.push(s);
            t += dt;
            if t >= 1.0 {
                t -= 1.0;
            }
        }
        // DFT magnitude at 10 Hz resolution (n = 4800 at 48 kHz)
        let mut harm = 0f64;
        let mut total = 0f64;
        for bin in 1..n / 2 {
            let w = 2.0 * std::f64::consts::PI * bin as f64 / n as f64;
            let (mut re, mut im) = (0f64, 0f64);
            for (i, s) in x.iter().enumerate() {
                re += *s as f64 * (w * i as f64).cos();
                im -= *s as f64 * (w * i as f64).sin();
            }
            let p = re * re + im * im;
            total += p;
            let hz = bin as f32 * 10.0;
            if (hz / f - (hz / f).round()).abs() * f < 15.0 {
                harm += p;
            }
        }
        ((total - harm) / total) as f32
    }

    #[test]
    fn oscillators_are_band_limited() {
        // 2.99 kHz: harmonics alias badly without correction
        for w in [Wave::Saw, Wave::Square, Wave::Triangle] {
            let naive = alias_ratio(w, 2990.0, false);
            let bl = alias_ratio(w, 2990.0, true);
            assert!(bl < naive * 0.5, "{w:?}: band-limited {bl} vs naive {naive}");
        }
    }

    #[test]
    fn ladder_is_stable_at_max_resonance() {
        let mut f = Ladder::default();
        let mut rng = Rng(99);
        let mut peak = 0f32;
        for fc in [30.0f32, 200.0, 1000.0, 5000.0, 15000.0, 21000.0] {
            let g = fast_tan(PI * fc.min(0.45 * SR) / SR);
            for i in 0..48000 {
                // loud noise burst, then a loud low square, then silence
                let x = if i < 4800 { 3.0 * rng.next_f32() } else if i < 24000 { if (i / 100) % 2 == 0 { 2.0 } else { -2.0 } } else { 0.0 };
                let y = f.tick(x, g, resonance_k(1.0));
                assert!(y.is_finite());
                peak = peak.max(y.abs());
                for s in f.state() {
                    assert!(s.is_finite() && s.abs() < 10.0, "state {s} at fc {fc}");
                }
            }
            f.flush_denormals();
        }
        assert!(peak <= 1.5, "peak {peak}");
        // just below self-oscillation it rings but decays to zero (no denormals left)
        let mut f = Ladder::default();
        let g = fast_tan(PI * 1000.0 / SR);
        f.tick(1.0, g, 3.6);
        for _ in 0..48000 * 4 {
            f.tick(0.0, g, 3.6);
            f.flush_denormals();
        }
        assert!(f.state().iter().all(|s| *s == 0.0 || s.abs() >= f32::MIN_POSITIVE));
        assert!(f.state().iter().all(|s| s.abs() < 1e-6));
    }

    #[test]
    fn legato_does_not_retrigger_and_glides() {
        let mut s = MonoSynth::new(SR);
        s.set_preset(1); // Lead
        s.set_voice_mode(VoiceMode::Mono);
        let c = MonoControls { glide: 0.5, ..controls() }; // 0.3 s
        s.note_on(60, 100, c.glide);
        run(&mut s, 4800, c);
        assert_eq!(s.mono_pitch(), Some(60.0));
        assert_ne!(s.mono_amp_stage(), Stage::Attack, "attack finished");
        // second key while the first is held: legato (no new attack), pitch glides
        s.note_on(67, 100, c.glide);
        assert_ne!(s.mono_amp_stage(), Stage::Attack, "legato must not retrigger");
        run(&mut s, 480, c); // 10 ms
        let p = s.mono_pitch().unwrap();
        assert!(p > 60.2 && p < 66.0, "gliding, at {p}");
        run(&mut s, 48000, c);
        assert!((s.mono_pitch().unwrap() - 67.0).abs() < 0.01);
        // release the top key: back to the held one (last-note priority), still legato
        s.note_off(67);
        run(&mut s, 48000, c);
        assert!((s.mono_pitch().unwrap() - 60.0).abs() < 0.01);
        assert_ne!(s.mono_amp_stage(), Stage::Release);
        // releasing a key that isn't sounding changes nothing
        s.note_on(64, 100, c.glide);
        s.note_off(60);
        assert_eq!(s.voices[0].target, 64.0);
        // last key up: release
        s.note_off(64);
        assert_eq!(s.mono_amp_stage(), Stage::Release);
        // a new phrase retriggers the envelope
        s.note_on(62, 100, c.glide);
        assert_eq!(s.mono_amp_stage(), Stage::Attack);
    }

    #[test]
    fn pitch_bend_is_smooth_and_works_during_glide() {
        let mut s = MonoSynth::new(SR);
        s.set_preset(1);
        s.set_voice_mode(VoiceMode::Mono);
        let mut c = MonoControls { glide: 0.6, ..controls() };
        s.note_on(60, 100, c.glide);
        run(&mut s, 4800, c);
        assert!((s.sounding_pitch() - 60.0).abs() < 1e-4);
        // full bend up (+2): reaches it smoothly, not in one jump
        c.bend = 2.0;
        run(&mut s, 32, c);
        let p = s.sounding_pitch();
        assert!(p > 60.0 && p < 61.9, "smoothed, at {p}");
        run(&mut s, 4800, c);
        assert!((s.sounding_pitch() - 62.0).abs() < 1e-3);
        // bend while gliding to another note: both add up
        s.note_on(72, 100, c.glide);
        run(&mut s, 2400, c);
        let mid = s.sounding_pitch() - 2.0;
        assert!(mid > 60.5 && mid < 71.5, "mid-glide {mid}");
        run(&mut s, 48000 * 2, c);
        assert!((s.sounding_pitch() - 74.0).abs() < 1e-3);
        // full bend down with a 12-semitone range
        c.bend = -12.0;
        run(&mut s, 4800, c);
        assert!((s.sounding_pitch() - 60.0).abs() < 1e-3);
        // and back to centre
        c.bend = 0.0;
        run(&mut s, 4800, c);
        assert!((s.sounding_pitch() - 72.0).abs() < 1e-3);
    }

    #[test]
    fn zero_glide_jumps() {
        let mut s = MonoSynth::new(SR);
        s.set_preset(0);
        s.set_voice_mode(VoiceMode::Mono);
        let c = controls();
        s.note_on(40, 100, 0.0);
        run(&mut s, 960, c);
        s.note_on(52, 100, 0.0);
        assert_eq!(s.mono_pitch(), Some(52.0));
        run(&mut s, 64, c);
        assert_eq!(s.mono_pitch(), Some(52.0));
    }

    #[test]
    fn last_note_priority_with_three_keys() {
        let mut s = MonoSynth::new(SR);
        s.set_preset(0);
        s.set_voice_mode(VoiceMode::Mono);
        for k in [48, 52, 55] {
            s.note_on(k, 90, 0.0);
        }
        assert_eq!(s.voices[0].target, 55.0);
        s.note_off(52); // not the sounding key
        assert_eq!(s.voices[0].target, 55.0);
        s.note_off(55);
        assert_eq!(s.voices[0].target, 48.0);
    }

    #[test]
    fn sustain_pedal_holds_mono_note() {
        let mut s = MonoSynth::new(SR);
        s.set_preset(0);
        s.set_voice_mode(VoiceMode::Mono);
        s.sustain(true);
        s.note_on(48, 90, 0.0);
        s.note_off(48);
        assert_ne!(s.mono_amp_stage(), Stage::Release);
        s.sustain(false);
        assert_eq!(s.mono_amp_stage(), Stage::Release);
    }

    #[test]
    fn output_is_finite_and_free_of_denormals() {
        for (preset, mode) in (0..MONO_PRESETS.len()).flat_map(|p| VoiceMode::ALL.map(|m| (p, m))) {
            let mut s = MonoSynth::new(SR);
            s.set_preset(preset);
            s.set_voice_mode(mode);
            let mut c = MonoControls { cutoff: 1.0, resonance: 1.0, glide: 0.3, vibrato: 1.0, bend: 12.0 };
            for k in [24u8, 36, 60, 96, 108, 127, 0] {
                s.note_on(k, 127, c.glide);
            }
            let out = run(&mut s, 24000, c);
            assert_clean(&out);
            assert!(out.iter().all(|x| x.abs() < 4.0));
            c.cutoff = 0.0;
            let out = run(&mut s, 24000, c);
            assert_clean(&out);
            for k in [24u8, 36, 60, 96, 108, 127, 0] {
                s.note_off(k);
            }
            let out = run(&mut s, 48000 * 3, c);
            assert_clean(&out);
            assert_eq!(s.active_voices(), 0, "all voices finished");
            // silence afterwards is exactly zero
            let out = run(&mut s, 4800, c);
            assert!(out.iter().all(|x| *x == 0.0));
        }
    }

    #[test]
    fn poly_mode_plays_chords_and_steals() {
        for preset in 0..MONO_PRESETS.len() {
            let mut s = MonoSynth::new(SR);
            s.set_preset(preset);
            assert_eq!(s.voice_mode(), VoiceMode::Poly, "Poly is the default on every preset");
            for k in [60, 64, 67] {
                s.note_on(k, 100, 0.0);
            }
            assert_eq!(s.active_voices(), 3);
            for k in 70..90 {
                s.note_on(k, 100, 0.0);
            }
            assert_eq!(s.active_voices(), POLY_VOICES);
            let out = run(&mut s, 4800, controls());
            assert_clean(&out);
        }
    }

    use crate::analysis::band_db;

    fn lvl(buf: &[f32], f: f32) -> f32 {
        band_db(buf, SR, f, 2.0)
    }

    fn hz(key: u8) -> f32 {
        440.0 * 2f32.powf((key as f32 - 69.0) / 12.0)
    }

    const CHORD: [u8; 4] = [36, 60, 64, 67]; // C2 + C4 E4 G4

    /// Renders `keys` (pressed in that order) on `preset` in Poly at the preset's
    /// own cutoff/resonance, and returns 2 s of output after a 0.25 s settle.
    fn render_chord(preset: usize, keys: &[u8]) -> Vec<f32> {
        let p = &MONO_PRESETS[preset];
        let c = MonoControls { cutoff: p.cutoff, resonance: p.resonance, glide: p.glide, vibrato: 0.0, bend: 0.0 };
        let mut s = MonoSynth::new(SR);
        s.set_preset(preset);
        for &k in keys {
            s.note_on(k, 100, c.glide);
        }
        run(&mut s, 12000, c);
        run(&mut s, 96000, c)
    }

    #[test]
    fn poly_chord_with_low_note_keeps_every_note_audible() {
        for preset in 0..MONO_PRESETS.len() {
            let out = render_chord(preset, &CHORD);
            assert_clean(&out);
            let lv: Vec<f32> = CHORD.iter().map(|k| lvl(&out, hz(*k))).collect();
            let top = lv.iter().cloned().fold(f32::MIN, f32::max);
            for (k, l) in CHORD.iter().zip(&lv) {
                assert!(top - l < 12.0, "{}: key {k} at {l:.1} dB, {:.1} dB under the loudest ({lv:?})", MONO_PRESETS[preset].name, top - l);
            }
            // and the low note doesn't mask the upper ones: each upper note is
            // about as strong in the chord as when it's played on its own
            for &k in &CHORD[1..] {
                let alone = lvl(&render_chord(preset, &[k]), hz(k));
                let inchord = lvl(&out, hz(k));
                assert!(inchord > alone - 3.0, "{}: key {k} {inchord:.1} dB in the chord vs {alone:.1} alone", MONO_PRESETS[preset].name);
            }
        }
    }

    #[test]
    fn key_tracking_keeps_high_notes_bright() {
        // With key tracking each voice's filter follows its key, so a C4 keeps
        // roughly the timbre of a C2 instead of being buried under a cutoff set
        // for the bass. Brightness = 3rd harmonic relative to the fundamental.
        // Without tracking, C4's 3rd harmonic would sit ~2 octaves further up
        // the 24 dB/oct slope than C2's (about 48 dB darker).
        for (preset, p) in MONO_PRESETS.iter().enumerate() {
            assert!((0.5..=1.0).contains(&p.key_track), "{} key_track {}", p.name, p.key_track);
            let bright = |k: u8| {
                let out = render_chord(preset, &[k]);
                lvl(&out, 3.0 * hz(k)) - lvl(&out, hz(k))
            };
            let (low, high) = (bright(36), bright(60));
            assert!(high > low - 20.0, "{}: C4 brightness {high:.1} dB vs C2 {low:.1} dB", p.name);
        }
    }

    #[test]
    fn notes_in_any_order_give_the_same_voices() {
        // all 24 orders of the chord
        let mut orders = Vec::new();
        let k = CHORD;
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    for d in 0..4 {
                        let idx = [a, b, c, d];
                        if (0..4).all(|i| idx.contains(&i)) {
                            orders.push(idx.map(|i| k[i]));
                        }
                    }
                }
            }
        }
        assert_eq!(orders.len(), 24);
        for preset in 0..MONO_PRESETS.len() {
            let mut want: Option<[u64; 2]> = None;
            for o in &orders {
                let mut s = MonoSynth::new(SR);
                s.set_preset(preset);
                for &key in o {
                    s.note_on(key, 100, 0.0);
                }
                run(&mut s, 960, controls());
                assert_eq!(s.active_voices(), 4, "{o:?}");
                let mut bits = [0u64; 2];
                s.sounding(&mut bits);
                if let Some(w) = want {
                    assert_eq!(bits, w, "order {o:?}");
                } else {
                    want = Some(bits);
                }
                let mut keys: Vec<u8> = s.voices.iter().filter(|v| v.active).map(|v| v.key).collect();
                keys.sort();
                assert_eq!(keys, vec![36, 60, 64, 67]);
            }
            // in mono, High and Low pick the same key whatever the order
            for (pr, want_key) in [(NotePriority::High, 67u8), (NotePriority::Low, 36)] {
                for o in &orders {
                    let mut s = MonoSynth::new(SR);
                    s.set_preset(preset);
                    s.set_voice_mode(VoiceMode::Mono);
                    s.set_priority(pr);
                    for &key in o {
                        s.note_on(key, 100, 0.0);
                    }
                    assert_eq!(s.mono_pitch(), Some(want_key as f32), "{pr:?} {o:?}");
                }
            }
        }
    }

    #[test]
    fn mono_note_priorities() {
        let c = controls();
        // Last (default): the newest key, falling back in order
        let mut s = MonoSynth::new(SR);
        s.set_voice_mode(VoiceMode::Mono);
        assert_eq!(s.priority(), NotePriority::Last);
        for k in [48, 60, 55] {
            s.note_on(k, 100, 0.0);
        }
        assert_eq!(s.mono_pitch(), Some(55.0));
        s.note_off(55);
        run(&mut s, 64, c); // glide off: the fall-back jumps within a block
        assert_eq!(s.mono_pitch(), Some(60.0));

        // High: a lower key under a held high one changes nothing (no retrigger)
        let mut s = MonoSynth::new(SR);
        s.set_voice_mode(VoiceMode::Mono);
        s.set_priority(NotePriority::High);
        s.note_on(60, 100, 0.0);
        run(&mut s, 4800, c);
        s.note_on(48, 100, 0.0);
        assert_eq!(s.mono_pitch(), Some(60.0));
        assert_ne!(s.mono_amp_stage(), Stage::Attack);
        s.note_on(67, 100, 0.0);
        assert_eq!(s.mono_pitch(), Some(67.0));
        assert_ne!(s.mono_amp_stage(), Stage::Attack, "legato");
        s.note_off(48); // not sounding
        assert_eq!(s.mono_pitch(), Some(67.0));
        s.note_off(67);
        run(&mut s, 64, c);
        assert_eq!(s.mono_pitch(), Some(60.0), "falls back to the highest still held");
        s.note_off(60);
        assert_eq!(s.mono_amp_stage(), Stage::Release);

        // Low: the bass note holds while higher keys come and go
        let mut s = MonoSynth::new(SR);
        s.set_voice_mode(VoiceMode::Mono);
        s.set_priority(NotePriority::Low);
        s.note_on(48, 100, 0.0);
        s.note_on(60, 100, 0.0);
        assert_eq!(s.mono_pitch(), Some(48.0));
        s.note_on(36, 100, 0.0);
        assert_eq!(s.mono_pitch(), Some(36.0));
        s.note_off(36);
        run(&mut s, 64, c);
        assert_eq!(s.mono_pitch(), Some(48.0));
        // switching priority while keys are held takes effect at once
        s.set_priority(NotePriority::High);
        run(&mut s, 64, c);
        assert_eq!(s.mono_pitch(), Some(60.0));
    }

    #[test]
    fn switching_voice_mode_releases_notes_and_leaves_nothing_stuck() {
        let c = controls();
        let mut s = MonoSynth::new(SR);
        for k in CHORD {
            s.note_on(k, 100, 0.0);
        }
        s.set_voice_mode(VoiceMode::Mono);
        // the keys are released by the player later: nothing must hang
        for k in CHORD {
            s.note_off(k);
        }
        run(&mut s, 48000 * 2, c);
        assert_eq!(s.active_voices(), 0);
        s.note_on(40, 100, 0.0);
        s.note_on(52, 100, 0.0);
        assert_eq!(s.active_voices(), 1, "mono: one voice");
        s.set_voice_mode(VoiceMode::Poly);
        s.note_off(40);
        s.note_off(52);
        run(&mut s, 48000 * 2, c);
        assert_eq!(s.active_voices(), 0);
    }

    /// Regression: a panic shortened the envelopes to 10 ms for good, so every
    /// later note died away at once. They are now restored when the fade ends.
    #[test]
    fn panic_restores_envelopes() {
        let c = controls();
        let mut s = MonoSynth::new(SR);
        s.note_on(60, 100, 0.0);
        run(&mut s, 4800, c);
        s.panic();
        run(&mut s, 4800, c);
        assert_eq!(s.active_voices(), 0);
        run(&mut s, 128, c);
        s.note_on(60, 100, 0.0);
        let out = run(&mut s, 48000, c);
        let late = &out[out.len() - 9600..];
        let rms = (late.iter().map(|x| x * x).sum::<f32>() / late.len() as f32).sqrt();
        assert!(rms > 0.01, "held note still sounding after 1 s (rms {rms})");
    }
}
