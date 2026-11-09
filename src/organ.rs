//! The organ: a tonewheel organ (in the style of the classic B3) with a
//! rotating-speaker cabinet (in the style of the Leslie 122), generated in
//! real time.
//!
//! - **Tonewheel generator.** 91 wheels, free-running all the time, tuned with
//!   the real generator's 12 gear ratios (so it's very slightly off equal
//!   temperament, as the originals are): wheel frequency = 20 rev/s × ratio ×
//!   teeth. Each key's nine drawbars (16', 5⅓', 8', 4', 2⅔', 2', 1⅗', 1⅓', 1')
//!   tap wheels at fixed intervals, folding back at the top as the real
//!   generator does. Drawbar steps are about 3 dB. The organ sums *per wheel*
//!   (a wheel shared by several keys is one sine), so its cost doesn't grow with
//!   the number of keys held.
//! - **Leakage/crosstalk:** every sounding wheel bleeds a little into its
//!   octave neighbours and the wheel sharing its generator compartment
//!   (about −40 dB).
//! - **Key click:** a short filtered noise burst when a key goes down (and a
//!   softer one on release), and the wheels' contacts close in about 2 ms.
//! - **Percussion:** 2nd (4') or 3rd (2⅔') harmonic, soft or normal, fast or
//!   slow decay, single-triggered as on the original: the envelope fires only
//!   when a key is pressed with no other key held; legato notes don't
//!   retrigger it. While percussion is on, the 1' drawbar is silent (on the
//!   original its key contact is borrowed by the percussion), and in Normal
//!   volume the drawbars drop about 3 dB.
//! - **No velocity:** keys are on/off.
//! - **Overdrive:** a cubic soft-clipping preamp (Tone = drive and
//!   brightness). The organ's own harmonics stop near 6 kHz, and a cubic only
//!   makes up to the 3rd harmonic, so it stays below Nyquist.
//! - **Rotating speaker:** an 800 Hz crossover into a horn (treble) and a
//!   drum (bass) rotor. Each rotor's angle drives a modulated delay (the
//!   Doppler shift) and its loudness towards two virtual microphones (left and
//!   right, 90° apart), so the sound swirls in stereo. Chorale (slow) and
//!   tremolo (fast) speeds with inertia: the light horn gets up to speed in
//!   about a second, the heavy drum takes several.
//!
//! Real-time safe: no allocation after construction, no locks.

use crate::synth::{Svf, fast_sin, fast_tan, flush};
use std::f32::consts::PI;

pub const WHEELS: usize = 91;
/// Output level, matched to the piano (a triad on Jazz peaks near -6 dBFS).
const OUT_GAIN: f32 = 0.039;
/// Drawbar footages in semitones above the 8' (unison) pitch.
pub const DRAWBAR_SEMIS: [i32; 9] = [-12, 7, 0, 12, 19, 24, 28, 31, 36];
pub const DRAWBAR_NAMES: [&str; 9] = ["16'", "5⅓'", "8'", "4'", "2⅔'", "2'", "1⅗'", "1⅓'", "1'"];
/// The generator's gear ratios for C, C#, ... B (wheel frequency = 20 × ratio × teeth).
const GEAR: [f64; 12] = [85.0 / 104.0, 71.0 / 82.0, 67.0 / 73.0, 105.0 / 108.0, 103.0 / 100.0, 84.0 / 77.0, 74.0 / 64.0, 98.0 / 80.0, 96.0 / 74.0, 88.0 / 64.0, 67.0 / 46.0, 108.0 / 70.0];

/// Rotor speeds in Hz: (horn, drum) for chorale and tremolo.
pub const HORN_SLOW: f32 = 0.83;
pub const HORN_FAST: f32 = 6.7;
pub const DRUM_SLOW: f32 = 0.67;
pub const DRUM_FAST: f32 = 5.7;
/// Speed-change time constants (s): (spin up, spin down). Horn is light, drum heavy.
const HORN_TAU: (f32, f32) = (0.45, 0.6);
const DRUM_TAU: (f32, f32) = (2.2, 3.2);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrganPreset {
    pub name: &'static str,
    pub drawbars: [u8; 9],
    pub perc: bool,
    /// true = 3rd harmonic (2⅔'), false = 2nd (4')
    pub perc_third: bool,
    pub perc_fast: bool,
    pub perc_soft: bool,
    /// Tone (drive) knob default
    pub drive: f32,
    /// key click level
    pub click: f32,
}

