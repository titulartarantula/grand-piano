//! Effects: tone filter, shimmer (chorus + gentle tremolo), room reverb, limiter.

use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn identity() -> Self {
        Biquad { b0: 1.0, ..Default::default() }
    }
    fn set(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }
    /// RBJ low-pass
    pub fn lowpass(&mut self, sr: f32, f: f32, q: f32) {
        let w = TAU * (f / sr).min(0.49);
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        self.set((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
    }
    /// RBJ high shelf
    pub fn highshelf(&mut self, sr: f32, f: f32, gain_db: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let w = TAU * (f / sr).min(0.49);
        let (s, c) = w.sin_cos();
        let alpha = s / 2.0 * (2.0f32).sqrt(); // S = 1
        let sa = 2.0 * a.sqrt() * alpha;
        self.set(
            a * ((a + 1.0) + (a - 1.0) * c + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - sa),
            (a + 1.0) - (a - 1.0) * c + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - sa,
        );
    }
    #[inline(always)]
    pub fn tick(&mut self, x: f32) -> f32 {
        // transposed direct form II
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Tone: 0.5 = neutral. Below: darker (low-pass sweeps 20 kHz -> 1.2 kHz).
/// Above: brighter (high shelf at 2.5 kHz up to +8 dB).
pub struct Tone {
    sr: f32,
    current: f32,
    lp: [Biquad; 2],
    hs: [Biquad; 2],
}

impl Tone {
    pub fn new(sr: f32) -> Self {
        let mut t = Tone { sr, current: -1.0, lp: [Biquad::identity(); 2], hs: [Biquad::identity(); 2] };
        t.update(0.5);
        t
    }
    pub fn update(&mut self, tone: f32) {
        if (tone - self.current).abs() < 1e-4 {
            return;
        }
        self.current = tone;
        let dark = ((0.5 - tone) * 2.0).clamp(0.0, 1.0);
        let bright = ((tone - 0.5) * 2.0).clamp(0.0, 1.0);
        let fc = 20000.0 * (1200.0f32 / 20000.0).powf(dark);
        let gain = 8.0 * bright;
        for ch in 0..2 {
            self.lp[ch].lowpass(self.sr, fc.min(self.sr * 0.45), 0.707);
            self.hs[ch].highshelf(self.sr, 2500.0, gain);
        }
    }
    #[inline(always)]
    pub fn tick(&mut self, l: f32, r: f32) -> (f32, f32) {
        let l = self.hs[0].tick(self.lp[0].tick(l));
        let r = self.hs[1].tick(self.lp[1].tick(r));
        (l, r)
    }
}

/// "Shimmer" — what the mod wheel does: a lush stereo chorus whose depth and mix
/// follow the wheel, plus a very gentle stereo tremolo (auto-pan) in the upper half.
pub struct Shimmer {
    sr: f32,
    buf: [Vec<f32>; 2],
    w: usize,
    phase: f32,
    trem_phase: f32,
}

impl Shimmer {
    pub fn new(sr: f32) -> Self {
        let n = (sr * 0.05) as usize + 4;
        Shimmer { sr, buf: [vec![0.0; n], vec![0.0; n]], w: 0, phase: 0.0, trem_phase: 0.0 }
    }
    #[inline(always)]
    fn read(buf: &[f32], w: usize, delay: f32) -> f32 {
        let n = buf.len();
        let pos = w as f32 + n as f32 - delay;
        let i = pos.floor();
        let frac = pos - i;
        let i = i as usize % n;
        let j = (i + 1) % n;
        buf[i] * (1.0 - frac) + buf[j] * frac
    }
    /// `amount` 0..1 (mod wheel)
    #[inline(always)]
    pub fn tick(&mut self, l: f32, r: f32, amount: f32) -> (f32, f32) {
        let n = self.buf[0].len();
        self.buf[0][self.w] = l;
        self.buf[1][self.w] = r;
        self.w = (self.w + 1) % n;
        self.phase += 0.55 / self.sr;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        self.trem_phase += 4.2 / self.sr;
        if self.trem_phase >= 1.0 {
            self.trem_phase -= 1.0;
        }
        if amount <= 0.0005 {
            return (l, r);
        }
        let base = 0.011 * self.sr;
        let depth = 0.0035 * self.sr * amount;
        let s1 = (TAU * self.phase).sin();
        let s2 = (TAU * self.phase + PI * 0.5).sin();
        let wl = Self::read(&self.buf[0], self.w, base + depth * (0.5 + 0.5 * s1));
        let wr = Self::read(&self.buf[1], self.w, base + depth * (0.5 + 0.5 * s2));
        let mix = 0.5 * amount;
        let mut ol = l * (1.0 - mix * 0.5) + wl * mix;
        let mut or = r * (1.0 - mix * 0.5) + wr * mix;
        // gentle stereo tremolo, only above half wheel, max ±15 %
        let trem = ((amount - 0.5) * 2.0).max(0.0) * 0.15;
        if trem > 0.0 {
            let t = (TAU * self.trem_phase).sin() * trem;
            ol *= 1.0 + t;
            or *= 1.0 - t;
        }
        (ol, or)
    }
}

// ---------------- Freeverb-style room ----------------

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}
impl Comb {
    fn new(n: usize) -> Self {
        Comb { buf: vec![0.0; n.max(1)], idx: 0, store: 0.0 }
    }
    #[inline(always)]
    fn tick(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let y = self.buf[self.idx];
        self.store = y * (1.0 - damp) + self.store * damp;
        self.buf[self.idx] = x + self.store * feedback;
        self.idx += 1;
        if self.idx == self.buf.len() {
            self.idx = 0;
        }
        y
    }
}
struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}
impl Allpass {
    fn new(n: usize) -> Self {
        Allpass { buf: vec![0.0; n.max(1)], idx: 0 }
    }
    #[inline(always)]
    fn tick(&mut self, x: f32) -> f32 {
        let b = self.buf[self.idx];
        let y = -x + b;
        self.buf[self.idx] = x + b * 0.5;
        self.idx += 1;
        if self.idx == self.buf.len() {
            self.idx = 0;
        }
        y
    }
}

pub struct Reverb {
    combs: [Vec<Comb>; 2],
    alls: [Vec<Allpass>; 2],
    pre: Vec<f32>,
    pre_idx: usize,
    lp_in: f32,
}

const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;

impl Reverb {
    pub fn new(sr: f32) -> Self {
        let k = sr / 44100.0 * 0.9; // a slightly smaller room than classic Freeverb
        let mk = |spread: usize| -> (Vec<Comb>, Vec<Allpass>) {
            (
                COMB_TUNING.iter().map(|&n| Comb::new(((n + spread) as f32 * k) as usize)).collect(),
                ALLPASS_TUNING.iter().map(|&n| Allpass::new(((n + spread) as f32 * k) as usize)).collect(),
            )
        };
        let (cl, al) = mk(0);
        let (cr, ar) = mk(STEREO_SPREAD);
        Reverb { combs: [cl, cr], alls: [al, ar], pre: vec![0.0; (sr * 0.012) as usize + 1], pre_idx: 0, lp_in: 0.0 }
    }
    /// Returns the wet signal only.
    #[inline(always)]
    pub fn tick(&mut self, l: f32, r: f32) -> (f32, f32) {
        const FEEDBACK: f32 = 0.82;
        const DAMP: f32 = 0.38;
        let x = (l + r) * 0.015;
        // gentle low-pass on the input so the room is warm
        self.lp_in += 0.45 * (x - self.lp_in);
        let delayed = self.pre[self.pre_idx];
        self.pre[self.pre_idx] = self.lp_in;
        self.pre_idx = (self.pre_idx + 1) % self.pre.len();
        let mut out = [0.0f32; 2];
        for ch in 0..2 {
            let mut acc = 0.0;
            for c in self.combs[ch].iter_mut() {
                acc += c.tick(delayed, FEEDBACK, DAMP);
            }
            for a in self.alls[ch].iter_mut() {
                acc = a.tick(acc);
            }
            out[ch] = acc;
        }
        (out[0], out[1])
    }
}

/// Look-ahead peak limiter: 1 ms look-ahead (adds 1 ms latency), ceiling -2 dBFS,
/// 120 ms release. The gain is the minimum required gain over the look-ahead
/// window, box-smoothed over the same window, so it is already fully applied when
/// a peak arrives (no flat-topping). A soft clipper behind it is a last resort
/// and keeps the output below ±0.98 whatever happens.
pub struct Limiter {
    look: usize,
    delay: Vec<(f32, f32)>,
    req: Vec<f32>,
    boxbuf: Vec<f32>,
    box_sum: f32,
    idx: usize,
    rel_coef: f32,
    gain: f32,
    pub ceiling: f32,
}

impl Limiter {
    pub fn new(sr: f32) -> Self {
        let look = ((sr * 0.001) as usize).max(8);
        Limiter {
            look,
            delay: vec![(0.0, 0.0); look],
            req: vec![1.0; look],
            boxbuf: vec![1.0; look],
            box_sum: look as f32,
            idx: 0,
            rel_coef: 1.0 - (-1.0 / (0.120 * sr)).exp(),
            gain: 1.0,
            ceiling: 10f32.powf(-2.0 / 20.0),
        }
    }
    pub fn latency_frames(&self) -> usize {
        self.look
    }
    /// Returns (l, r, gain applied)
    #[inline(always)]
    pub fn tick(&mut self, l: f32, r: f32) -> (f32, f32, f32) {
        let peak = l.abs().max(r.abs());
        let need = if peak > self.ceiling { self.ceiling / peak } else { 1.0 };
        let i = self.idx;
        let (dl, dr) = self.delay[i];
        self.delay[i] = (l, r);
        self.req[i] = need;
        // minimum required gain over the look-ahead window
        let mut m = 1.0f32;
        for &g in self.req.iter() {
            m = m.min(g);
        }
        // box smoothing over the same window
        self.box_sum += m - self.boxbuf[i];
        self.boxbuf[i] = m;
        self.idx = if i + 1 == self.look { 0 } else { i + 1 };
        let target = (self.box_sum / self.look as f32).min(1.0);
        if target < self.gain {
            self.gain = target;
        } else {
            self.gain += (target - self.gain) * self.rel_coef;
        }
        let g = self.gain.min(1.0);
        (soft_clip(dl * g), soft_clip(dr * g), g)
    }
}

/// Linear below 0.8, smoothly saturating to 0.98 above.
#[inline(always)]
pub fn soft_clip(x: f32) -> f32 {
    const K: f32 = 0.8;
    const H: f32 = 0.98 - K;
    let a = x.abs();
    if a <= K {
        x
    } else {
        let y = K + H * ((a - K) / H).tanh();
        y.copysign(x)
    }
}

/// One-pole parameter smoother.
pub struct Smooth {
    pub value: f32,
    coef: f32,
}
impl Smooth {
    pub fn new(v: f32, sr: f32, ms: f32) -> Self {
        Smooth { value: v, coef: 1.0 - (-1.0 / (ms * 0.001 * sr)).exp() }
    }
    #[inline(always)]
    pub fn next(&mut self, target: f32) -> f32 {
        self.value += (target - self.value) * self.coef;
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_never_exceeds_ceiling_much() {
        let mut lim = Limiter::new(48000.0);
        let mut max = 0f32;
        for i in 0..48000 {
            let x = 3.0 * (i as f32 * 0.05).sin();
            let (l, r, _) = lim.tick(x, -x);
            max = max.max(l.abs()).max(r.abs());
        }
        assert!(max <= 0.98, "{max}");
        // look-ahead: a single loud spike is attenuated without flat-topping
        let mut lim = Limiter::new(48000.0);
        let mut out = Vec::new();
        for i in 0..2000 {
            let x = if (1000..1010).contains(&i) { 2.0 * ((i - 1000) as f32 / 9.0 * PI).sin() } else { 0.0 };
            out.push(lim.tick(x, x).0);
        }
        let m = out.iter().fold(0f32, |a, b| a.max(b.abs()));
        assert!(m <= lim.ceiling + 1e-4, "{m}");
    }

    #[test]
    fn soft_clip_transparent_below_knee() {
        assert_eq!(soft_clip(0.5), 0.5);
        assert!(soft_clip(10.0) < 0.981);
        assert!(soft_clip(-10.0) > -0.981);
    }

    #[test]
    fn tone_neutral_is_transparent() {
        let mut t = Tone::new(48000.0);
        t.update(0.5);
        // 1 kHz sine passes at ~unity
        let mut peak = 0f32;
        for i in 0..48000 {
            let x = (TAU * 1000.0 * i as f32 / 48000.0).sin();
            let (l, _) = t.tick(x, x);
            if i > 4800 {
                peak = peak.max(l.abs());
            }
        }
        assert!((peak - 1.0).abs() < 0.03, "{peak}");
    }

    #[test]
    fn reverb_is_stable() {
        let mut rv = Reverb::new(48000.0);
        let mut e = 0f32;
        for i in 0..48000 * 6 {
            let x = if i < 100 { 1.0 } else { 0.0 };
            let (l, r) = rv.tick(x, x);
            assert!(l.is_finite() && r.is_finite());
            if i > 48000 * 5 {
                e = e.max(l.abs());
            }
        }
        assert!(e < 1e-3, "tail still {e}");
    }
}
