//! The electric piano: a physically inspired model of tine (Rhodes-style) and
//! reed (Wurlitzer-style) electric pianos, generated in real time.
//!
//! What it models, per note:
//! - **The tine or reed** is a decaying sinusoid at the key's pitch (the
//!   fundamental mode), with a two-stage, key-dependent decay: low notes ring
//!   for many seconds, high ones for about one.
//! - **The pickup** turns the tine's displacement into voltage non-linearly.
//!   It's a fixed degree-5 polynomial of the displacement (asymmetric, so it
//!   makes even harmonics as a tine pickup does when voiced off-centre). The
//!   harder the key is struck, the further the tine swings and the more
//!   harmonics the pickup adds: that's the "bark" of a hard-played tine
//!   piano. Because the polynomial is degree 5, nothing above the 5th
//!   harmonic is generated, and harmonics that would land above 0.45 × the
//!   sample rate are dropped per note, so it doesn't alias.
//! - **Attack transient:** the hammer's thud (a short burst of filtered noise)
//!   and the tine's (or reed's) inharmonic second bending mode, which decays
//!   in a fraction of a second. On *Bright Tines* two extra high partials add a
//!   1980s FM-piano-style sparkle.
//! - **Damper:** releasing a key (pedal up) damps the note over 60–250 ms and
//!   adds a soft low "thump" as the felt lands. The sustain pedal holds the
//!   dampers off.
//! - **Tremolo:** stereo autopan on the tine presets (the suitcase-amp kind),
//!   mono tremolo on the reed preset. The mod wheel sets the depth.
//!
//! Real-time safe: no allocation after construction, no locks. Voices that
//! are stolen fade out over 5 ms; denormals are flushed.

use crate::synth::{Rng, Svf, fast_sin, fast_tan, flush};
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug)]
pub struct EpPreset {
    pub name: &'static str,
    /// pickup polynomial: y = u + c[0]·u² + c[1]·u³ + c[2]·u⁴ + c[3]·u⁵ (u = displacement × drive)
    pub shaper: [f32; 4],
    /// pickup drive at the softest and the hardest velocity
    pub drive: (f32, f32),
    /// inharmonic attack partial: frequency ratio, level, decay (s)
    pub bell: (f32, f32, f32),
    /// FM-piano-style high partials (13 and 15 × f0): level (0 = off), decay (s)
    pub sparkle: (f32, f32),
    /// hammer noise level and its low-pass frequency (Hz) at full velocity
    pub hammer: (f32, f32),
    /// time to -60 dB while held, at the lowest (A0) and the highest key (C8), s
    pub decay: (f32, f32),
    /// share of the amplitude in the fast first stage of the decay, and how much faster it is
    pub fast_part: (f32, f32),
    /// damper time (to -60 dB) at the lowest and highest key, s
    pub release: (f32, f32),
    /// damper thump level
    pub thump: f32,
    pub tremolo_hz: f32,
    /// true = stereo autopan (antiphase L/R), false = mono tremolo
    pub autopan: bool,
    /// Tone knob default
    pub tone: f32,
    pub gain: f32,
}

pub const EP_PRESETS: [EpPreset; 3] = [
    EpPreset {
        name: "Suitcase",
        shaper: [0.38, -0.12, 0.10, -0.04],
        drive: (0.32, 1.15),
        bell: (7.11, 0.10, 0.09),
        sparkle: (0.0, 0.1),
        hammer: (0.05, 1800.0),
        decay: (24.0, 2.0),
        fast_part: (0.3, 6.0),
        release: (0.26, 0.07),
        thump: 0.05,
        tremolo_hz: 4.6,
        autopan: true,
        tone: 0.45,
        gain: 0.22,
    },
    EpPreset {
        name: "Bright Tines",
        shaper: [0.48, -0.06, 0.14, 0.0],
        drive: (0.45, 1.2),
        bell: (7.11, 0.30, 0.16),
        sparkle: (0.12, 0.18),
        hammer: (0.07, 3200.0),
        decay: (22.0, 1.8),
        fast_part: (0.35, 6.0),
        release: (0.24, 0.07),
        thump: 0.04,
        tremolo_hz: 5.2,
        autopan: true,
        tone: 0.7,
        gain: 0.19,
    },
    EpPreset {
        name: "Wurli",
        shaper: [0.55, 0.32, 0.16, 0.10],
        drive: (0.38, 1.25),
        bell: (6.27, 0.16, 0.06),
        sparkle: (0.0, 0.1),
        hammer: (0.06, 2400.0),
        decay: (14.0, 1.4),
        fast_part: (0.4, 5.0),
        release: (0.16, 0.05),
        thump: 0.035,
        tremolo_hz: 5.6,
        autopan: false,
        tone: 0.55,
        gain: 0.18,
    },
];

