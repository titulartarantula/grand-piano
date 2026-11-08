//! Metronome: sample-accurate beat clock and a synthesized click.
//!
//! Timing is exact integer arithmetic. One beat is `60 · sr · 1000` units and
//! every sample adds the tempo in milli-BPM, so beat k (from the start) falls
//! on sample `ceil(k · 60 · sr / bpm)` with no accumulated rounding error
//! however long it runs. A tempo change keeps the fraction of the current beat
//! already played and finishes the beat at the new tempo, so there are no
//! double or skipped clicks.
//!
//! The click is a short sine "pip" (0.4 ms attack, ~6 ms decay, 40 ms long):
//! 2 kHz on the downbeat and 1.3 kHz on the other beats. The engine mixes it
//! after the instrument's effects and limiter.

use crate::synth::fast_sin;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetroControls {
    pub running: bool,
    /// whole BPM (30..240)
    pub bpm: f32,
    pub beats_per_bar: u8,
    pub click_on: bool,
    /// level knob 0..1
    pub level: f32,
}

pub const ACCENT_HZ: f32 = 2000.0;
pub const BEAT_HZ: f32 = 1300.0;

pub struct Metronome {
    sr: f32,
    running: bool,
    acc: u64,
    beat_len: u64,
    beat_in_bar: u8,
    // click voice
    click_age: u32,
    click_len: u32,
    click_phase: f32,
    click_inc: f32,
    click_gain: f32,
    click_env: f32,
    click_decay: f32,
    att_len: f32,
    /// frequency of the last click started (tests)
    pub last_click_hz: f32,
}