pub const ORGAN_PRESETS: [OrganPreset; 4] = [
    OrganPreset { name: "Jazz", drawbars: [8, 8, 8, 0, 0, 0, 0, 0, 0], perc: false, perc_third: true, perc_fast: true, perc_soft: false, drive: 0.3, click: 1.0 },
    OrganPreset { name: "Gospel", drawbars: [8, 8, 8, 8, 0, 0, 0, 0, 0], perc: true, perc_third: true, perc_fast: true, perc_soft: false, drive: 0.45, click: 1.0 },
    OrganPreset { name: "Rock", drawbars: [8, 8, 8, 8, 8, 8, 8, 8, 8], perc: false, perc_third: true, perc_fast: true, perc_soft: false, drive: 0.8, click: 0.8 },
    OrganPreset { name: "Ballad", drawbars: [0, 0, 8, 8, 0, 0, 0, 0, 0], perc: false, perc_third: true, perc_fast: false, perc_soft: true, drive: 0.15, click: 0.5 },
];

/// Drawbar position (0..8) -> amplitude (about 3 dB per step, 0 = off).
pub fn drawbar_gain(pos: u8) -> f32 {
    if pos == 0 { 0.0 } else { 10f32.powf((pos.min(8) as f32 - 8.0) * 3.0 / 20.0) }
}

/// The wheel (0-based) that key `key`'s drawbar `d` taps. The manual's lowest
/// C (MIDI 36) plays wheel 13 (C2) at 8'; keys outside the manual's range
/// fold back by octaves into the 91 wheels.
pub fn wheel_for(key: u8, d: usize) -> usize {
    let mut w = key as i32 - 36 + 12 + DRAWBAR_SEMIS[d];
    while w < 0 {
        w += 12;
    }
    while w >= WHEELS as i32 {
        w -= 12;
    }
    w as usize
}

pub fn wheel_hz(w: usize) -> f64 {
    let oct = w / 12;
    20.0 * GEAR[w % 12] * (2u32 << oct) as f64
}

#[derive(Clone, Copy, Debug)]
pub struct OrganControls {
    pub drawbars: [u8; 9],
    pub perc: bool,
    pub perc_third: bool,
    pub perc_fast: bool,
    pub perc_soft: bool,
    /// Tone knob 0..1: overdrive and brightness
    pub drive: f32,
    /// Leslie speed: false = chorale (slow), true = tremolo (fast)
    pub leslie_fast: bool,
    /// pitch bend in semitones (off by default: range 0)
    pub bend: f32,
}

impl OrganControls {
    pub fn from_preset(p: &OrganPreset) -> Self {
        OrganControls { drawbars: p.drawbars, perc: p.perc, perc_third: p.perc_third, perc_fast: p.perc_fast, perc_soft: p.perc_soft, drive: p.drive, leslie_fast: false, bend: 0.0 }
    }
}

/// Fractional delay line with cubic (Hermite) reads.
struct Delay {
    buf: Vec<f32>,
    w: usize,
}

impl Delay {
    fn new(len: usize) -> Self {
        Delay { buf: vec![0.0; len.next_power_of_two()], w: 0 }
    }
    #[inline(always)]
    fn push(&mut self, x: f32) {
        let m = self.buf.len() - 1;
        self.buf[self.w] = x;
        self.w = (self.w + 1) & m;
    }
    /// Reads `d` samples back (d >= 2).
    #[inline(always)]
    fn read(&self, d: f32) -> f32 {
        let m = self.buf.len() - 1;
        let pos = self.w as f32 + self.buf.len() as f32 - 1.0 - d;
        let i = pos as usize;
        let t = pos - i as f32;
        let y0 = self.buf[(i.wrapping_sub(1)) & m];
        let y1 = self.buf[i & m];
        let y2 = self.buf[(i + 1) & m];
        let y3 = self.buf[(i + 2) & m];
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }
    fn clear(&mut self) {
        for x in self.buf.iter_mut() {
            *x = 0.0;
        }
    }
}

