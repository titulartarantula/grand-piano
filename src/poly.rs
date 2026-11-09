//! The polysynth: an 8-voice-plus analogue-style polyphonic synth with three
//! voice architectures, in the style of the classic early-1980s polysynths.
//!
//! - **Juno-style:** one DCO with a band-limited saw and a pulse whose width
//!   is swept by an LFO (PWM), a square sub-oscillator an octave down, a
//!   non-resonant high-pass, a 24 dB/oct resonant low-pass, and the
//!   characteristic built-in stereo bucket-brigade chorus (modes I and II).
//! - **Prophet-style:** two oscillators and "poly-mod": the filter envelope
//!   bends oscillator A's pitch and oscillator B modulates the filter cutoff
//!   at audio rate, for a slightly gritty, vocal edge. 24 dB/oct low-pass.
//! - **Oberheim-style:** two widely detuned saws through a 12 dB/oct
//!   state-variable filter (the SEM's softer, rounder slope).
//!
//! It reuses the Moog-style synth's building blocks (PolyBLEP oscillators,
//! zero-delay-feedback ladder and SVF filters, analogue-style envelopes).
//! Ten voices; a new note takes a free voice, else the quietest released
//! voice, else the oldest held one (its envelopes restart from their current
//! level, so the steal doesn't click). Filter keyboard tracking per preset,
//! and ±1.5 cents of random drift per note, as on the Moog-style Synth.
//! The mod wheel adds vibrato on every preset.

