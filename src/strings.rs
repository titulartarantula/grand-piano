//! The string machine: a polyphonic, divide-down string ensemble in the style
//! of late-1970s string synthesizers.
//!
//! How those machines work, and what this models:
//! - **Divide-down oscillators.** Twelve top-octave oscillators, one per pitch
//!   class, run all the time; every lower C, C#, ... is that oscillator divided
//!   by 2, 4, 8, ... So all octaves of a note are phase-locked, every key is
//!   always "running" (a key press opens a gate mid-cycle), and the whole
//!   keyboard is polyphonic. Vibrato bends the master clocks, so every note
//!   moves together. Here the twelve clocks are counters in top-octave cycles
//!   and each key reads its phase from its clock divided by a power of two.
//! - **Footages.** Each key mixes two registers (16', 8' or 4'), band-limited
//!   sawtooths (PolyBLEP).
//! - **Per-key attack/release** envelopes, a shared low-pass (the Tone/cutoff
//!   knob) and a high-pass that thins the low end like the originals.
//! - **Ensemble chorus.** Three modulated delay lines driven by a slow
//!   (0.6 Hz) and a fast (6 Hz) LFO at 120° offsets, spread to stereo. This is
//!   what turns buzzy sawtooths into a "string section".
//! - **Phaser** (some presets): six all-pass stages with feedback, swept slowly.
//!
//! Real-time safe: no allocation after construction, no locks.

use crate::synth::{Svf, VIBRATO_HZ, cutoff_hz, exp2, fast_sin, fast_tan, osc_sample, vibrato_semis, Wave};
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug)]
pub struct StringPreset {
    pub name: &'static str,
    /// (octave offset relative to 8', level): -1 = 16', 0 = 8', +1 = 4'
    pub footage: [(i8, f32); 2],
    pub attack: f32,
    pub release: f32,
    /// cutoff knob default
    pub cutoff: f32,
    pub highpass_hz: f32,
    /// 0..1
    pub ensemble: f32,
    /// phaser mix 0..1 (0 = off)
    pub phaser: f32,
    pub phaser_rate: f32,
    pub gain: f32,
}

pub const STRING_PRESETS: [StringPreset; 3] = [
    StringPreset {
        name: "Strings",
        footage: [(0, 1.0), (1, 0.5)],
        attack: 0.07,
        release: 0.6,
        cutoff: 0.8,
        highpass_hz: 140.0,
        ensemble: 1.0,
        phaser: 0.0,
        phaser_rate: 0.3,
        gain: 0.12,
    },
    StringPreset {
        name: "Phased",
        footage: [(0, 1.0), (1, 0.45)],
        attack: 0.045,
        release: 0.7,
        cutoff: 0.76,
        highpass_hz: 170.0,
        ensemble: 1.0,
        phaser: 0.85,
        phaser_rate: 0.32,
        gain: 0.2,
    },
    StringPreset {
        name: "Dark",
        footage: [(-1, 0.7), (0, 1.0)],
        attack: 0.16,
        release: 1.0,
        cutoff: 0.6,
        highpass_hz: 70.0,
        ensemble: 0.85,
        phaser: 0.0,
        phaser_rate: 0.3,
        gain: 0.11,
    },
];

/// Top-octave clock counters wrap at 2^10 cycles, so every divider (1..2^10)
/// sees a continuous phase.
const WRAP: f64 = 1024.0;
const TOP_OCTAVE: i32 = 10; // MIDI keys 120..131

#[derive(Clone, Copy, Default)]
struct KeyVoice {
    active: bool,
    gate: bool,
    sustained: bool,
    env: f32,
    vel_gain: f32,
}

struct Ensemble {
    buf: Vec<f32>,
    w: usize,
    slow: f32,
    fast: f32,
}