/// The rotating speaker.
pub struct Leslie {
    sr: f32,
    xover: Svf,
    horn: Delay,
    drum: Delay,
    /// rotor angles (cycles, 0..1) and speeds (Hz)
    pub horn_angle: f32,
    pub drum_angle: f32,
    pub horn_speed: f32,
    pub drum_speed: f32,
}

impl Leslie {
    pub fn new(sr: f32) -> Self {
        Leslie {
            sr,
            xover: Svf::default(),
            horn: Delay::new((0.004 * sr) as usize + 8),
            drum: Delay::new((0.004 * sr) as usize + 8),
            horn_angle: 0.0,
            drum_angle: 0.31,
            horn_speed: HORN_SLOW,
            drum_speed: DRUM_SLOW,
        }
    }

    /// Advances the rotors by one sample towards the chosen speed.
    #[inline(always)]
    fn spin(&mut self, fast: bool) {
        let sr = self.sr;
        let (ht, dt) = if fast { (HORN_FAST, DRUM_FAST) } else { (HORN_SLOW, DRUM_SLOW) };
        let h_tau = if ht > self.horn_speed { HORN_TAU.0 } else { HORN_TAU.1 };
        let d_tau = if dt > self.drum_speed { DRUM_TAU.0 } else { DRUM_TAU.1 };
        self.horn_speed += (ht - self.horn_speed) / (h_tau * sr);
        self.drum_speed += (dt - self.drum_speed) / (d_tau * sr);
        self.horn_angle += self.horn_speed / sr;
        if self.horn_angle >= 1.0 {
            self.horn_angle -= 1.0;
        }
        self.drum_angle += self.drum_speed / sr;
        if self.drum_angle >= 1.0 {
            self.drum_angle -= 1.0;
        }
    }

    /// Spins the rotors for `n` samples with no sound (keeps their motion continuous).
    pub fn idle(&mut self, n: usize, fast: bool) {
        for _ in 0..n {
            self.spin(fast);
        }
    }

    /// One sample through the cabinet: mono in, stereo out.
    #[inline(always)]
    pub fn tick(&mut self, x: f32, fast: bool, xover_g: f32) -> (f32, f32) {
        self.spin(fast);
        let sr = self.sr;
        // complementary split (horn = input - drum), so the two rotors sum flat:
        // a plain 2nd-order low-pass + high-pass pair notches at the crossover
        let (lo, _) = self.xover.tick(x, xover_g, 1.414);
        let hi = x - lo;
        self.horn.push(hi);
        self.drum.push(lo);
        // horn: Doppler ±0.45 ms around 1.6 ms, strong directional loudness
        let base = 0.0016 * sr;
        let hd = 0.00045 * sr;
        let dd = 0.00018 * sr;
        let mut a_r = self.horn_angle + 0.25;
        if a_r >= 1.0 {
            a_r -= 1.0;
        }
        let mut c_l = self.horn_angle + 0.25; // cos(θ) = sin(θ + ¼)
        if c_l >= 1.0 {
            c_l -= 1.0;
        }
        let mut c_r = a_r + 0.25;
        if c_r >= 1.0 {
            c_r -= 1.0;
        }
        let hl = self.horn.read(base + hd * fast_sin(self.horn_angle)) * (0.45 + 0.55 * (0.5 + 0.5 * fast_sin(c_l)));
        let hr = self.horn.read(base + hd * fast_sin(a_r)) * (0.45 + 0.55 * (0.5 + 0.5 * fast_sin(c_r)));
        // drum: smaller Doppler, gentler loudness swing (the baffle)
        let mut d_r = self.drum_angle + 0.25;
        if d_r >= 1.0 {
            d_r -= 1.0;
        }
        let mut dc_l = self.drum_angle + 0.25;
        if dc_l >= 1.0 {
            dc_l -= 1.0;
        }
        let mut dc_r = d_r + 0.25;
        if dc_r >= 1.0 {
            dc_r -= 1.0;
        }
        let dl = self.drum.read(base + dd * fast_sin(self.drum_angle)) * (0.7 + 0.3 * (0.5 + 0.5 * fast_sin(dc_l)));
        let dr = self.drum.read(base + dd * fast_sin(d_r)) * (0.7 + 0.3 * (0.5 + 0.5 * fast_sin(dc_r)));
        (hl + dl, hr + dr)
    }