/// Voices that can sound at once. Above SOFT_VOICES, new notes fade the
/// least important voice out over 5 ms (released notes first, quietest first).
pub const EP_VOICES: usize = 40;
const SOFT_VOICES: usize = 32;

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    key: u8,
    /// key is down
    held: bool,
    /// key is up, pedal holds the dampers off
    sustained: bool,
    /// damper on: decaying fast
    damped: bool,
    /// stolen or re-struck: fading out quickly
    fading: bool,
    fade_mul: f32,
    fade: f32,
    vel: f32,
    phase: f32,
    inc: f32,
    /// two-stage decay: slow and fast components, per-sample multipliers
    a_slow: f32,
    a_fast: f32,
    m_slow: f32,
    m_fast: f32,
    /// damper multiplier per sample
    m_damp: f32,
    /// attack ramp 0..1
    att: f32,
    att_inc: f32,
    /// pickup drive, and the per-note shaper coefficients (high orders dropped near Nyquist)
    drive: f32,
    c: [f32; 4],
    bell_phase: f32,
    bell_inc: f32,
    bell: f32,
    bell_mul: f32,
    sp_phase: [f32; 2],
    sp_inc: [f32; 2],
    sp: f32,
    sp_mul: f32,
    /// hammer noise: envelope, rise, decay, one-pole low-pass state and coefficient
    ham: f32,
    ham_rise: f32,
    ham_peak: f32,
    ham_mul: f32,
    ham_lp: f32,
    ham_a: f32,
    /// damper thump: envelope, low-pass state
    thump: f32,
    thump_mul: f32,
    thump_lp: [f32; 2],
}

impl Voice {
    const EMPTY: Voice = Voice {
        active: false,
        key: 0,
        held: false,
        sustained: false,
        damped: false,
        fading: false,
        fade_mul: 1.0,
        fade: 1.0,
        vel: 0.0,
        phase: 0.0,
        inc: 0.0,
        a_slow: 0.0,
        a_fast: 0.0,
        m_slow: 1.0,
        m_fast: 1.0,
        m_damp: 1.0,
        att: 0.0,
        att_inc: 1.0,
        drive: 1.0,
        c: [0.0; 4],
        bell_phase: 0.0,
        bell_inc: 0.0,
        bell: 0.0,
        bell_mul: 0.0,
        sp_phase: [0.0; 2],
        sp_inc: [0.0; 2],
        sp: 0.0,
        sp_mul: 0.0,
        ham: 0.0,
        ham_rise: 0.0,
        ham_peak: 0.0,
        ham_mul: 0.0,
        ham_lp: 0.0,
        ham_a: 0.0,
        thump: 0.0,
        thump_mul: 0.0,
        thump_lp: [0.0; 2],
    };