impl Ensemble {
    fn new(sr: f32) -> Self {
        Ensemble { buf: vec![0.0; (sr * 0.03) as usize + 4], w: 0, slow: 0.0, fast: 0.37 }
    }
    #[inline(always)]
    fn read(&self, delay: f32) -> f32 {
        let n = self.buf.len();
        let pos = self.w as f32 + n as f32 - delay;
        let i = pos as usize;
        let frac = pos - i as f32;
        let a = self.buf[i % n];
        let b = self.buf[(i + 1) % n];
        a + (b - a) * frac
    }
    /// mono in, stereo out
    #[inline(always)]
    fn tick(&mut self, x: f32, sr: f32, depth: f32) -> (f32, f32) {
        let n = self.buf.len();
        self.buf[self.w] = x;
        self.w = if self.w + 1 == n { 0 } else { self.w + 1 };
        self.slow += 0.62 / sr;
        if self.slow >= 1.0 {
            self.slow -= 1.0;
        }
        self.fast += 6.1 / sr;
        if self.fast >= 1.0 {
            self.fast -= 1.0;
        }
        let base = 0.0065 * sr;
        let ds = 0.0022 * sr * depth;
        let df = 0.00028 * sr * depth;
        let mut taps = [0f32; 3];
        for (i, t) in taps.iter_mut().enumerate() {
            let off = i as f32 / 3.0;
            let mut ps = self.slow + off;
            if ps >= 1.0 {
                ps -= 1.0;
            }
            let mut pf = self.fast + off;
            if pf >= 1.0 {
                pf -= 1.0;
            }
            *t = self.read(base + ds * fast_sin(ps) + df * fast_sin(pf));
        }
        let l = (taps[0] + 0.6 * taps[1]) * 0.62;
        let r = (taps[2] + 0.6 * taps[1]) * 0.62;
        let dry = 1.0 - 0.75 * depth;
        (dry * x + l, dry * x + r)
    }
}

#[derive(Default, Clone, Copy)]
struct Phaser {
    z: [f32; 6],
    fb: f32,
}