    fn flush(&mut self) {
        self.xover.flush_denormals();
    }

    fn clear(&mut self) {
        self.xover = Svf::default();
        self.horn.clear();
        self.drum.clear();
    }
}

pub struct Organ {
    sr: f32,
    preset: usize,
    down: [bool; 128],
    /// keys held by the sustain pedal
    held_by_pedal: [bool; 128],
    pedal: bool,
    /// wheel phases (cycles) and increments
    phase: [f32; WHEELS],
    inc: [f32; WHEELS],
    /// drawbar bus: target and current amplitude per wheel
    target: [f32; WHEELS],
    amp: [f32; WHEELS],
    /// percussion bus: target and current amplitude per wheel
    p_target: [f32; WHEELS],
    p_amp: [f32; WHEELS],
    dirty: bool,
    last: Option<OrganControls>,
    /// percussion envelope (0..1), its attack ramp, and how many times it fired (tests)
    perc_env: f32,
    perc_att: f32,
    pub perc_triggers: u32,
    /// key click: envelope and band-pass filter
    click_env: f32,
    click_bp: Svf,
    noise: crate::synth::Rng,
    drive_s: f32,
    tone_lp: Svf,
    dc: f32,
    pub leslie: Leslie,
    bend_s: f32,
    quiet_frames: usize,
}

impl Organ {
    pub fn new(sr: f32) -> Self {
        let mut inc = [0f32; WHEELS];
        for (w, x) in inc.iter_mut().enumerate() {
            *x = (wheel_hz(w) / sr as f64) as f32;
        }
        // free-running wheels at unrelated phases
        let mut phase = [0f32; WHEELS];
        for (w, p) in phase.iter_mut().enumerate() {
            *p = (w as f32 * 0.618_034).fract();
        }
        Organ {
            sr,
            preset: 0,
            down: [false; 128],
            held_by_pedal: [false; 128],
            pedal: false,
            phase,
            inc,
            target: [0.0; WHEELS],
            amp: [0.0; WHEELS],
            p_target: [0.0; WHEELS],
            p_amp: [0.0; WHEELS],
            dirty: true,
            last: None,
            perc_env: 0.0,
            perc_att: 1.0,
            perc_triggers: 0,
            click_env: 0.0,
            click_bp: Svf::default(),
            noise: crate::synth::Rng(0x0bad_cafe),
            drive_s: -1.0,
            tone_lp: Svf::default(),
            dc: 0.0,
            leslie: Leslie::new(sr),
            bend_s: 0.0,
            quiet_frames: usize::MAX / 2,
        }
    }

    pub fn preset(&self) -> usize {
        self.preset
    }

    pub fn set_preset(&mut self, idx: usize) {
        self.preset = idx.min(ORGAN_PRESETS.len() - 1);
        self.dirty = true;
    }

    fn sounding_key(&self, k: usize) -> bool {
        self.down[k] || self.held_by_pedal[k]
    }

    pub fn active_voices(&self) -> usize {
        (0..128).filter(|k| self.sounding_key(*k)).count()
    }

    pub fn sounding(&self, out: &mut [u64; 2]) {
        for k in 0..128 {
            if self.sounding_key(k) {
                out[k / 64] |= 1 << (k % 64);
            }
        }
    }

    pub fn busy(&self) -> bool {
        self.quiet_frames < (self.sr * 0.05) as usize
    }

    /// Velocity is ignored: organ keys are on/off.
    pub fn note_on(&mut self, key: u8) {
        let k = key as usize & 127;
        // single trigger: percussion fires only if no other key is held
        let others = (0..128).any(|j| j != k && self.down[j]);
        if !others {
            self.perc_env = 0.0;
            self.perc_att = 0.0;
            self.perc_triggers += 1;
        }
        self.down[k] = true;
        self.held_by_pedal[k] = false;
        self.click_env = (self.click_env + 1.0).min(2.0);
        self.dirty = true;
        self.quiet_frames = 0;
    }