impl Metronome {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate as f32;
        Metronome {
            sr,
            running: false,
            acc: 0,
            beat_len: 60 * sample_rate as u64 * 1000,
            beat_in_bar: 0,
            click_age: u32::MAX,
            click_len: (0.04 * sr) as u32,
            click_phase: 0.0,
            click_inc: 0.0,
            click_gain: 0.0,
            click_env: 0.0,
            click_decay: (-1.0 / (0.006 * sr)).exp(),
            att_len: (0.0004 * sr).max(1.0),
            last_click_hz: 0.0,
        }
    }

    pub fn running(&self) -> bool {
        self.running
    }

    /// Level knob -> peak gain of the click (max -6 dBFS).
    pub fn level_gain(level: f32) -> f32 {
        let l = level.clamp(0.0, 1.0);
        0.5 * l * l
    }

    /// Running, or a click is still sounding.
    pub fn busy(&self) -> bool {
        self.running || self.click_age < self.click_len
    }

    /// Renders `n` frames, adding the click into `out` (interleaved stereo).
    /// `on_beat(offset, beat_in_bar)` is called for every beat in this block.
    /// Returns true if any click sound was written.
    pub fn process(&mut self, out: &mut [f32], n: usize, c: MetroControls, mut on_beat: impl FnMut(usize, u8)) -> bool {
        let mut wrote = false;
        if c.running && !self.running {
            // start: the downbeat sounds on the first sample of this block
            self.acc = self.beat_len;
            self.beat_in_bar = u8::MAX; // the first beat becomes 0
        }
        self.running = c.running;
        let bpb = c.beats_per_bar.clamp(2, 6);
        let inc = (c.bpm.clamp(30.0, 240.0).round() as u64) * 1000;
        for i in 0..n {
            if self.running {
                if self.acc >= self.beat_len {
                    self.acc -= self.beat_len;
                    self.beat_in_bar = if self.beat_in_bar == u8::MAX || self.beat_in_bar + 1 >= bpb { 0 } else { self.beat_in_bar + 1 };
                    on_beat(i, self.beat_in_bar);
                    if c.click_on {
                        let hz = if self.beat_in_bar == 0 { ACCENT_HZ } else { BEAT_HZ };
                        self.last_click_hz = hz;
                        self.click_inc = hz / self.sr;
                        self.click_phase = 0.0;
                        self.click_age = 0;
                        self.click_env = 1.0;
                        self.click_gain = Self::level_gain(c.level) * if self.beat_in_bar == 0 { 1.0 } else { 0.75 };
                    }
                }
                self.acc += inc;
            }
            if self.click_age < self.click_len {
                let a = self.click_age as f32;
                let att = if a < self.att_len { a / self.att_len } else { 1.0 };
                // short linear fade over the last 2 ms so the tail ends at exactly zero
                let left = (self.click_len - self.click_age) as f32 / (0.002 * self.sr);
                let fade = left.min(1.0);
                let s = fast_sin(self.click_phase) * self.click_env * att * fade * self.click_gain;
                self.click_phase += self.click_inc;
                if self.click_phase >= 1.0 {
                    self.click_phase -= 1.0;
                }
                self.click_env *= self.click_decay;
                self.click_age += 1;
                out[i * 2] += s;
                out[i * 2 + 1] += s;
                wrote = true;
            }
        }
        wrote
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctl(bpm: f32) -> MetroControls {
        MetroControls { running: true, bpm, beats_per_bar: 4, click_on: false, level: 0.6 }
    }

    /// Runs the metronome for `frames`, in blocks of varying size, and returns
    /// the absolute frame and beat-in-bar of every beat.
    fn run(m: &mut Metronome, start: u64, frames: u64, c: MetroControls, beats: &mut Vec<(u64, u8)>) {
        let sizes = [1usize, 7, 64, 128, 333, 480, 512, 2];
        let mut buf = vec![0f32; 1024];
        let mut pos = 0u64;
        let mut i = 0;
        while pos < frames {
            let n = (sizes[i % sizes.len()] as u64).min(frames - pos) as usize;
            i += 1;
            let base = start + pos;
            m.process(&mut buf[..n * 2], n, c, |off, b| beats.push((base + off as u64, b)));
            pos += n as u64;
        }
    }

    #[test]
    fn ten_minutes_without_drift() {
        for sr in [44100u32, 48000, 96000] {
            for bpm in [30.0f32, 60.0, 100.0, 137.0, 240.0] {
                let mut m = Metronome::new(sr);
                let frames = 600 * sr as u64;
                let mut beats = Vec::new();
                run(&mut m, 0, frames, ctl(bpm), &mut beats);
                let num = 60 * sr as u64 * 1000;
                let den = bpm as u64 * 1000;
                // beat k at ceil(k * 60 * sr / bpm), exactly
                let expected = (frames * den).div_ceil(num); // beats with frame < frames
                assert_eq!(beats.len() as u64, expected, "sr {sr} bpm {bpm}: beat count");
                for (k, (f, _)) in beats.iter().enumerate() {
                    let want = (k as u64 * num).div_ceil(den);
                    assert_eq!(*f, want, "sr {sr} bpm {bpm}: beat {k} at {f}, expected {want}");
                }
                // 10 minutes at 100 BPM is exactly 1000 beats
                if bpm == 100.0 {
                    assert_eq!(beats.len(), 1000);
                    assert_eq!(beats[999].0, 999 * 60 * sr as u64 / 100);
                }
            }
        }
    }

    #[test]
    fn tempo_change_mid_beat_is_smooth() {
        let sr = 48000u32;
        let mut m = Metronome::new(sr);
        let mut beats = Vec::new();
        // 120 BPM: beats every 24000 frames; change halfway through beat 3
        run(&mut m, 0, 60000, ctl(120.0), &mut beats);
        assert_eq!(beats.iter().map(|b| b.0).collect::<Vec<_>>(), vec![0, 24000, 48000]);
        // the rest of the half-played beat now takes half of a 60 BPM beat (24000)
        run(&mut m, 60000, 100000, ctl(60.0), &mut beats);
        let f: Vec<u64> = beats.iter().map(|b| b.0).collect();
        assert_eq!(f, vec![0, 24000, 48000, 84000, 132000]);
        // no doubles, no skips: beat-in-bar keeps counting
        let b: Vec<u8> = beats.iter().map(|b| b.1).collect();
        assert_eq!(b, vec![0, 1, 2, 3, 0]);
        // and speeding up mid-beat
        let mut m = Metronome::new(sr);
        let mut beats = Vec::new();
        run(&mut m, 0, 30000, ctl(60.0), &mut beats); // beat 0, then 30000/48000 of beat 1
        run(&mut m, 30000, 48000, ctl(240.0), &mut beats);
        let f: Vec<u64> = beats.iter().map(|b| b.0).collect();
        // remaining 18000/48000 of a beat at 240 BPM = 4500 frames
        assert_eq!(f[..3], [0, 34500, 46500]);
    }

    #[test]
    fn accent_on_beat_one() {
        let mut m = Metronome::new(48000);
        let mut c = ctl(240.0);
        c.click_on = true;
        let mut beats = Vec::new();
        let mut hz = Vec::new();
        let mut buf = vec![0f32; 2];
        for f in 0..(48000 * 3) {
            let mut fired = None;
            m.process(&mut buf, 1, c, |_, b| fired = Some(b));
            if let Some(b) = fired {
                beats.push((f, b));
                hz.push(m.last_click_hz);
            }
        }
        // 240 BPM for 3 s = 12 beats, bars of 4
        assert_eq!(beats.len(), 12);
        for (i, (_, b)) in beats.iter().enumerate() {
            assert_eq!(*b as usize, i % 4);
            assert_eq!(hz[i], if i % 4 == 0 { ACCENT_HZ } else { BEAT_HZ });
        }
        // beats per bar change mid-bar: 4 -> 3 at beat 2 of the bar
        let mut m = Metronome::new(48000);
        let mut beats = Vec::new();
        run(&mut m, 0, 24001, ctl(240.0), &mut beats); // beats 0,1,2 (frames 0, 12000, 24000)
        let mut c3 = ctl(240.0);
        c3.beats_per_bar = 3;
        run(&mut m, 24001, 48000, c3, &mut beats);
        let b: Vec<u8> = beats.iter().map(|b| b.1).collect();
        assert_eq!(b, vec![0, 1, 2, 0, 1, 2, 0]);
    }

    #[test]
    fn stop_and_restart_begins_on_the_downbeat() {
        let mut m = Metronome::new(48000);
        let mut beats = Vec::new();
        run(&mut m, 0, 30000, ctl(120.0), &mut beats);
        let mut off = ctl(120.0);
        off.running = false;
        run(&mut m, 30000, 10000, off, &mut beats);
        assert_eq!(beats.len(), 2);
        run(&mut m, 40000, 100, ctl(120.0), &mut beats);
        assert_eq!(beats[2], (40000, 0), "restart sounds the downbeat immediately");
    }

    #[test]
    fn click_is_clean() {
        let mut m = Metronome::new(48000);
        let mut c = ctl(120.0);
        c.click_on = true;
        c.level = 1.0;
        let mut buf = vec![0f32; 48000 * 2];
        m.process(&mut buf, 48000, c, |_, _| {});
        let peak = buf.iter().fold(0f32, |a, b| a.max(b.abs()));
        assert!(peak <= 0.5 && peak > 0.4, "{peak}");
        // starts from zero and ends at exactly zero
        assert!(buf[0].abs() < 1e-6);
        let end = (0.04 * 48000.0) as usize;
        assert!(buf[(end + 1) * 2..24000 * 2].iter().all(|x| *x == 0.0));
        assert!(buf[(end - 2) * 2].abs() < 1e-3);
    }
}