use crate::synth::{Adsr, AdsrTimes, Ladder, Rng, Stage, Svf, VIBRATO_HZ, Wave, exp2, fast_sin, fast_tan, flush, osc_sample, pulse_sample, resonance_k, tanh_fast, vibrato_semis};
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolyModel {
    Juno,
    Prophet,
    Oberheim,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chorus {
    Off,
    /// slow and gentle (0.5 Hz)
    I,
    /// faster and deeper (0.83 Hz)
    II,
}

#[derive(Clone, Copy, Debug)]
pub struct PolyPreset {
    pub name: &'static str,
    pub model: PolyModel,
    /// oscillator A: saw and pulse levels, pulse width, PWM depth and rate
    pub saw: f32,
    pub pulse: f32,
    pub width: f32,
    pub pwm: f32,
    pub pwm_hz: f32,
    /// oscillator A fine tune in cents (the Oberheim's fat detune)
    pub a_cents: f32,
    /// sub-oscillator (square, one octave down) and noise
    pub sub: f32,
    pub noise: f32,
    /// oscillator B: wave, semitones, cents, level (level 0 = off)
    pub b_wave: Wave,
    pub b_semis: f32,
    pub b_cents: f32,
    pub b_level: f32,
    /// poly-mod: filter envelope -> osc A pitch (semitones), osc B -> cutoff (octaves)
    pub pm_env_pitch: f32,
    pub pm_b_filter: f32,
    pub hpf_hz: f32,
    /// filter envelope amount (octaves), keyboard tracking, level into the filter
    pub env_amount: f32,
    pub key_track: f32,
    pub drive: f32,
    pub filter_env: AdsrTimes,
    pub amp_env: AdsrTimes,
    pub cutoff: f32,
    pub resonance: f32,
    pub chorus: Chorus,
    /// fixed per-voice output gain
    pub gain: f32,
}

pub const POLY_PRESETS: [PolyPreset; 5] = [
    PolyPreset {
        name: "80s Pad",
        model: PolyModel::Juno,
        saw: 0.75,
        pulse: 0.6,
        width: 0.5,
        pwm: 0.32,
        pwm_hz: 0.55,
        a_cents: 0.0,
        sub: 0.45,
        noise: 0.0,
        b_wave: Wave::Saw,
        b_semis: 0.0,
        b_cents: 0.0,
        b_level: 0.0,
        pm_env_pitch: 0.0,
        pm_b_filter: 0.0,
        hpf_hz: 70.0,
        env_amount: 1.3,
        key_track: 0.6,
        drive: 0.5,
        filter_env: AdsrTimes { attack: 0.9, decay: 2.2, sustain: 0.55, release: 1.6 },
        amp_env: AdsrTimes { attack: 0.55, decay: 1.5, sustain: 0.9, release: 1.5 },
        cutoff: 0.5,
        resonance: 0.18,
        chorus: Chorus::II,
        gain: 0.95,
    },
    PolyPreset {
        name: "Brass Stab",
        model: PolyModel::Prophet,
        saw: 1.0,
        pulse: 0.0,
        width: 0.5,
        pwm: 0.0,
        pwm_hz: 0.5,
        a_cents: 0.0,
        sub: 0.0,
        noise: 0.0,
        b_wave: Wave::Saw,
        b_semis: 0.0,
        b_cents: 7.0,
        b_level: 0.9,
        pm_env_pitch: 0.0,
        pm_b_filter: 0.25,
        hpf_hz: 30.0,
        env_amount: 3.0,
        key_track: 0.7,
        drive: 0.7,
        filter_env: AdsrTimes { attack: 0.035, decay: 0.32, sustain: 0.32, release: 0.22 },
        amp_env: AdsrTimes { attack: 0.012, decay: 0.6, sustain: 0.75, release: 0.2 },
        cutoff: 0.36,
        resonance: 0.18,
        chorus: Chorus::Off,
        gain: 0.62,
    },
    PolyPreset {
        name: "Synthwave Lead",
        model: PolyModel::Prophet,
        saw: 1.0,
        pulse: 0.0,
        width: 0.5,
        pwm: 0.0,
        pwm_hz: 0.5,
        a_cents: 0.0,
        sub: 0.0,
        noise: 0.0,
        b_wave: Wave::Square,
        b_semis: 12.0,
        b_cents: 5.0,
        b_level: 0.45,
        pm_env_pitch: 0.15,
        pm_b_filter: 0.0,
        hpf_hz: 40.0,
        env_amount: 2.0,
        key_track: 0.8,
        drive: 0.65,
        filter_env: AdsrTimes { attack: 0.005, decay: 0.55, sustain: 0.5, release: 0.35 },
        amp_env: AdsrTimes { attack: 0.005, decay: 1.0, sustain: 1.0, release: 0.35 },
        cutoff: 0.52,
        resonance: 0.35,
        chorus: Chorus::I,
        gain: 0.52,
    },
    PolyPreset {
        name: "Juno Strings",
        model: PolyModel::Juno,
        saw: 0.0,
        pulse: 1.0,
        width: 0.5,
        pwm: 0.4,
        pwm_hz: 0.75,
        a_cents: 0.0,
        sub: 0.15,
        noise: 0.0,
        b_wave: Wave::Saw,
        b_semis: 0.0,
        b_cents: 0.0,
        b_level: 0.0,
        pm_env_pitch: 0.0,
        pm_b_filter: 0.0,
        hpf_hz: 220.0,
        env_amount: 0.6,
        key_track: 0.7,
        drive: 0.5,
        filter_env: AdsrTimes { attack: 0.25, decay: 1.0, sustain: 0.7, release: 0.9 },
        amp_env: AdsrTimes { attack: 0.22, decay: 1.0, sustain: 1.0, release: 0.9 },
        cutoff: 0.62,
        resonance: 0.1,
        chorus: Chorus::I,
        gain: 0.65,
    },
    PolyPreset {
        name: "OB Brass",
        model: PolyModel::Oberheim,
        saw: 1.0,
        pulse: 0.0,
        width: 0.5,
        pwm: 0.0,
        pwm_hz: 0.5,
        a_cents: -9.0,
        sub: 0.0,
        noise: 0.0,
        b_wave: Wave::Saw,
        b_semis: 0.0,
        b_cents: 9.0,
        b_level: 1.0,
        pm_env_pitch: 0.0,
        pm_b_filter: 0.0,
        hpf_hz: 30.0,
        env_amount: 2.6,
        key_track: 0.7,
        drive: 0.7,
        filter_env: AdsrTimes { attack: 0.06, decay: 0.5, sustain: 0.45, release: 0.3 },
        amp_env: AdsrTimes { attack: 0.03, decay: 0.8, sustain: 0.8, release: 0.3 },
        cutoff: 0.42,
        resonance: 0.3,
        chorus: Chorus::Off,
        gain: 0.55,
    },
];

pub const POLYSYNTH_VOICES: usize = 10;
const DRIFT_CENTS: f32 = 1.5;

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    key: u8,
    vel: f32,
    drift: f32,
    pa: f32,
    pb: f32,
    psub: f32,
    ladder: Ladder,
    svf: Svf,
    hp: f32,
    amp: Adsr,
    fenv: Adsr,
    sustained: bool,
    age: u64,
}