    pub fn note_off(&mut self, key: u8) {
        let k = key as usize & 127;
        if !self.down[k] {
            return;
        }
        self.down[k] = false;
        if self.pedal {
            self.held_by_pedal[k] = true;
        } else {
            self.click_env = (self.click_env + 0.35).min(2.0);
        }
        self.dirty = true;
    }

    /// The pedal holds notes (an original console has no sustain pedal; this
    /// is for convenience).
    pub fn sustain(&mut self, down: bool) {
        self.pedal = down;
        if !down && self.held_by_pedal.iter().any(|h| *h) {
            self.held_by_pedal = [false; 128];
            self.dirty = true;
        }
    }

    pub fn release_all(&mut self) {
        self.down = [false; 128];
        self.held_by_pedal = [false; 128];
        self.dirty = true;
    }

    pub fn panic(&mut self) {
        self.release_all();
    }

    /// Rotor speeds (horn, drum) in Hz, for tests and the UI.
    pub fn rotor_speeds(&self) -> (f32, f32) {
        (self.leslie.horn_speed, self.leslie.drum_speed)
    }

    /// Recomputes the per-wheel targets from the held keys and the drawbars.
    fn retarget(&mut self, c: &OrganControls) {
        self.target = [0.0; WHEELS];
        self.p_target = [0.0; WHEELS];
        let mut g = [0f32; 9];
        for d in 0..9 {
            g[d] = drawbar_gain(c.drawbars[d]);
        }
        if c.perc {
            g[8] = 0.0; // the 1' contact is taken by the percussion
            if !c.perc_soft {
                for x in g.iter_mut() {
                    *x *= 0.7;
                }
            }
        }
        // drawbar loading: more drawbars out = each a little quieter
        let e: f32 = g.iter().map(|x| x * x).sum();
        let load = if e > 0.0 { (3.0 / e.max(1.0)).powf(0.3) } else { 1.0 };
        let perc_gain = if c.perc_soft { 0.45 } else { 0.9 };
        let perc_bar = if c.perc_third { 4 } else { 3 };
        for k in 0..128 {
            if !self.sounding_key(k) {
                continue;
            }
            for d in 0..9 {
                if g[d] > 0.0 {
                    let w = wheel_for(k as u8, d);
                    self.target[w] += g[d] * load;
                }
            }
            if c.perc {
                self.p_target[wheel_for(k as u8, perc_bar)] += perc_gain;
            }
        }
        // leakage: each wheel bleeds into its octave neighbours and its generator
        // compartment partner (w ± 48), about -40 dB
        let src = self.target;
        for w in 0..WHEELS {
            let a = src[w];
            if a == 0.0 {
                continue;
            }
            for (off, lk) in [(-12i32, 0.008f32), (12, 0.008), (48, 0.01), (-48, 0.01)] {
                let j = w as i32 + off;
                if (0..WHEELS as i32).contains(&j) {
                    self.target[j as usize] += a * lk;
                }
            }
        }
    }