impl Phaser {
    #[inline(always)]
    fn tick(&mut self, x: f32, a: f32) -> f32 {
        let mut y = x + self.fb * 0.45;
        for z in self.z.iter_mut() {
            // first-order all-pass: y = a*x + z; z = x - a*y
            let o = a * y + *z;
            *z = y - a * o;
            y = o;
        }
        self.fb = y;
        y
    }
    fn flush(&mut self) {
        for z in self.z.iter_mut().chain(std::iter::once(&mut self.fb)) {
            if z.abs() < 1e-15 {
                *z = 0.0;
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StringControls {
    /// cutoff knob 0..1
    pub cutoff: f32,
    /// mod wheel 0..1 (vibrato)
    pub vibrato: f32,
    /// pitch bend target in semitones (smoothed per sample)
    pub bend: f32,
}

pub struct StringMachine {
    sr: f32,
    preset: usize,
    keys: [KeyVoice; 128],
    /// per pitch class: top-octave cycles (wraps at WRAP)
    clock: [f64; 12],
    /// per pitch class: top-octave frequency in Hz
    f_top: [f64; 12],
    pedal: bool,
    att: f32,
    rel: f32,
    lp: Svf,
    hp: Svf,
    ens: Ensemble,
    ph: [Phaser; 2],
    ph_lfo: f32,
    vib_phase: f32,
    norm: f32,
    cut_s: f32,
    bend_s: f32,
    /// silence counter: effects tails finished
    quiet_frames: usize,
}

impl StringMachine {
    pub fn new(sr: f32) -> Self {
        let mut f_top = [0f64; 12];
        for (pc, f) in f_top.iter_mut().enumerate() {
            *f = 440.0 * 2f64.powf((TOP_OCTAVE * 12 + pc as i32 - 69) as f64 / 12.0);
        }
        // start the clocks at unrelated phases, like free-running oscillators
        let mut clock = [0f64; 12];
        for (i, c) in clock.iter_mut().enumerate() {
            *c = (i as f64 * 0.618_034 * 97.0) % WRAP;
        }
        let mut s = StringMachine {
            sr,
            preset: 0,
            keys: [KeyVoice::default(); 128],
            clock,
            f_top,
            pedal: false,
            att: 0.0,
            rel: 0.0,
            lp: Svf::default(),
            hp: Svf::default(),
            ens: Ensemble::new(sr),
            ph: [Phaser::default(); 2],
            ph_lfo: 0.0,
            vib_phase: 0.0,
            norm: 1.0,
            cut_s: -1.0,
            bend_s: 0.0,
            quiet_frames: usize::MAX / 2,
        };
        s.set_preset(0);
        s
    }

    pub fn preset(&self) -> usize {
        self.preset
    }

    pub fn set_preset(&mut self, idx: usize) {
        let idx = idx.min(STRING_PRESETS.len() - 1);
        self.preset = idx;
        let p = &STRING_PRESETS[idx];
        // attack: linear-ish RC to 1.0; release: exponential to -60 dB
        self.att = 1.0 - (-(5f32.ln()) / (p.attack.max(0.002) * self.sr)).exp();
        self.rel = 1.0 - (-6.9 / (p.release.max(0.01) * self.sr)).exp();
    }

    pub fn active_voices(&self) -> usize {
        self.keys.iter().filter(|k| k.active).count()
    }

    pub fn sounding(&self, out: &mut [u64; 2]) {
        for (k, v) in self.keys.iter().enumerate() {
            if v.active && (v.gate || v.sustained) {
                out[k / 64] |= 1 << (k % 64);
            }
        }
    }

    /// `vel` is the shaped velocity. The originals weren't touch-sensitive;
    /// here velocity changes the level gently (about 8 dB from soft to hard).
    pub fn note_on(&mut self, key: u8, vel: u8) {
        let v = &mut self.keys[key as usize & 127];
        let vn = vel as f32 / 127.0;
        v.vel_gain = 0.4 + 0.6 * vn;
        v.active = true;
        v.gate = true;
        v.sustained = false;
        self.quiet_frames = 0;
    }

    pub fn note_off(&mut self, key: u8) {
        let pedal = self.pedal;
        let v = &mut self.keys[key as usize & 127];
        if v.gate {
            v.gate = false;
            v.sustained = pedal;
        }
    }

    pub fn sustain(&mut self, down: bool) {
        self.pedal = down;
        if !down {
            for v in self.keys.iter_mut() {
                v.sustained = false;
            }
        }
    }

    pub fn release_all(&mut self) {
        for v in self.keys.iter_mut() {
            v.gate = false;
            v.sustained = false;
        }
    }

    pub fn panic(&mut self) {
        self.release_all();
        self.rel = 1.0 - (-6.9 / (0.01 * self.sr)).exp();
    }

    /// True while notes or effect tails are still sounding.
    pub fn busy(&self) -> bool {
        self.quiet_frames < (self.sr * 0.1) as usize
    }

    /// Adds `n` stereo frames into `out` (interleaved).
    pub fn render(&mut self, out: &mut [f32], n: usize, c: StringControls) {
        let sr = self.sr;
        // keep the master clocks running even when silent (they're free-running oscillators)
        let vib_depth = vibrato_semis(c.vibrato);
        if !self.busy() && !self.keys.iter().any(|k| k.active) {
            for (pc, cl) in self.clock.iter_mut().enumerate() {
                *cl = (*cl + self.f_top[pc] * n as f64 / sr as f64) % WRAP;
            }
            self.vib_phase = (self.vib_phase + VIBRATO_HZ * n as f32 / sr).fract();
            self.ph_lfo = (self.ph_lfo + STRING_PRESETS[self.preset].phaser_rate * n as f32 / sr).fract();
            self.bend_s = c.bend;
            return;
        }
        let p = STRING_PRESETS[self.preset];
        if self.cut_s < 0.0 {
            self.cut_s = c.cutoff;
        }
        let a = 1.0 - (-(n as f32) / (0.005 * sr)).exp();
        let cut0 = self.cut_s;
        self.cut_s += (c.cutoff - self.cut_s) * a;
        let inv_sr = 1.0 / sr;
        let hp_g = fast_tan(PI * p.highpass_hz * inv_sr);
        // active keys for this block (keys only start in note_on, between blocks)
        let mut list = [0u8; 128];
        let mut nl = 0;
        for (k, v) in self.keys.iter().enumerate() {
            if v.active {
                list[nl] = k as u8;
                nl += 1;
            }
        }
        let n_active = nl as f32;
        // gentle level normalisation for big chords (smoothed below)
        let norm_t = (3.0 / n_active.max(3.0)).powf(0.35);
        let norm_a = 1.0 - (-1.0 / (0.08 * sr)).exp();
        let mut any = false;
        let bend_a = 1.0 - (-1.0 / (0.003 * sr)).exp();
        for i in 0..n {
            // pitch bend, smoothed per sample, bends the master clocks with the vibrato
            self.bend_s += (c.bend - self.bend_s) * bend_a;
            // vibrato bends all the master clocks together
            let vib = if vib_depth > 0.0 { vib_depth * fast_sin(self.vib_phase) } else { 0.0 };
            self.vib_phase += VIBRATO_HZ * inv_sr;
            if self.vib_phase >= 1.0 {
                self.vib_phase -= 1.0;
            }
            let shift = vib + self.bend_s;
            let vmul = if shift != 0.0 { exp2(shift / 12.0) as f64 } else { 1.0 };
            let mut inc = [0f64; 12];
            for pc in 0..12 {
                inc[pc] = self.f_top[pc] * vmul / sr as f64;
                let mut c2 = self.clock[pc] + inc[pc];
                if c2 >= WRAP {
                    c2 -= WRAP;
                }
                self.clock[pc] = c2;
            }
            let mut mix = 0f32;
            for &k8 in list[..nl].iter() {
                let k = k8 as usize;
                let v = &mut self.keys[k];
                if !v.active {
                    continue;
                }
                if v.gate || v.sustained {
                    v.env += (1.25 - v.env) * self.att;
                    if v.env > 1.0 {
                        v.env = 1.0;
                    }
                } else {
                    v.env -= v.env * self.rel;
                    if v.env < 1e-4 {
                        v.env = 0.0;
                        v.active = false;
                        continue;
                    }
                }
                let mut s = 0f32;
                for &(oct, lvl) in p.footage.iter() {
                    let kk = k as i32 + 12 * oct as i32;
                    if !(0..=TOP_OCTAVE * 12 + 11).contains(&kk) {
                        continue;
                    }
                    let pc = (kk % 12) as usize;
                    let div = (1u32 << (TOP_OCTAVE - kk / 12)) as f64;
                    let ph = (self.clock[pc] / div).fract() as f32;
                    let dt = (inc[pc] / div) as f32;
                    if dt >= 0.45 {
                        continue; // above the audio band at this sample rate
                    }
                    s += lvl * osc_sample(Wave::Saw, ph, dt);
                }
                mix += s * v.env * v.vel_gain;
            }
            self.norm += (norm_t - self.norm) * norm_a;
            let cut = cut0 + (self.cut_s - cut0) * (i as f32 / n as f32);
            let g = fast_tan(PI * cutoff_hz(cut).min(0.45 * sr) * inv_sr);
            let (lp, _) = self.lp.tick(mix * self.norm, g, 1.3);
            let (_, hp) = self.hp.tick(lp, hp_g, 1.414);
            let (mut l, mut r) = self.ens.tick(hp, sr, p.ensemble);
            if p.phaser > 0.0 {
                self.ph_lfo += p.phaser_rate * inv_sr;
                if self.ph_lfo >= 1.0 {
                    self.ph_lfo -= 1.0;
                }
                let sweep = 0.5 + 0.5 * fast_sin(self.ph_lfo);
                let mut ph2 = self.ph_lfo + 0.25;
                if ph2 >= 1.0 {
                    ph2 -= 1.0;
                }
                let sweep2 = 0.5 + 0.5 * fast_sin(ph2);
                let coef = |s: f32| {
                    let fc = 220.0 * exp2(3.4 * s);
                    let t = fast_tan(PI * fc * inv_sr);
                    (t - 1.0) / (t + 1.0)
                };
                let wl = self.ph[0].tick(l, coef(sweep));
                let wr = self.ph[1].tick(r, coef(sweep2));
                let m = p.phaser * 0.5;
                l = l * (1.0 - m) + wl * m;
                r = r * (1.0 - m) + wr * m;
            }
            if mix != 0.0 || l.abs() > 1e-6 || r.abs() > 1e-6 {
                any = true;
            }
            out[i * 2] += l * p.gain;
            out[i * 2 + 1] += r * p.gain;
        }
        self.lp.flush_denormals();
        self.hp.flush_denormals();
        self.ph[0].flush();
        self.ph[1].flush();
        if any {
            self.quiet_frames = 0;
        } else {
            self.quiet_frames += n;
        }
        if !self.busy() {
            // tails done: clear the delay line so nothing denormal lingers
            for x in self.ens.buf.iter_mut() {
                *x = 0.0;
            }
            self.lp = Svf::default();
            self.hp = Svf::default();
            self.ph = [Phaser::default(); 2];
        }
        // restore the preset's release after a panic
        if !self.keys.iter().any(|k| k.active) {
            self.set_preset(self.preset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    fn run(s: &mut StringMachine, frames: usize, c: StringControls) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        for ch in out.chunks_mut(128) {
            let n = ch.len() / 2;
            s.render(ch, n, c);
        }
        out
    }

    fn ctl() -> StringControls {
        StringControls { cutoff: 0.8, vibrato: 0.0, bend: 0.0 }
    }

    #[test]
    fn octaves_are_phase_locked() {
        // key 60 and key 72 share a clock: the 8' phase of 60 at twice the
        // speed is the phase of 72
        let s = StringMachine::new(SR);
        let pc = 0;
        let ph60 = (s.clock[pc] / (1u32 << (TOP_OCTAVE - 5)) as f64).fract();
        let ph72 = (s.clock[pc] / (1u32 << (TOP_OCTAVE - 6)) as f64).fract();
        assert!(((ph60 * 2.0).fract() - ph72).abs() < 1e-9);
    }

    #[test]
    fn pitch_is_right() {
        // A4 = 440 Hz: count zero crossings of the raw key (no effects path)
        let s = StringMachine::new(SR);
        let k = 69;
        let pc = k % 12;
        let div = (1u32 << (TOP_OCTAVE - k as i32 / 12)) as f64;
        let f = s.f_top[pc] / div;
        assert!((f - 440.0).abs() < 1e-6, "{f}");
    }

    #[test]
    fn plays_releases_and_goes_quiet() {
        for preset in 0..STRING_PRESETS.len() {
            let mut s = StringMachine::new(SR);
            s.set_preset(preset);
            for k in [48u8, 55, 60, 64, 67, 72] {
                s.note_on(k, 100);
            }
            let c = StringControls { cutoff: 1.0, vibrato: 1.0, bend: -12.0 };
            let out = run(&mut s, 48000, c);
            let rms = (out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32).sqrt();
            assert!(rms > 0.01, "preset {preset}: rms {rms}");
            for x in &out {
                assert!(x.is_finite() && (*x == 0.0 || x.abs() >= f32::MIN_POSITIVE));
            }
            for k in [48u8, 55, 60, 64, 67, 72] {
                s.note_off(k);
            }
            let out = run(&mut s, 48000 * 3, c);
            for x in &out {
                assert!(x.is_finite() && (*x == 0.0 || x.abs() >= f32::MIN_POSITIVE), "denormal/NaN {x:e}");
            }
            assert_eq!(s.active_voices(), 0);
            assert!(!s.busy());
            let out = run(&mut s, 4800, c);
            assert!(out.iter().all(|x| *x == 0.0));
        }
    }

    #[test]
    fn whole_keyboard_is_polyphonic_and_bounded() {
        let mut s = StringMachine::new(SR);
        for k in 0..128u8 {
            s.note_on(k, 127);
        }
        let out = run(&mut s, 24000, ctl());
        assert_eq!(s.active_voices(), 128);
        assert!(out.iter().all(|x| x.is_finite() && x.abs() < 8.0));
    }

    #[test]
    fn sustain_pedal_holds() {
        let mut s = StringMachine::new(SR);
        s.sustain(true);
        s.note_on(60, 100);
        run(&mut s, 4800, ctl());
        s.note_off(60);
        run(&mut s, 48000, ctl());
        assert!(s.keys[60].active && s.keys[60].env > 0.99);
        s.sustain(false);
        run(&mut s, 48000 * 2, ctl());
        assert!(!s.keys[60].active);
    }
}