    fn level(&self) -> f32 {
        (self.a_slow + self.a_fast) * self.fade
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EpControls {
    /// Tone knob 0..1 (brightness)
    pub tone: f32,
    /// mod wheel 0..1: tremolo / autopan depth
    pub tremolo: f32,
    /// pitch bend in semitones (smoothed per sample here)
    pub bend: f32,
}

pub struct ElectricPiano {
    sr: f32,
    preset: usize,
    voices: [Voice; EP_VOICES],
    pedal: bool,
    clock: u64,
    rng: Rng,
    lp: [Svf; 2],
    dc: [f32; 2],
    trem_phase: f32,
    tone_s: f32,
    trem_s: f32,
    bend_s: f32,
    quiet_frames: usize,
}

/// Interpolates `(lo, hi)` exponentially over the keyboard (A0 .. C8).
fn by_key(key: u8, lohi: (f32, f32)) -> f32 {
    let f = ((key as f32 - 21.0) / 87.0).clamp(0.0, 1.0);
    lohi.0 * (lohi.1 / lohi.0).powf(f)
}

/// Per-sample multiplier that takes a level down by 60 dB in `secs`.
fn mul_60db(secs: f32, sr: f32) -> f32 {
    (-6.9 / (secs.max(0.001) * sr)).exp()
}

impl ElectricPiano {
    pub fn new(sr: f32) -> Self {
        ElectricPiano {
            sr,
            preset: 0,
            voices: [Voice::EMPTY; EP_VOICES],
            pedal: false,
            clock: 0,
            rng: Rng(0x2468_ace1),
            lp: [Svf::default(); 2],
            dc: [0.0; 2],
            trem_phase: 0.0,
            tone_s: -1.0,
            trem_s: 0.0,
            bend_s: 0.0,
            quiet_frames: usize::MAX / 2,
        }
    }

    pub fn preset(&self) -> usize {
        self.preset
    }

    /// Switches preset; sounding notes are released (they ring out on the old voicing).
    pub fn set_preset(&mut self, idx: usize) {
        let idx = idx.min(EP_PRESETS.len() - 1);
        if idx != self.preset {
            self.release_all();
        }
        self.preset = idx;
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    pub fn sounding(&self, out: &mut [u64; 2]) {
        for v in self.voices.iter().filter(|v| v.active && !v.fading && (v.held || v.sustained)) {
            out[(v.key / 64) as usize] |= 1 << (v.key % 64);
        }
    }

    pub fn busy(&self) -> bool {
        self.quiet_frames < (self.sr * 0.05) as usize
    }

    fn importance(v: &Voice) -> f32 {
        let base = if v.held || v.sustained { 2.0 } else { 1.0 };
        base + v.level().min(0.999)
    }

    fn start_fade(v: &mut Voice, secs: f32, sr: f32) {
        if !v.fading {
            v.fading = true;
            v.fade_mul = (-1.0 / (secs * sr)).exp();
        }
    }

    fn alloc(&mut self) -> usize {
        let sr = self.sr;
        let live = self.voices.iter().filter(|v| v.active && !v.fading).count();
        if live >= SOFT_VOICES {
            if let Some(i) = self
                .voices
                .iter()
                .enumerate()
                .filter(|(_, v)| v.active && !v.fading)
                .min_by(|a, b| Self::importance(a.1).total_cmp(&Self::importance(b.1)))
                .map(|(i, _)| i)
            {
                Self::start_fade(&mut self.voices[i], 0.005, sr);
            }
        }
        if let Some(i) = self.voices.iter().position(|v| !v.active) {
            return i;
        }
        // all slots busy (very fast playing): reuse the quietest fading voice
        self.voices
            .iter()
            .enumerate()
            .min_by(|a, b| {
                let ka = if a.1.fading { a.1.level() - 10.0 } else { Self::importance(a.1) };
                let kb = if b.1.fading { b.1.level() - 10.0 } else { Self::importance(b.1) };
                ka.total_cmp(&kb)
            })
            .map(|(i, _)| i)
            .unwrap()
    }

    /// `vel` is the shaped velocity, 1..127.
    pub fn note_on(&mut self, key: u8, vel: u8) {
        let sr = self.sr;
        let p = EP_PRESETS[self.preset];
        // re-striking a key: the previous strike fades over 30 ms under the new one
        for v in self.voices.iter_mut().filter(|v| v.active && v.key == key) {
            Self::start_fade(v, 0.03, sr);
        }
        self.clock += 1;
        let slot = self.alloc();
        let vn = (vel as f32 / 127.0).clamp(0.0, 1.0);
        let f0 = 440.0 * 2f32.powf((key as f32 - 69.0) / 12.0);
        let nyq = 0.45 * sr;
        // amplitude: tine swing grows with velocity (about 20 dB soft to hard);
        // the bottom two octaves are voiced a little softer (to -2.5 dB at C2)
        // so low notes don't swamp a chord
        let voicing = 0.75 + 0.25 * ((key as f32 - 36.0) / 24.0).clamp(0.0, 1.0);
        let amp = (0.1 + 0.9 * vn.powf(1.4)) * voicing;
        // pickup drive also grows with velocity: the bark; less on the top keys
        let top = ((key as f32 - 84.0) / 24.0).clamp(0.0, 1.0);
        let drive = (p.drive.0 + (p.drive.1 - p.drive.0) * vn * vn) * (1.0 - 0.4 * top);
        let mut c = p.shaper;
        // harmonic k (2..5) of the shaper sits at k·f0: drop the orders that would alias
        for (i, ci) in c.iter_mut().enumerate() {
            if (i as f32 + 2.0) * f0 > nyq {
                *ci = 0.0;
            }
        }
        let t60 = by_key(key, p.decay);
        let r = &mut self.rng;
        let bell_ok = p.bell.0 * f0 < nyq;
        let sp_inc = [13.0 * f0 / sr, 15.0 * f0 / sr];
        let sp_ok = p.sparkle.0 > 0.0 && 15.0 * f0 < nyq;
        let ham_hz = (p.hammer.1 * (0.35 + 0.65 * vn)).min(nyq);
        let v = &mut self.voices[slot];
        *v = Voice {
            active: true,
            key,
            held: true,
            vel: vn,
            // the tine starts from rest: phase 0 (sin = 0), no step
            phase: 0.0,
            inc: f0 / sr,
            a_slow: amp * (1.0 - p.fast_part.0),
            a_fast: amp * p.fast_part.0,
            m_slow: mul_60db(t60, sr),
            m_fast: mul_60db(t60 / p.fast_part.1, sr),
            m_damp: mul_60db(by_key(key, p.release), sr),
            att: 0.0,
            att_inc: 1.0 / (0.0012 * sr),
            drive,
            c,
            bell_phase: r.next_f32() * 0.5 + 0.5,
            bell_inc: p.bell.0 * f0 / sr,
            bell: if bell_ok { p.bell.1 * amp * (0.3 + 0.7 * vn) } else { 0.0 },
            bell_mul: mul_60db(p.bell.2 * (1.0 - 0.5 * top), sr),
            sp_phase: [0.0, 0.25],
            sp_inc,
            sp: if sp_ok { p.sparkle.0 * amp * vn } else { 0.0 },
            sp_mul: mul_60db(p.sparkle.1, sr),
            ham: 0.0,
            ham_rise: 1.0 / (0.0004 * sr),
            ham_peak: p.hammer.0 * vn * vn,
            ham_mul: mul_60db(0.012, sr),
            ham_lp: 0.0,
            ham_a: 1.0 - (-2.0 * PI * ham_hz / sr).exp(),
            ..Voice::EMPTY
        };
    }

    fn damp(v: &mut Voice, thump: f32, sr: f32) {
        if v.damped || v.fading {
            return;
        }
        v.damped = true;
        // the felt landing on the tine: a soft low thump, louder for louder notes
        v.thump = thump * (0.3 + 0.7 * v.vel) * (v.level() / (v.level() + 0.05)).min(1.0);
        v.thump_mul = mul_60db(0.05, sr);
    }

    pub fn note_off(&mut self, key: u8) {
        let sr = self.sr;
        let thump = EP_PRESETS[self.preset].thump;
        let pedal = self.pedal;
        for v in self.voices.iter_mut().filter(|v| v.active && v.key == key && v.held) {
            v.held = false;
            if pedal {
                v.sustained = true;
            } else {
                Self::damp(v, thump, sr);
            }
        }
    }

    pub fn sustain(&mut self, down: bool) {
        self.pedal = down;
        if !down {
            let sr = self.sr;
            let thump = EP_PRESETS[self.preset].thump;
            for v in self.voices.iter_mut().filter(|v| v.active && v.sustained) {
                v.sustained = false;
                Self::damp(v, thump, sr);
            }
        }
    }

    pub fn release_all(&mut self) {
        let sr = self.sr;
        let thump = EP_PRESETS[self.preset].thump;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.held = false;
            v.sustained = false;
            Self::damp(v, thump, sr);
        }
    }

    pub fn panic(&mut self) {
        let sr = self.sr;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.held = false;
            v.sustained = false;
            Self::start_fade(v, 0.01, sr);
        }
    }

    /// Adds `n` stereo frames into `out` (interleaved).
    pub fn render(&mut self, out: &mut [f32], n: usize, c: EpControls) {
        let sr = self.sr;
        let p = EP_PRESETS[self.preset];
        let any_active = self.voices.iter().any(|v| v.active);
        if !any_active && !self.busy() {
            self.trem_phase = (self.trem_phase + p.tremolo_hz * n as f32 / sr).fract();
            self.trem_s = c.tremolo;
            self.bend_s = c.bend;
            self.tone_s = c.tone;
            return;
        }
        let inv_sr = 1.0 / sr;
        if self.tone_s < 0.0 {
            self.tone_s = c.tone;
        }
        // controls smoothed per block (~5 ms), interpolated within it
        let a = 1.0 - (-(n as f32) / (0.005 * sr)).exp();
        self.tone_s += (c.tone - self.tone_s) * a;
        let trem0 = self.trem_s;
        self.trem_s += (c.tremolo - self.trem_s) * a;
        // Tone: the pickup drive (more or less bark) and a low-pass, 1.2 kHz .. 20 kHz
        let drive_k = 0.7 + 0.6 * self.tone_s;
        let lp_hz = (1200.0 * 2f32.powf(self.tone_s * 4.1)).min(0.45 * sr);
        let lp_g = fast_tan(PI * lp_hz * inv_sr);
        let bend_a = 1.0 - (-1.0 / (0.003 * sr)).exp();
        let mut bend = self.bend_s;
        let thump_a = 1.0 - (-2.0 * PI * 140.0 * inv_sr).exp();
        // the voices, mono
        let mut mono = [0f32; 128];
        let mut bend_mul = [1f32; 128];
        for i in 0..n {
            bend += (c.bend - bend) * bend_a;
            bend_mul[i] = if bend != 0.0 { crate::synth::exp2(bend / 12.0) } else { 1.0 };
        }
        self.bend_s = if (bend - c.bend).abs() < 1e-6 { c.bend } else { bend };
        let mut any = false;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let drive = v.drive * drive_k;
            let inv_drive = 1.0 / drive;
            let [c2, c3, c4, c5] = v.c;
            for i in 0..n {
                let bm = bend_mul[i];
                if v.att < 1.0 {
                    v.att = (v.att + v.att_inc).min(1.0);
                }
                let amp = (v.a_slow + v.a_fast) * v.att;
                // tine displacement through the pickup
                let s = fast_sin(v.phase);
                let big_u = drive * amp;
                let u = big_u * s;
                let u2 = u * u;
                let uu2 = big_u * big_u;
                // the even terms' mean over a cycle (U²/2, 3U⁴/8) is removed so the
                // pickup adds no envelope-following DC
                let y = u + c2 * (u2 - 0.5 * uu2) + c3 * u2 * u + c4 * (u2 * u2 - 0.375 * uu2 * uu2) + c5 * u2 * u2 * u;
                let mut x = y * inv_drive;
                v.phase += v.inc * bm;
                if v.phase >= 1.0 {
                    v.phase -= 1.0;
                }
                // attack partials
                if v.bell > 1e-6 {
                    x += v.bell * fast_sin(v.bell_phase) * v.att;
                    v.bell *= v.bell_mul;
                    v.bell_phase += v.bell_inc * bm;
                    if v.bell_phase >= 1.0 {
                        v.bell_phase -= 1.0;
                    }
                }
                if v.sp > 1e-6 {
                    x += v.sp * (fast_sin(v.sp_phase[0]) + 0.7 * fast_sin(v.sp_phase[1])) * v.att;
                    v.sp *= v.sp_mul;
                    for k in 0..2 {
                        v.sp_phase[k] += v.sp_inc[k] * bm;
                        if v.sp_phase[k] >= 1.0 {
                            v.sp_phase[k] -= 1.0;
                        }
                    }
                }
                // hammer noise: 0.4 ms rise, ~12 ms decay, low-passed
                if v.ham_peak > 0.0 {
                    if v.ham_rise > 0.0 && v.ham < v.ham_peak {
                        v.ham = (v.ham + v.ham_peak * v.ham_rise).min(v.ham_peak);
                        if v.ham >= v.ham_peak {
                            v.ham_rise = 0.0;
                        }
                    } else {
                        v.ham_rise = 0.0;
                        v.ham *= v.ham_mul;
                        if v.ham < 1e-6 {
                            v.ham = 0.0;
                            v.ham_peak = 0.0;
                        }
                    }
                    let nz = self.rng.next_f32();
                    v.ham_lp += (nz - v.ham_lp) * v.ham_a;
                    x += v.ham * v.ham_lp;
                }
                // damper thump: low-passed noise burst
                if v.thump > 1e-6 {
                    let nz = self.rng.next_f32();
                    v.thump_lp[0] += (nz - v.thump_lp[0]) * thump_a;
                    v.thump_lp[1] += (v.thump_lp[0] - v.thump_lp[1]) * thump_a;
                    x += v.thump * v.thump_lp[1] * 4.0;
                    v.thump *= v.thump_mul;
                }
                // decay: natural while held/sustained, damper once released
                if v.damped {
                    v.a_slow *= v.m_damp;
                    v.a_fast *= v.m_damp;
                } else {
                    v.a_slow *= v.m_slow;
                    v.a_fast *= v.m_fast;
                }
                if v.fading {
                    v.fade *= v.fade_mul;
                }
                mono[i] += x * v.fade;
            }
            v.ham_lp = flush(v.ham_lp);
            v.thump_lp = [flush(v.thump_lp[0]), flush(v.thump_lp[1])];
            let lvl = v.level();
            if (v.fading && v.fade < 1e-3) || (lvl < 2e-5 && v.thump < 1e-6 && v.ham_peak == 0.0 && v.att >= 1.0) {
                v.active = false;
            } else {
                any = true;
            }
        }
        // bus: tone low-pass, DC blocker, tremolo / autopan
        let dc_a = 1.0 - (-2.0 * PI * 12.0 * inv_sr).exp();
        let inv_n = 1.0 / n as f32;
        for i in 0..n {
            let x = mono[i];
            let (y, _) = self.lp[0].tick(x, lp_g, 1.2);
            self.dc[0] += (y - self.dc[0]) * dc_a;
            let y = (y - self.dc[0]) * p.gain;
            let depth = trem0 + (self.trem_s - trem0) * (i as f32 * inv_n);
            let s = fast_sin(self.trem_phase);
            self.trem_phase += p.tremolo_hz * inv_sr;
            if self.trem_phase >= 1.0 {
                self.trem_phase -= 1.0;
            }
            let (gl, gr) = if p.autopan {
                (1.0 - depth * 0.5 * (1.0 + s), 1.0 - depth * 0.5 * (1.0 - s))
            } else {
                let g = 1.0 - depth * 0.3 * (1.0 + s);
                (g, g)
            };
            out[i * 2] += flush(y * gl);
            out[i * 2 + 1] += flush(y * gr);
            if y.abs() > 1e-6 {
                any = true;
            }
        }
        self.lp[0].flush_denormals();
        self.dc[0] = flush(self.dc[0]);
        if any {
            self.quiet_frames = 0;
        } else {
            self.quiet_frames += n;
            if !self.busy() {
                self.lp = [Svf::default(); 2];
                self.dc = [0.0; 2];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::band_db;

    const SR: f32 = 48000.0;

    fn ctl() -> EpControls {
        EpControls { tone: 0.5, tremolo: 0.0, bend: 0.0 }
    }

    fn run(s: &mut ElectricPiano, frames: usize, c: EpControls) -> Vec<f32> {
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

    /// Energy in harmonics 2..5 relative to the fundamental, in dB, 50..300 ms after the strike.
    fn brightness(preset: usize, key: u8, vel: u8) -> f32 {
        let mut s = ElectricPiano::new(SR);
        s.set_preset(preset);
        s.note_on(key, vel);
        let out = run(&mut s, 14400, ctl());
        let seg = &out[2400 * 2..];
        let f = hz(key);
        let fund = band_db(seg, SR, f, 4.0);
        let hp: f64 = (2..=5).map(|k| 10f64.powf(band_db(seg, SR, k as f32 * f, 4.0) as f64 / 10.0)).sum();
        10.0 * hp.log10() as f32 - fund
    }

    #[test]
    fn velocity_brightens_monotonically() {
        for preset in 0..EP_PRESETS.len() {
            for key in [48u8, 60, 72] {
                let b: Vec<f32> = [20u8, 50, 80, 110, 127].iter().map(|v| brightness(preset, key, *v)).collect();
                for w in b.windows(2) {
                    assert!(w[1] > w[0] + 0.5, "{} key {key}: brightness not rising with velocity {b:?}", EP_PRESETS[preset].name);
                }
                assert!(b[4] - b[0] > 10.0, "{} key {key}: soft-to-hard timbre change only {:.1} dB {b:?}", EP_PRESETS[preset].name, b[4] - b[0]);
            }
        }
    }

    #[test]
    fn decay_is_key_dependent_and_dampers_work() {
        let level_after = |key: u8, secs: f32, release: bool| {
            let mut s = ElectricPiano::new(SR);
            s.note_on(key, 100);
            if release {
                run(&mut s, 4800, ctl());
                s.note_off(key);
            }
            let out = run(&mut s, (secs * SR) as usize, ctl());
            let tail = &out[out.len() - 4800..];
            (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt()
        };
        // a low note rings longer than a high one
        let low = level_after(40, 2.0, false) / level_after(40, 0.1, false);
        let high = level_after(88, 2.0, false) / level_after(88, 0.1, false);
        assert!(low > high * 4.0, "low decay ratio {low}, high {high}");
        // releasing the key damps it
        assert!(level_after(48, 1.0, true) < level_after(48, 1.1, false) * 0.01);
    }

    #[test]
    fn pedal_holds_and_everything_ends_clean() {
        for preset in 0..EP_PRESETS.len() {
            let mut s = ElectricPiano::new(SR);
            s.set_preset(preset);
            s.sustain(true);
            for k in [36u8, 48, 60, 64, 67, 72, 76, 84, 96, 108] {
                s.note_on(k, 127);
                s.note_off(k);
            }
            let c = EpControls { tone: 1.0, tremolo: 1.0, bend: 2.0 };
            let out = run(&mut s, 24000, c);
            assert_eq!(s.active_voices(), 10, "pedal holds");
            for x in &out {
                assert!(x.is_finite() && (*x == 0.0 || x.abs() >= f32::MIN_POSITIVE) && x.abs() < 2.0);
            }
            s.sustain(false);
            let out = run(&mut s, 48000 * 2, c);
            for x in &out {
                assert!(x.is_finite() && (*x == 0.0 || x.abs() >= f32::MIN_POSITIVE), "denormal/NaN {x:e}");
            }
            assert_eq!(s.active_voices(), 0);
            let out = run(&mut s, 9600, c);
            assert!(out.iter().all(|x| *x == 0.0));
        }
    }

    #[test]
    fn voice_limit_and_restrike() {
        let mut s = ElectricPiano::new(SR);
        s.sustain(true);
        for i in 0..200u32 {
            s.note_on(30 + (i % 70) as u8, 100);
            run(&mut s, 64, ctl());
            assert!(s.active_voices() <= EP_VOICES);
        }
        // re-striking a key keeps one live voice for it
        let mut s = ElectricPiano::new(SR);
        s.note_on(60, 100);
        run(&mut s, 480, ctl());
        s.note_on(60, 100);
        run(&mut s, 14400, ctl());
        assert_eq!(s.voices.iter().filter(|v| v.active && v.key == 60).count(), 1);
    }
}