    /// Adds `n` stereo frames into `out` (interleaved).
    pub fn render(&mut self, out: &mut [f32], n: usize, c: OrganControls) {
        let sr = self.sr;
        if self.last.map(|l| l.drawbars != c.drawbars || l.perc != c.perc || l.perc_third != c.perc_third || l.perc_soft != c.perc_soft) != Some(false) {
            self.dirty = true;
        }
        self.last = Some(c);
        if self.dirty {
            self.retarget(&c);
            self.dirty = false;
        }
        let any_key = (0..128).any(|k| self.sounding_key(k));
        let wheels_on = self.amp.iter().chain(self.p_amp.iter()).any(|a| *a > 1e-6);
        if !any_key && !wheels_on && !self.busy() {
            // silence: the wheels and the rotors keep turning
            for w in 0..WHEELS {
                self.phase[w] = (self.phase[w] + self.inc[w] * n as f32).fract();
            }
            self.leslie.idle(n, c.leslie_fast);
            self.bend_s = c.bend;
            self.drive_s = c.drive;
            return;
        }
        let inv_sr = 1.0 / sr;
        if self.drive_s < 0.0 {
            self.drive_s = c.drive;
        }
        let a = 1.0 - (-(n as f32) / (0.01 * sr)).exp();
        self.drive_s += (c.drive - self.drive_s) * a;
        let drive = self.drive_s;
        // contacts: ~2 ms in, ~4 ms out (exponential, no corners)
        let k_on = 1.0 - (-1.0 / (0.002 * sr)).exp();
        let k_off = 1.0 - (-1.0 / (0.004 * sr)).exp();
        let perc_mul = (-6.9 / ((if c.perc_fast { 0.55 } else { 1.8 }) * sr)).exp();
        let perc_att_inc = 1.0 / (0.001 * sr);
        let click_mul = (-1.0 / (0.004 * sr)).exp();
        let click_g = fast_tan(PI * 3200.0 * inv_sr);
        let click_lvl = ORGAN_PRESETS[self.preset].click * 0.12;
        // preamp: drive 0 dB .. +19 dB, makeup gain, brightness 3.5 .. 12 kHz.
        // One key at low drive stays clean; chords and high drive break up.
        let pre = 1.0 + 8.0 * drive * drive;
        let makeup = 1.0 / pre.powf(0.75);
        let tone_g = fast_tan(PI * (3500.0 * 2f32.powf(drive * 1.8)).min(0.45 * sr) * inv_sr);
        let xover_g = fast_tan(PI * 800.0 * inv_sr);
        let dc_a = 1.0 - (-2.0 * PI * 10.0 * inv_sr).exp();
        let bend_a = 1.0 - (-1.0 / (0.003 * sr)).exp();
        let nyq_inc = 0.45;
        let mut any = false;
        for i in 0..n {
            self.bend_s += (c.bend - self.bend_s) * bend_a;
            let bm = if self.bend_s != 0.0 { crate::synth::exp2(self.bend_s / 12.0) } else { 1.0 };
            // percussion envelope: 1 ms rise, then exponential decay
            if self.perc_att < 1.0 {
                self.perc_att = (self.perc_att + perc_att_inc).min(1.0);
                self.perc_env = self.perc_att;
            } else {
                self.perc_env *= perc_mul;
            }
            let pe = self.perc_env;
            let mut x = 0f32;
            for w in 0..WHEELS {
                let t = self.target[w];
                let am = &mut self.amp[w];
                *am += (t - *am) * if t > *am { k_on } else { k_off };
                let pt = self.p_target[w];
                let pa = &mut self.p_amp[w];
                *pa += (pt - *pa) * if pt > *pa { k_on } else { k_off };
                let lvl = *am + *pa * pe;
                let ph = &mut self.phase[w];
                let inc = self.inc[w] * bm;
                if lvl > 1e-7 && inc < nyq_inc {
                    x += lvl * fast_sin(*ph);
                }
                *ph += inc;
                if *ph >= 1.0 {
                    *ph -= 1.0;
                }
            }
            // key click: band-passed noise burst
            if self.click_env > 1e-5 {
                let nz = self.noise.next_f32();
                let (lp, hp) = self.click_bp.tick(nz, click_g, 0.9);
                x += (0.5 * hp + 0.3 * lp) * self.click_env * click_lvl;
                self.click_env *= click_mul;
            } else {
                self.click_env = 0.0;
            }
            // preamp: slight asymmetry, cubic soft clip (C1-continuous)
            let u = x * 0.08 * pre;
            let u = u + 0.08 * u * u;
            let y = if u > 1.0 {
                1.0
            } else if u < -1.0 {
                -1.0
            } else {
                1.5 * u - 0.5 * u * u * u
            } * makeup
                / 0.08;
            self.dc += (y - self.dc) * dc_a;
            let y = y - self.dc;
            let (y, _) = self.tone_lp.tick(y, tone_g, 1.3);
            let (l, r) = self.leslie.tick(y, c.leslie_fast, xover_g);
            out[i * 2] += flush(l * OUT_GAIN);
            out[i * 2 + 1] += flush(r * OUT_GAIN);
            if l.abs() > 1e-6 || r.abs() > 1e-6 {
                any = true;
            }
        }
        for w in 0..WHEELS {
            self.amp[w] = flush(if self.amp[w] < 1e-9 && self.target[w] == 0.0 { 0.0 } else { self.amp[w] });
            self.p_amp[w] = flush(if self.p_amp[w] < 1e-9 && self.p_target[w] == 0.0 { 0.0 } else { self.p_amp[w] });
        }
        if self.perc_env < 1e-6 && self.perc_att >= 1.0 {
            self.perc_env = 0.0;
        }
        self.click_bp.flush_denormals();
        self.tone_lp.flush_denormals();
        self.leslie.flush();
        self.dc = flush(self.dc);
        if any || any_key {
            self.quiet_frames = 0;
        } else {
            self.quiet_frames += n;
            if !self.busy() {
                self.leslie.clear();
                self.tone_lp = Svf::default();
                self.click_bp = Svf::default();
                self.dc = 0.0;
                self.amp = [0.0; WHEELS];
                self.p_amp = [0.0; WHEELS];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::band_db;

    const SR: f32 = 48000.0;

    fn ctl(p: usize) -> OrganControls {
        OrganControls::from_preset(&ORGAN_PRESETS[p])
    }

    fn run(s: &mut Organ, frames: usize, c: OrganControls) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        for ch in out.chunks_mut(128) {
            let n = ch.len() / 2;
            s.render(ch, n, c);
        }
        out
    }

    #[test]
    fn wheels_are_tuned_like_the_generator() {
        // wheel 10 (A1) is exactly 55 Hz with the 88/64 gear; A4 (key 69 at 8') is 440
        assert!((wheel_hz(9) - 55.0).abs() < 1e-9);
        assert!((wheel_hz(wheel_for(69, 2)) - 440.0).abs() < 1e-6);
        // every wheel is within 1.5 cents of equal temperament
        for w in 0..WHEELS {
            let et = 440.0 * 2f64.powf((w as f64 + 24.0 - 69.0) / 12.0);
            let cents = 1200.0 * (wheel_hz(w) / et).log2();
            assert!(cents.abs() < 1.5, "wheel {w}: {cents:.2} cents");
        }
        // top foldback: the 1' of the top C stays inside the generator
        assert!(wheel_for(96, 8) < WHEELS && wheel_for(108, 8) < WHEELS);
        assert_eq!(wheel_for(36, 0), 0, "16' of the lowest C is wheel 1");
    }

    #[test]
    fn percussion_is_single_triggered() {
        let mut s = Organ::new(SR);
        let c = ctl(1); // Gospel: percussion on
        s.note_on(60);
        assert_eq!(s.perc_triggers, 1);
        run(&mut s, 4800, c);
        let decayed = s.perc_env;
        assert!(decayed < 0.9 && decayed > 0.0);
        // legato: a second key while the first is held doesn't retrigger
        s.note_on(64);
        assert_eq!(s.perc_triggers, 1);
        run(&mut s, 480, c);
        assert!(s.perc_env < decayed, "still decaying");
        // releasing one key and pressing another while one is still held: no retrigger
        s.note_off(60);
        s.note_on(67);
        assert_eq!(s.perc_triggers, 1);
        // all keys up, then a new key: fires again
        s.note_off(64);
        s.note_off(67);
        run(&mut s, 480, c);
        s.note_on(72);
        assert_eq!(s.perc_triggers, 2);
        run(&mut s, 96, c);
        assert!(s.perc_env > 0.9);
        // and it's audible: the 3rd harmonic (2⅔') is much stronger just after
        // the strike than later
        let mut s = Organ::new(SR);
        s.note_on(60);
        let out = run(&mut s, 48000, c);
        let f3 = wheel_hz(wheel_for(60, 4)) as f32;
        let early = band_db(&out[..9600 * 2], SR, f3, 8.0);
        let late = band_db(&out[38400 * 2..], SR, f3, 8.0);
        assert!(early > late + 12.0, "percussion early {early:.1} late {late:.1}");
    }

    #[test]
    fn leslie_ramps_with_inertia() {
        let mut s = Organ::new(SR);
        let mut c = ctl(0);
        s.note_on(60);
        run(&mut s, 4800, c);
        let (h, d) = s.rotor_speeds();
        assert!((h - HORN_SLOW).abs() < 0.01 && (d - DRUM_SLOW).abs() < 0.01);
        // switch to fast: the horn gets there quickly, the drum lags
        c.leslie_fast = true;
        run(&mut s, 24000, c); // 0.5 s
        let (h, d) = s.rotor_speeds();
        let hf = (h - HORN_SLOW) / (HORN_FAST - HORN_SLOW);
        let df = (d - DRUM_SLOW) / (DRUM_FAST - DRUM_SLOW);
        assert!(hf > 0.6 && df < 0.3, "after 0.5 s: horn {hf:.2}, drum {df:.2} of the way");
        run(&mut s, 48000, c); // 1.5 s
        let (h, d) = s.rotor_speeds();
        assert!((HORN_FAST - h) / (HORN_FAST - HORN_SLOW) < 0.05, "horn at {h}");
        assert!((DRUM_FAST - d) / (DRUM_FAST - DRUM_SLOW) > 0.25, "drum already at {d}");
        run(&mut s, 48000 * 12, c);
        let (h, d) = s.rotor_speeds();
        assert!((h - HORN_FAST).abs() < 0.02 && (d - DRUM_FAST).abs() < 0.05, "{h} {d}");
        // and back to slow, also with inertia (and the rotors keep turning while silent)
        c.leslie_fast = false;
        s.note_off(60);
        run(&mut s, 24000, c);
        let (h, d) = s.rotor_speeds();
        assert!(h < HORN_FAST * 0.6 && d > DRUM_FAST * 0.8, "{h} {d}");
        run(&mut s, 48000 * 20, c);
        let (h, d) = s.rotor_speeds();
        assert!((h - HORN_SLOW).abs() < 0.02 && (d - DRUM_SLOW).abs() < 0.02);
    }

    #[test]
    fn leslie_modulates_in_stereo() {
        // fast speed: the left and right channels differ and the level swirls
        let mut s = Organ::new(SR);
        let mut c = ctl(0);
        c.leslie_fast = true;
        s.leslie.horn_speed = HORN_FAST;
        s.leslie.drum_speed = DRUM_FAST;
        s.note_on(84); // C6: mostly through the horn
        let out = run(&mut s, 48000, c);
        let seg = &out[9600 * 2..];
        let diff: f32 = seg.chunks(2).map(|f| (f[0] - f[1]).powi(2)).sum::<f32>();
        let sum: f32 = seg.chunks(2).map(|f| (f[0] + f[1]).powi(2)).sum::<f32>();
        assert!(diff > 0.05 * sum, "stereo difference {diff} vs {sum}");
    }

    #[test]
    fn all_keys_with_rock_drawbars_is_clean_and_stops() {
        let mut s = Organ::new(SR);
        let mut c = ctl(2);
        c.leslie_fast = true;
        for k in 36..=96u8 {
            s.note_on(k);
        }
        let out = run(&mut s, 48000, c);
        for x in &out {
            assert!(x.is_finite() && (*x == 0.0 || x.abs() >= f32::MIN_POSITIVE) && x.abs() < 4.0);
        }
        s.release_all();
        let out = run(&mut s, 48000, c);
        for x in &out {
            assert!(x.is_finite() && (*x == 0.0 || x.abs() >= f32::MIN_POSITIVE), "{x:e}");
        }
        assert_eq!(s.active_voices(), 0);
        let out = run(&mut s, 4800, c);
        assert!(out.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn organ_ignores_velocity_and_one_prime_drops_with_percussion() {
        // the 1' drawbar is silent with percussion on
        let mut c = ctl(2);
        let lvl = |c: OrganControls| {
            let mut s = Organ::new(SR);
            s.note_on(60);
            let out = run(&mut s, 24000, c);
            band_db(&out[9600 * 2..], SR, wheel_hz(wheel_for(60, 8)) as f32, 6.0)
        };
        let with = lvl(c);
        c.perc = true;
        let without = lvl(c);
        // what's left is the 2' wheel's leakage into the 1' wheel (about -40 dB)
        assert!(with > without + 10.0, "1' {with:.1} dB, with percussion on {without:.1} dB");
    }
}