impl Voice {
    fn new() -> Voice {
        Voice {
            active: false,
            key: 0,
            vel: 0.0,
            drift: 1.0,
            pa: 0.0,
            pb: 0.0,
            psub: 0.0,
            ladder: Ladder::default(),
            svf: Svf::default(),
            hp: 0.0,
            amp: Adsr::default(),
            fenv: Adsr::default(),
            sustained: false,
            age: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PolyControls {
    pub cutoff: f32,
    pub resonance: f32,
    /// mod wheel: vibrato
    pub vibrato: f32,
    pub bend: f32,
}

/// The Juno-style bucket-brigade chorus: one delay line, two taps swept in
/// opposite directions by a triangle LFO, one per side, with the BBD's
/// high-frequency loss.
struct JunoChorus {
    buf: Vec<f32>,
    w: usize,
    lfo: f32,
    lp: [f32; 2],
}

impl JunoChorus {
    fn new(sr: f32) -> Self {
        JunoChorus { buf: vec![0.0; ((0.008 * sr) as usize + 4).next_power_of_two()], w: 0, lfo: 0.0, lp: [0.0; 2] }
    }
    #[inline(always)]
    fn read(&self, d: f32) -> f32 {
        let m = self.buf.len() - 1;
        let pos = self.w as f32 + self.buf.len() as f32 - d;
        let i = pos as usize;
        let t = pos - i as f32;
        let a = self.buf[i & m];
        let b = self.buf[(i + 1) & m];
        a + (b - a) * t
    }
    #[inline(always)]
    fn tick(&mut self, x: f32, mode: Chorus, sr: f32, lp_a: f32) -> (f32, f32) {
        let m = self.buf.len() - 1;
        self.buf[self.w] = x;
        self.w = (self.w + 1) & m;
        let (rate, lo, hi) = match mode {
            Chorus::II => (0.83, 0.00166, 0.00535),
            _ => (0.5, 0.00166, 0.00535),
        };
        let depth = if mode == Chorus::II { 1.0 } else { 0.7 };
        self.lfo += rate / sr;
        if self.lfo >= 1.0 {
            self.lfo -= 1.0;
        }
        let tri = if self.lfo < 0.5 { 4.0 * self.lfo - 1.0 } else { 3.0 - 4.0 * self.lfo };
        let mid = 0.5 * (lo + hi) * sr;
        let sw = 0.5 * (hi - lo) * sr * depth;
        let wl = self.read(mid + sw * tri);
        let wr = self.read(mid - sw * tri);
        self.lp[0] += (wl - self.lp[0]) * lp_a;
        self.lp[1] += (wr - self.lp[1]) * lp_a;
        (0.75 * (x + self.lp[0]), 0.75 * (x + self.lp[1]))
    }
    fn clear(&mut self) {
        for x in self.buf.iter_mut() {
            *x = 0.0;
        }
        self.lp = [0.0; 2];
    }
}

pub struct Polysynth {
    sr: f32,
    preset: usize,
    voices: [Voice; POLYSYNTH_VOICES],
    pedal: bool,
    clock: u64,
    rng: Rng,
    vib_phase: f32,
    pwm_phase: f32,
    cut_s: f32,
    res_s: f32,
    bend_s: f32,
    chorus: JunoChorus,
    panicked: bool,
    quiet_frames: usize,
}

impl Polysynth {
    pub fn new(sr: f32) -> Self {
        let mut s = Polysynth {
            sr,
            preset: 0,
            voices: [Voice::new(); POLYSYNTH_VOICES],
            pedal: false,
            clock: 0,
            rng: Rng(0x5eed_1234),
            vib_phase: 0.0,
            pwm_phase: 0.0,
            cut_s: -1.0,
            res_s: 0.0,
            bend_s: 0.0,
            chorus: JunoChorus::new(sr),
            panicked: false,
            quiet_frames: usize::MAX / 2,
        };
        s.set_preset(0);
        s
    }

    pub fn preset(&self) -> usize {
        self.preset
    }

    fn p(&self) -> &'static PolyPreset {
        &POLY_PRESETS[self.preset]
    }

    pub fn set_preset(&mut self, idx: usize) {
        let idx = idx.min(POLY_PRESETS.len() - 1);
        if idx != self.preset {
            self.release_all();
        }
        self.preset = idx;
        let p = &POLY_PRESETS[idx];
        let sr = self.sr;
        for v in self.voices.iter_mut() {
            v.amp.set(&p.amp_env, sr);
            v.fenv.set(&p.filter_env, sr);
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    pub fn busy(&self) -> bool {
        self.quiet_frames < (self.sr * 0.05) as usize
    }

    pub fn sounding(&self, out: &mut [u64; 2]) {
        for v in self.voices.iter().filter(|v| v.active && v.amp.stage != Stage::Release && v.amp.stage != Stage::Idle) {
            out[(v.key / 64) as usize] |= 1 << (v.key % 64);
        }
    }

    /// Keys of the voices currently playing (tests).
    pub fn voice_keys(&self) -> Vec<u8> {
        let mut k: Vec<u8> = self.voices.iter().filter(|v| v.active).map(|v| v.key).collect();
        k.sort();
        k
    }

    /// Which voice a new note takes: the same key's voice, a free one, the
    /// quietest released one, or the oldest held one.
    fn pick_voice(&self, key: u8) -> usize {
        if let Some(i) = self.voices.iter().position(|v| v.active && v.key == key) {
            return i;
        }
        if let Some(i) = self.voices.iter().position(|v| !v.active) {
            return i;
        }
        let released = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, v)| v.amp.stage == Stage::Release)
            .min_by(|a, b| a.1.amp.level.total_cmp(&b.1.amp.level))
            .map(|(i, _)| i);
        released.unwrap_or_else(|| self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap())
    }

    /// `vel` is the shaped velocity, 1..127.
    pub fn note_on(&mut self, key: u8, vel: u8) {
        self.clock += 1;
        let slot = self.pick_voice(key);
        let drift = 2f32.powf(self.rng.next_f32() * DRIFT_CENTS / 1200.0);
        let r = &mut self.rng;
        let v = &mut self.voices[slot];
        if !v.active {
            v.ladder.reset();
            v.svf = Svf::default();
            v.hp = 0.0;
            v.pa = r.next_f32() * 0.5 + 0.5;
            v.pb = r.next_f32() * 0.5 + 0.5;
            v.psub = r.next_f32() * 0.5 + 0.5;
        }
        v.active = true;
        v.key = key;
        v.vel = vel as f32 / 127.0;
        v.drift = drift;
        v.sustained = false;
        v.age = self.clock;
        // envelopes restart from their current level (no click on a steal)
        v.amp.gate_on();
        v.fenv.gate_on();
        self.quiet_frames = 0;
    }

    pub fn note_off(&mut self, key: u8) {
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

    pub fn sustain(&mut self, down: bool) {
        self.pedal = down;
        if !down {
            for v in self.voices.iter_mut().filter(|v| v.sustained) {
                v.sustained = false;
                v.amp.gate_off();
                v.fenv.gate_off();
            }
        }
    }

    pub fn release_all(&mut self) {
        for v in self.voices.iter_mut() {
            v.sustained = false;
            v.amp.gate_off();
            v.fenv.gate_off();
        }
    }

    pub fn panic(&mut self) {
        self.panicked = true;
        let t = AdsrTimes { attack: 0.01, decay: 0.01, sustain: 0.0, release: 0.01 };
        for v in self.voices.iter_mut() {
            v.sustained = false;
            v.amp.set(&t, self.sr);
            v.amp.gate_off();
            v.fenv.gate_off();
        }
    }

    /// Adds `n` stereo frames into `out` (interleaved).
    pub fn render(&mut self, out: &mut [f32], n: usize, c: PolyControls) {
        let sr = self.sr;
        let p = *self.p();
        let any_voice = self.voices.iter().any(|v| v.active);
        if !any_voice {
            if self.panicked {
                self.panicked = false;
                for v in self.voices.iter_mut() {
                    v.amp.set(&p.amp_env, sr);
                    v.fenv.set(&p.filter_env, sr);
                }
            }
            if !self.busy() {
                self.vib_phase = (self.vib_phase + VIBRATO_HZ * n as f32 / sr).fract();
                self.pwm_phase = (self.pwm_phase + p.pwm_hz * n as f32 / sr).fract();
                self.bend_s = c.bend;
                return;
            }
        }
        let inv_sr = 1.0 / sr;
        if self.cut_s < 0.0 {
            self.cut_s = c.cutoff;
            self.res_s = c.resonance;
        }
        let a = 1.0 - (-(n as f32) / (0.005 * sr)).exp();
        let (cut0, res0) = (self.cut_s, self.res_s);
        self.cut_s += (c.cutoff - self.cut_s) * a;
        self.res_s += (c.resonance - self.res_s) * a;
        let oct0 = cut0 * 9.0;
        let oct1 = self.cut_s * 9.0;
        let ob = p.model == PolyModel::Oberheim;
        // ladder feedback, or the SVF's damping (1/Q: 1.4 at 0 .. 0.07 at max)
        let (k0, k1) = (resonance_k(res0), resonance_k(self.res_s));
        let r2_of = |r: f32| 1.4 * (1.0 - r.clamp(0.0, 1.0)).powf(1.3) + 0.07;
        let (r20, r21) = (r2_of(res0), r2_of(self.res_s));
        let vib_depth = vibrato_semis(c.vibrato);
        let vib_inc = VIBRATO_HZ * inv_sr;
        let pwm_inc = p.pwm_hz * inv_sr;
        let bend_a = 1.0 - (-1.0 / (0.003 * sr)).exp();
        let hp_a = (-2.0 * PI * p.hpf_hz * inv_sr).exp();
        let nyq = 0.45 * sr;
        let inv_n = 1.0 / n as f32;
        let ra = 2f32.powf(p.a_cents / 1200.0);
        let rb = 2f32.powf((p.b_semis + p.b_cents / 100.0) / 12.0);
        // per-sample shared modulation: vibrato, PWM LFO, bend
        let mut pitch_mod = [0f32; 128];
        let mut width = [0.5f32; 128];
        {
            let mut vp = self.vib_phase;
            let mut pp = self.pwm_phase;
            let mut bend = self.bend_s;
            for i in 0..n {
                bend += (c.bend - bend) * bend_a;
                let vib = if vib_depth > 0.0 { vib_depth * fast_sin(vp) } else { 0.0 };
                pitch_mod[i] = vib + bend;
                width[i] = (p.width + p.pwm * 0.5 * fast_sin(pp)).clamp(0.05, 0.95);
                vp += vib_inc;
                if vp >= 1.0 {
                    vp -= 1.0;
                }
                pp += pwm_inc;
                if pp >= 1.0 {
                    pp -= 1.0;
                }
            }
            self.vib_phase = vp;
            self.pwm_phase = pp;
            self.bend_s = if (bend - c.bend).abs() < 1e-6 { c.bend } else { bend };
        }
        let mut mono = [0f32; 128];
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let vel_amp = 0.4 + 0.6 * v.vel * v.vel;
            let env_oct = p.env_amount * (0.55 + 0.45 * v.vel);
            let key = v.key as f32;
            for i in 0..n {
                let f = i as f32 * inv_n;
                let aenv = v.amp.tick();
                let fenv = v.fenv.tick();
                let pitch = key + pitch_mod[i];
                let base = 440.0 * exp2((pitch - 69.0) / 12.0) * v.drift;
                // oscillator A (poly-mod: the filter envelope bends its pitch)
                let fa = if p.pm_env_pitch != 0.0 { base * ra * exp2(p.pm_env_pitch * fenv / 12.0) } else { base * ra };
                let dta = (fa * inv_sr).min(0.45);
                let mut x = 0.0;
                if p.saw > 0.0 {
                    x += p.saw * osc_sample(Wave::Saw, v.pa, dta);
                }
                if p.pulse > 0.0 {
                    x += p.pulse * pulse_sample(v.pa, dta, width[i]);
                }
                v.pa += dta;
                if v.pa >= 1.0 {
                    v.pa -= 1.0;
                }
                if p.sub > 0.0 {
                    let dts = dta * 0.5;
                    x += p.sub * osc_sample(Wave::Square, v.psub, dts);
                    v.psub += dts;
                    if v.psub >= 1.0 {
                        v.psub -= 1.0;
                    }
                }
                let mut b_out = 0.0;
                if p.b_level > 0.0 {
                    let dtb = (base * rb * inv_sr).min(0.45);
                    b_out = osc_sample(p.b_wave, v.pb, dtb);
                    x += p.b_level * b_out;
                    v.pb += dtb;
                    if v.pb >= 1.0 {
                        v.pb -= 1.0;
                    }
                }
                // high-pass (one pole)
                let hp_in = x;
                v.hp = hp_a * v.hp + (1.0 - hp_a) * hp_in;
                let x = hp_in - v.hp;
                // filter: knob + key tracking + envelope (+ poly-mod osc B), octaves
                let oct = oct0 + (oct1 - oct0) * f + p.key_track * (key - 60.0) / 12.0 + env_oct * fenv + p.pm_b_filter * b_out;
                let fc = (40.0 * exp2(oct)).clamp(20.0, nyq);
                let g = fast_tan(PI * fc * inv_sr);
                let y = if ob {
                    let r2 = r20 + (r21 - r20) * f;
                    let (lp, _) = v.svf.tick(tanh_fast(x * 0.42 * p.drive), g, r2);
                    lp
                } else {
                    let k = k0 + (k1 - k0) * f;
                    v.ladder.tick(x * 0.42 * p.drive, g, k)
                };
                mono[i] += y * aenv * vel_amp * p.gain;
            }
            v.ladder.flush_denormals();
            v.svf.flush_denormals();
            v.hp = flush(v.hp);
            if v.amp.is_idle() {
                v.active = false;
                v.ladder.reset();
                v.svf = Svf::default();
            }
        }
        // chorus (or straight stereo)
        let lp_a = 1.0 - (-2.0 * PI * 9000.0f32.min(nyq) * inv_sr).exp();
        let mut any = false;
        for i in 0..n {
            let x = mono[i];
            let (l, r) = if p.chorus == Chorus::Off { (x, x) } else { self.chorus.tick(x, p.chorus, sr, lp_a) };
            out[i * 2] += flush(l);
            out[i * 2 + 1] += flush(r);
            if l.abs() > 1e-6 || r.abs() > 1e-6 {
                any = true;
            }
        }
        self.chorus.lp = [flush(self.chorus.lp[0]), flush(self.chorus.lp[1])];
        if any || self.voices.iter().any(|v| v.active) {
            self.quiet_frames = 0;
        } else {
            self.quiet_frames += n;
            if !self.busy() {
                self.chorus.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::band_db;

    const SR: f32 = 48000.0;

    fn ctl(p: usize) -> PolyControls {
        PolyControls { cutoff: POLY_PRESETS[p].cutoff, resonance: POLY_PRESETS[p].resonance, vibrato: 0.0, bend: 0.0 }
    }

    fn run(s: &mut Polysynth, frames: usize, c: PolyControls) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        for ch in out.chunks_mut(128) {
            let n = ch.len() / 2;
            s.render(ch, n, c);
        }
        out
    }

    fn hz(key: u8) -> f32 {
        440.0 * 2f32.powf((key as f32 - 69.0) / 12.0)
    }

    #[test]
    fn pulse_is_band_limited_and_dc_free() {
        for w in [0.1f32, 0.5, 0.8] {
            let dt = 2990.0 / SR;
            let mut t = 0.0;
            let mut sum = 0.0;
            let n = 48000;
            for _ in 0..n {
                let s = pulse_sample(t, dt, w);
                assert!(s.abs() < 2.5);
                sum += s;
                t += dt;
                if t >= 1.0 {
                    t -= 1.0;
                }
            }
            assert!((sum / n as f32).abs() < 0.02, "DC {}", sum / n as f32);
        }
    }

    #[test]
    fn voice_stealing_prefers_released_then_oldest() {
        let mut s = Polysynth::new(SR);
        s.set_preset(1);
        let c = ctl(1);
        // ten held notes fill every voice
        for k in 60..70u8 {
            s.note_on(k, 100);
            run(&mut s, 64, c);
        }
        assert_eq!(s.active_voices(), POLYSYNTH_VOICES);
        // release one in the middle; a new note takes that voice, not a held one
        s.note_off(65);
        run(&mut s, 480, c);
        s.note_on(80, 100);
        let keys = s.voice_keys();
        assert!(!keys.contains(&65) && keys.contains(&80) && keys.contains(&60), "{keys:?}");
        // everything held: the oldest held note (60) is stolen
        s.note_on(81, 100);
        let keys = s.voice_keys();
        assert!(!keys.contains(&60) && keys.contains(&61) && keys.contains(&81), "{keys:?}");
        assert_eq!(s.active_voices(), POLYSYNTH_VOICES);
        // the same key again reuses its own voice
        s.note_on(81, 100);
        assert_eq!(s.voice_keys().iter().filter(|k| **k == 81).count(), 1);
        // and the steal doesn't click
        let out = run(&mut s, 4800, c);
        let st = crate::analysis::synth_stats(&out, 48000);
        assert_eq!(st.clicks, 0);
    }

    #[test]
    fn stealing_a_sounding_voice_is_click_free() {
        let mut s = Polysynth::new(SR);
        s.set_preset(1);
        let c = ctl(1);
        let mut out = Vec::new();
        for k in 0..30u8 {
            s.note_on(40 + k * 2, 110);
            out.extend(run(&mut s, 2400, c));
        }
        let st = crate::analysis::synth_stats(&out, 48000);
        assert_eq!(st.clicks, 0, "worst {} at {}", st.worst_click_ratio, st.worst_at);
    }

    #[test]
    fn chords_keep_every_note_audible() {
        for preset in 0..POLY_PRESETS.len() {
            let c = ctl(preset);
            let mut s = Polysynth::new(SR);
            s.set_preset(preset);
            for k in [36u8, 60, 64, 67] {
                s.note_on(k, 100);
            }
            run(&mut s, 24000, c);
            let out = run(&mut s, 96000, c);
            let lv: Vec<f32> = [36u8, 60, 64, 67].iter().map(|k| band_db(&out, SR, hz(*k), 2.0)).collect();
            let top = lv.iter().cloned().fold(f32::MIN, f32::max);
            for l in &lv {
                assert!(top - l < 12.0, "{}: {lv:?}", POLY_PRESETS[preset].name);
            }
        }
    }

    #[test]
    fn output_is_clean_and_stops() {
        for preset in 0..POLY_PRESETS.len() {
            let mut s = Polysynth::new(SR);
            s.set_preset(preset);
            let c = PolyControls { cutoff: 1.0, resonance: 1.0, vibrato: 1.0, bend: 12.0 };
            for k in [24u8, 36, 48, 60, 72, 84, 96, 108, 127, 0, 50, 51] {
                s.note_on(k, 127);
            }
            let out = run(&mut s, 24000, c);
            for x in &out {
                assert!(x.is_finite() && (*x == 0.0 || x.abs() >= f32::MIN_POSITIVE) && x.abs() < 4.0);
            }
            s.release_all();
            let out = run(&mut s, 48000 * 4, PolyControls { cutoff: 0.0, ..c });
            for x in &out {
                assert!(x.is_finite() && (*x == 0.0 || x.abs() >= f32::MIN_POSITIVE), "{x:e}");
            }
            assert_eq!(s.active_voices(), 0);
            let out = run(&mut s, 4800, c);
            assert!(out.iter().all(|x| *x == 0.0));
        }
    }

    #[test]
    fn key_tracking_and_drift() {
        for p in POLY_PRESETS.iter() {
            assert!((0.5..=1.0).contains(&p.key_track), "{}", p.name);
        }
        let mut s = Polysynth::new(SR);
        s.note_on(60, 100);
        s.note_on(72, 100);
        let d: Vec<f32> = s.voices.iter().filter(|v| v.active).map(|v| v.drift).collect();
        assert!(d.iter().all(|x| (1200.0 * x.log2()).abs() <= DRIFT_CENTS + 1e-3));
    }
}
