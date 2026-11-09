//! The sampler engine. Runs on the audio thread: never allocates, never locks.
//!
//! Damper model: a key that is up decays at a rate set by the sustain pedal
//! position. Pedal fully down = no extra decay (the sample's natural decay),
//! pedal up = dampers on the strings (fast decay), and anything in between is a
//! half pedal with partial damping. When the dampers land on a string (key up
//! with the pedal up, or the pedal lifted after the key was released) the
//! Salamander string-resonance release samples are triggered, attenuated by
//! `rt_decay` for the time the note was held. Key-release noise plays on every
//! key-up.

use crate::dsp::{Limiter, Reverb, Shimmer, Smooth, Tone, soft_clip};
use crate::library::{Instrument, NONE};
use crate::metronome::{MetroControls, Metronome};
use crate::params::{InstrumentKind, Params, now_us, volume_gain};
use crate::strings::{StringControls, StringMachine};
use crate::synth::{MonoControls, MonoSynth};
use crate::sfz::RegionKind;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    NoteOn { key: u8, vel: u8 },
    NoteOff { key: u8 },
    /// 0..1, continuous (half pedal supported)
    Sustain(f32),
    AllNotesOff,
    /// Immediately silence everything (with a short fade)
    Panic,
}

pub const MAX_VOICES: usize = 192;
/// Above this many voices, new notes fade out the least important ones.
pub const SOFT_VOICE_LIMIT: usize = 160;
const BLOCK: usize = 64;

/// Output calibration. Chosen so that a medium-velocity (64) C-major triad in
/// the default Light mode, default volume and room, peaks around -6 dBFS.
/// Measured by `analysis::level_report` (see the render test).
pub const CALIBRATION_DB: f32 = -6.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Note,
    OneShot, // release samples and pedal noise: play to the end
}

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    kind: Kind,
    key: u8,
    sample: u32,
    pos: u32,
    frac: f32,
    inc: f32,
    gain: f32,
    env: f32,
    /// Fast fade (steal / retrigger / panic): multiplier per sample, <1 when fading.
    fade_mul: f32,
    fading: bool,
    key_down: bool,
    /// dB per second when dampers are fully on the string
    damper_rate_db: f32,
}

impl Voice {
    const EMPTY: Voice = Voice {
        active: false,
        kind: Kind::Note,
        key: 0,
        sample: 0,
        pos: 0,
        frac: 0.0,
        inc: 1.0,
        gain: 0.0,
        env: 0.0,
        fade_mul: 1.0,
        fading: false,
        key_down: false,
        damper_rate_db: 0.0,
    };
}

#[derive(Clone, Copy, Default)]
struct KeyState {
    down: bool,
    /// key is up but its dampers are still lifted by the pedal; release samples pending
    pending_release: bool,
    vel: u8,
    on_time: u64,
}

pub struct Engine {
    inst: Arc<Instrument>,
    params: Arc<Params>,
    inputs: Vec<rtrb::Consumer<Event>>,
    sr: f32,
    voices: Box<[Voice; MAX_VOICES]>,
    keys: [KeyState; 128],
    pedal: f32,
    pedal_target: f32,
    clock: u64,
    rng: u32,
    tone: Tone,
    shimmer: Shimmer,
    reverb: Reverb,
    limiter: Limiter,
    vol: Smooth,
    rev_amt: Smooth,
    mod_amt: Smooth,
    tone_amt: Smooth,
    dry: Box<[f32; BLOCK * 2]>,
    cal: f32,
    peak_hold: f32,
    // ---- synths and metronome
    current: InstrumentKind,
    mono: Box<MonoSynth>,
    strings: Box<StringMachine>,
    synth_down: [bool; 128],
    metro: Metronome,
    /// piano pitch-bend playback-rate multiplier at the end of the last block
    piano_bend_mul: f32,
}

impl Engine {
    pub fn new(inst: Arc<Instrument>, params: Arc<Params>, sample_rate: u32) -> Engine {
        let sr = sample_rate as f32;
        let mut e = Engine {
            inst,
            vol: Smooth::new(volume_gain(params.volume.get()), sr, 30.0),
            rev_amt: Smooth::new(params.reverb.get(), sr, 50.0),
            mod_amt: Smooth::new(params.modulation.get(), sr, 50.0),
            tone_amt: Smooth::new(params.tone.get(), sr / BLOCK as f32, 40.0),
            params,
            inputs: Vec::new(),
            sr,
            voices: Box::new([Voice::EMPTY; MAX_VOICES]),
            keys: [KeyState::default(); 128],
            pedal: 0.0,
            pedal_target: 0.0,
            clock: 0,
            rng: 0x9E3779B9,
            tone: Tone::new(sr),
            shimmer: Shimmer::new(sr),
            reverb: Reverb::new(sr),
            limiter: Limiter::new(sr),
            dry: Box::new([0.0; BLOCK * 2]),
            cal: 10f32.powf(CALIBRATION_DB / 20.0),
            peak_hold: 0.0,
            current: InstrumentKind::GrandPiano,
            mono: Box::new(MonoSynth::new(sr)),
            strings: Box::new(StringMachine::new(sr)),
            synth_down: [false; 128],
            metro: Metronome::new(sample_rate),
            piano_bend_mul: 1.0,
        };
        e.current = e.params.instrument();
        e.mono.set_preset(e.params.preset_of(InstrumentKind::MonoSynth));
        e.mono.set_voice_mode(e.params.voice_mode());
        e.mono.set_priority(e.params.note_priority());
        e.strings.set_preset(e.params.preset_of(InstrumentKind::StringMachine));
        e
    }

    pub fn instrument(&self) -> InstrumentKind {
        self.current
    }

    /// Follows instrument and preset changes made by the UI or MIDI. Notes on
    /// the old instrument are released normally (they ring out; no cut-off).
    fn sync_instrument(&mut self) {
        let inst = self.params.instrument();
        if inst != self.current {
            match self.current {
                InstrumentKind::GrandPiano => {
                    for k in 0..128u8 {
                        if self.keys[k as usize].down {
                            self.note_off(k);
                        }
                    }
                }
                _ => self.synth_release_all(),
            }
            self.current = inst;
        }
        let mp = self.params.preset_of(InstrumentKind::MonoSynth);
        if mp != self.mono.preset() {
            self.mono.set_preset(mp); // releases its notes
        }
        self.sync_voice_mode();
        let sp = self.params.preset_of(InstrumentKind::StringMachine);
        if sp != self.strings.preset() {
            self.strings.release_all();
            self.strings.set_preset(sp);
        }
    }

    /// Follows the Voices switch and note priority (a switch releases the
    /// synth's notes; they ring out).
    fn sync_voice_mode(&mut self) {
        self.mono.set_voice_mode(self.params.voice_mode());
        self.mono.set_priority(self.params.note_priority());
    }

    fn synth_release_all(&mut self) {
        self.mono.release_all();
        self.strings.release_all();
        for k in 0..128u8 {
            if self.synth_down[k as usize] {
                self.synth_down[k as usize] = false;
                Params::key_bit(&self.params.keys_down, k, false);
            }
        }
    }

    fn synth_note_on(&mut self, key: u8, raw_vel: u8) {
        let vel = self.params.shape(raw_vel);
        self.params.last_velocity_in.store(raw_vel, Ordering::Relaxed);
        self.params.last_velocity_out.store(vel, Ordering::Relaxed);
        match self.current {
            InstrumentKind::MonoSynth => {
                self.sync_voice_mode();
                self.mono.note_on(key, vel, self.params.glide.get())
            }
            InstrumentKind::StringMachine => self.strings.note_on(key, vel),
            InstrumentKind::GrandPiano => return,
        }
        self.synth_down[key as usize] = true;
        Params::key_bit(&self.params.keys_down, key, true);
    }

    fn synth_note_off(&mut self, key: u8) {
        if !self.synth_down[key as usize] {
            return;
        }
        self.synth_down[key as usize] = false;
        self.mono.note_off(key);
        self.strings.note_off(key);
        if !self.keys[key as usize].down {
            Params::key_bit(&self.params.keys_down, key, false);
        }
    }

    pub fn params(&self) -> &Arc<Params> {
        &self.params
    }

    pub fn sample_rate(&self) -> u32 {
        self.sr as u32
    }

    /// Adds an event source (one per producer thread: MIDI, UI, ...).
    pub fn add_input(&mut self, c: rtrb::Consumer<Event>) {
        self.inputs.push(c);
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count() + self.mono.active_voices() + self.strings.active_voices()
    }

    fn rand(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Applies one event immediately (also used by tests and the offline renderer).
    pub fn handle(&mut self, ev: Event) {
        if self.params.instrument() != self.current {
            self.sync_instrument();
        }
        match ev {
            Event::NoteOn { key, vel } if vel > 0 => {
                let key = key & 127;
                if self.current == InstrumentKind::GrandPiano {
                    self.note_on(key, vel)
                } else {
                    self.synth_note_on(key, vel)
                }
            }
            Event::NoteOn { key, .. } | Event::NoteOff { key } => {
                let key = key & 127;
                self.note_off(key);
                self.synth_note_off(key);
            }
            Event::Sustain(v) => {
                let v = v.clamp(0.0, 1.0);
                self.sustain(v);
                self.mono.sustain(v >= 0.5);
                self.strings.sustain(v >= 0.5);
            }
            Event::AllNotesOff => {
                for k in 0..128u8 {
                    if self.keys[k as usize].down {
                        self.note_off(k);
                    }
                }
                self.synth_release_all();
            }
            Event::Panic => {
                self.mono.panic();
                self.strings.panic();
                self.synth_down = [false; 128];
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    v.fading = true;
                    v.fade_mul = (-1.0 / (0.01 * self.sr)).exp();
                }
                self.keys = [KeyState::default(); 128];
                self.pedal = 0.0;
                self.pedal_target = 0.0;
            }
        }
    }

    fn note_on(&mut self, key: u8, raw_vel: u8) {
        let vel = self.params.shape(raw_vel);
        self.params.last_velocity_in.store(raw_vel, Ordering::Relaxed);
        self.params.last_velocity_out.store(vel, Ordering::Relaxed);
        let ridx = self.inst.note_table[key as usize][vel as usize];
        if ridx == NONE {
            return;
        }
        // Re-striking a key: let the previous strikes of this key ring out briefly.
        let retrig = (-1.0 / (0.12 * self.sr)).exp();
        for v in self.voices.iter_mut() {
            if v.active && v.kind == Kind::Note && v.key == key && !v.fading {
                v.fading = true;
                v.fade_mul = retrig;
            }
        }
        let ks = &mut self.keys[key as usize];
        ks.down = true;
        ks.pending_release = false;
        ks.vel = vel;
        ks.on_time = self.clock;
        self.start_region(ridx, key, vel, Kind::Note, 0.0, true);
        Params::key_bit(&self.params.keys_down, key, true);
    }

    fn note_off(&mut self, key: u8) {
        let ks = self.keys[key as usize];
        if !ks.down {
            return;
        }
        self.keys[key as usize].down = false;
        Params::key_bit(&self.params.keys_down, key, false);
        for v in self.voices.iter_mut() {
            if v.active && v.kind == Kind::Note && v.key == key {
                v.key_down = false;
            }
        }
        let held = (self.clock - ks.on_time) as f32 / self.sr;
        self.trigger_release(key, ks.vel, held, RegionKind::KeyNoise);
        if self.pedal < 0.5 {
            self.trigger_release(key, ks.vel, held, RegionKind::DamperRelease);
        } else {
            self.keys[key as usize].pending_release = true;
        }
    }

    fn sustain(&mut self, v: f32) {
        let was_down = self.pedal_target >= 0.5;
        self.pedal_target = v;
        let now_down = v >= 0.5;
        if now_down && !was_down {
            self.trigger_pedal(true);
        } else if !now_down && was_down {
            self.trigger_pedal(false);
            for k in 0..128u8 {
                let ks = self.keys[k as usize];
                if ks.pending_release && !ks.down {
                    self.keys[k as usize].pending_release = false;
                    let held = (self.clock - ks.on_time) as f32 / self.sr;
                    self.trigger_release(k, ks.vel, held, RegionKind::DamperRelease);
                }
            }
        }
    }

    fn trigger_pedal(&mut self, down: bool) {
        let inst = self.inst.clone();
        let list = if down { &inst.pedal_down } else { &inst.pedal_up };
        if list.is_empty() {
            return;
        }
        let r = self.rand();
        let pick = list
            .iter()
            .copied()
            .find(|&i| {
                let reg = &inst.regions[i as usize].region;
                r >= reg.lorand && r < reg.hirand
            })
            .unwrap_or(list[0]);
        let key = inst.regions[pick as usize].region.keycenter;
        self.start_region(pick, key, 127, Kind::OneShot, 0.0, false);
    }

    fn trigger_release(&mut self, key: u8, vel: u8, held_secs: f32, kind: RegionKind) {
        let n = self.inst.release_table[key as usize].len();
        for i in 0..n {
            let ridx = self.inst.release_table[key as usize][i];
            let reg = &self.inst.regions[ridx as usize].region;
            if reg.kind != kind || vel < reg.lovel || vel > reg.hivel {
                continue;
            }
            let atten = -reg.rt_decay * held_secs;
            if atten < -60.0 {
                continue;
            }
            self.start_region(ridx, key, vel, Kind::OneShot, atten, false);
        }
    }

    fn start_region(&mut self, ridx: u16, key: u8, vel: u8, kind: Kind, extra_db: f32, key_down: bool) {
        let inst = self.inst.clone();
        let ri = &inst.regions[ridx as usize];
        let reg = &ri.region;
        let s = &inst.samples[ri.sample];
        let semis = (key as f32 - reg.keycenter as f32) * reg.pitch_keytrack;
        let inc = 2f32.powf(semis / 12.0) * s.rate as f32 / self.sr;
        let vt = reg.amp_veltrack;
        let vn = vel as f32 / 127.0;
        let vel_gain = (1.0 - vt) + vt * vn * vn;
        let gain = self.cal * s.scale * vel_gain * 10f32.powf((reg.volume_db + extra_db) / 20.0);
        let damper_rate_db = if reg.ampeg_release > 2.0 { 60.0 / reg.ampeg_release } else { 75.0 / reg.ampeg_release.max(0.05) };
        let slot = self.alloc_voice();
        self.voices[slot] = Voice {
            active: true,
            kind,
            key,
            sample: ri.sample as u32,
            pos: 0,
            frac: 0.0,
            inc,
            gain,
            env: 1.0,
            fade_mul: 1.0,
            fading: false,
            key_down,
            damper_rate_db,
        };
    }

    /// Voice priority for stealing: lower = steal first.
    fn importance(v: &Voice) -> f32 {
        let base = match (v.kind, v.key_down) {
            (Kind::OneShot, _) => 0.0,
            (Kind::Note, false) => 1.0,
            (Kind::Note, true) => 2.0,
        };
        base + v.env.min(0.999)
    }

    fn alloc_voice(&mut self) -> usize {
        let active = self.voices.iter().filter(|v| v.active && !v.fading).count();
        if active >= SOFT_VOICE_LIMIT {
            // fade out the least important non-fading voice (5 ms)
            if let Some(i) = self
                .voices
                .iter()
                .enumerate()
                .filter(|(_, v)| v.active && !v.fading)
                .min_by(|a, b| Self::importance(a.1).total_cmp(&Self::importance(b.1)))
                .map(|(i, _)| i)
            {
                self.voices[i].fading = true;
                self.voices[i].fade_mul = (-1.0 / (0.005 * self.sr)).exp();
            }
        }
        if let Some(i) = self.voices.iter().position(|v| !v.active) {
            return i;
        }
        // Hard limit: reuse the quietest fading voice, else the least important.
        self.voices
            .iter()
            .enumerate()
            .min_by(|a, b| {
                let ka = if a.1.fading { a.1.env - 10.0 } else { Self::importance(a.1) };
                let kb = if b.1.fading { b.1.env - 10.0 } else { Self::importance(b.1) };
                ka.total_cmp(&kb)
            })
            .map(|(i, _)| i)
            .unwrap()
    }

    fn drain_events(&mut self) {
        let mut buf: [Option<Event>; 256] = [None; 256];
        let mut n = 0;
        for c in self.inputs.iter_mut() {
            while n < buf.len() {
                match c.pop() {
                    Ok(e) => {
                        buf[n] = Some(e);
                        n += 1;
                    }
                    Err(_) => break,
                }
            }
        }
        for e in buf.iter().take(n).flatten() {
            self.handle(*e);
        }
    }

    /// Renders interleaved stereo into `out` (len = frames * 2).
    pub fn process(&mut self, out: &mut [f32]) {
        self.sync_instrument();
        self.drain_events();
        let block_start_us = now_us();
        let synth = self.current.is_synth();
        let frames = out.len() / 2;
        let mut done = 0;
        let mut peak = 0f32;
        let mut min_gain = 1f32;
        while done < frames {
            let n = (frames - done).min(BLOCK);
            self.render_block(n);
            let vol_t = volume_gain(self.params.volume.get());
            let rev_t = self.params.reverb.get();
            // The piano's tone filter and shimmer stay neutral on the synths:
            // there Tone is the filter cutoff and the wheel is vibrato.
            let mod_t = if synth { 0.0 } else { self.params.modulation.get() };
            let tone = self.tone_amt.next(if synth { 0.5 } else { self.params.tone.get() });
            self.tone.update(tone);
            for i in 0..n {
                let (l, r) = self.tone.tick(self.dry[i * 2], self.dry[i * 2 + 1]);
                let m = self.mod_amt.next(mod_t);
                let (l, r) = self.shimmer.tick(l, r, m);
                let (wl, wr) = self.reverb.tick(l, r);
                let rv = self.rev_amt.next(rev_t);
                let wet = rv * 1.1;
                let dry = 1.0 - 0.25 * rv;
                let g = self.vol.next(vol_t);
                let (ol, or, lg) = self.limiter.tick((l * dry + wl * wet) * g, (r * dry + wr * wet) * g);
                min_gain = min_gain.min(lg);
                peak = peak.max(ol.abs()).max(or.abs());
                out[(done + i) * 2] = ol;
                out[(done + i) * 2 + 1] = or;
            }
            // metronome: mixed after the effects and the limiter
            let mc = MetroControls {
                running: self.params.metro_running.load(Ordering::Relaxed),
                bpm: self.params.tempo_bpm(),
                beats_per_bar: self.params.beats_per_bar.load(Ordering::Relaxed),
                click_on: self.params.click_on.load(Ordering::Relaxed),
                level: self.params.click_level.get(),
            };
            if mc.running || self.metro.busy() {
                let mut beat: Option<(usize, u8)> = None;
                let seg = &mut out[done * 2..(done + n) * 2];
                if self.metro.process(seg, n, mc, |off, b| beat = Some((off, b))) {
                    // keep instrument + click from clipping (identity below 0.8)
                    for x in seg.iter_mut() {
                        *x = soft_clip(*x);
                        peak = peak.max(x.abs());
                    }
                }
                if let Some((off, b)) = beat {
                    // when this beat leaves the speakers: block time + offset + output
                    // latency (minus the 1 ms limiter look-ahead the click skips)
                    let lat_ms = (self.params.output_latency_ms.get() - 1.0).max(0.0);
                    let t = block_start_us + (((done + off) as f32 / self.sr) * 1e6) as u64 + (lat_ms * 1000.0) as u64;
                    self.params.beat_in_bar.store(b, Ordering::Relaxed);
                    self.params.beat_audible_us.store(t, Ordering::Relaxed);
                    self.params.beat_seq.fetch_add(1, Ordering::Release);
                }
            }
            done += n;
        }
        // telemetry
        self.peak_hold = peak;
        self.params.out_peak.set(peak.max(self.params.out_peak.get() * 0.9));
        self.params.limiter_gr_db.set(20.0 * min_gain.log10());
        let mut sounding = [0u64; 2];
        let mut count = 0;
        for v in self.voices.iter().filter(|v| v.active) {
            count += 1;
            if v.kind == Kind::Note && v.env > 0.01 && !v.fading {
                sounding[(v.key / 64) as usize] |= 1 << (v.key % 64);
            }
        }
        self.mono.sounding(&mut sounding);
        self.strings.sounding(&mut sounding);
        count += (self.mono.active_voices() + self.strings.active_voices()) as u32;
        self.params.keys_sounding[0].store(sounding[0], Ordering::Relaxed);
        self.params.keys_sounding[1].store(sounding[1], Ordering::Relaxed);
        self.params.active_voices.store(count, Ordering::Relaxed);
    }

    fn render_block(&mut self, n: usize) {
        for x in self.dry[..n * 2].iter_mut() {
            *x = 0.0;
        }
        // pedal smoothing per block (~3 ms time constant)
        let a = 1.0 - (-(n as f32) / (0.003 * self.sr)).exp();
        self.pedal += (self.pedal_target - self.pedal) * a;
        // fraction of damper lift; half-pedal zone between 0.15 and 0.85
        let lift = ((self.pedal - 0.15) / 0.7).clamp(0.0, 1.0);
        let lift = lift * lift * (3.0 - 2.0 * lift);
        let damping = (1.0 - lift) * (1.0 - lift);
        // pitch bend on the piano (off unless allowed in Settings): playback rate,
        // interpolated across the block
        let bend_target = if self.current == InstrumentKind::GrandPiano { self.params.bend_semis(InstrumentKind::GrandPiano) } else { 0.0 };
        let b0 = self.piano_bend_mul;
        let b1 = 2f32.powf(bend_target / 12.0);
        // limit the change per block (~a semitone per 30 ms at most) so steps don't zipper
        let b1 = b0 + (b1 - b0).clamp(-0.0025 * b0, 0.0025 * b0);
        self.piano_bend_mul = if (b1 - 1.0).abs() < 1e-6 && bend_target == 0.0 { 1.0 } else { b1 };
        let bending = b0 != 1.0 || b1 != 1.0;
        let inst = &*self.inst;
        let sr = self.sr;
        let dry = &mut self.dry;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let s = &inst.samples[v.sample as usize];
            // per-block envelope multiplier
            let mut mul = 1.0f32;
            if v.fading {
                mul = v.fade_mul;
            } else if v.kind == Kind::Note && !v.key_down {
                let rate = v.damper_rate_db * damping; // dB/s
                if rate > 0.0 {
                    mul = 10f32.powf(-rate / (20.0 * sr));
                }
            }
            let mut pos = v.pos as usize;
            let mut frac = v.frac;
            let inc = v.inc;
            let mut env = v.env;
            let g = v.gain;
            let last = s.frames;
            let mut ended = false;
            for i in 0..n {
                if pos + 2 >= last + 2 {
                    ended = true;
                    break;
                }
                // 4-point cubic Hermite interpolation
                let p0 = if pos > 0 { s.frame(pos - 1) } else { s.frame(pos) };
                let p1 = s.frame(pos);
                let p2 = s.frame(pos + 1);
                let p3 = s.frame(pos + 2);
                let hl = hermite(frac, p0.0, p1.0, p2.0, p3.0);
                let hr = hermite(frac, p0.1, p1.1, p2.1, p3.1);
                let k = g * env;
                dry[i * 2] += hl * k;
                dry[i * 2 + 1] += hr * k;
                env *= mul;
                frac += if bending { inc * (b0 + (b1 - b0) * (i as f32 / n as f32)) } else { inc };
                let adv = frac as usize;
                pos += adv;
                frac -= adv as f32;
            }
            v.pos = pos as u32;
            v.frac = frac;
            v.env = env;
            if ended || env < 1e-4 || (v.fading && env < 1e-3) {
                v.active = false;
            }
        }
        // synths (they also finish releasing after a switch back to the piano)
        let p = &self.params;
        let cur = self.current;
        let bend_for = |i: InstrumentKind| if cur == i { p.bend_semis(i) } else { 0.0 };
        let mc = MonoControls {
            cutoff: p.synth_cutoff[1].get(),
            resonance: p.resonance.get(),
            glide: p.glide.get(),
            vibrato: p.vibrato.get(),
            bend: bend_for(InstrumentKind::MonoSynth),
        };
        self.mono.render(&mut self.dry[..n * 2], n, mc);
        let sc = StringControls { cutoff: p.synth_cutoff[2].get(), vibrato: p.vibrato.get(), bend: bend_for(InstrumentKind::StringMachine) };
        self.strings.render(&mut self.dry[..n * 2], n, sc);
        self.clock += n as u64;
    }

    /// Offline helper: render `frames` frames.
    pub fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        for chunk in out.chunks_mut(256) {
            self.process(chunk);
        }
        out
    }

    #[cfg(test)]
    fn voices_for_key(&self, key: u8) -> Vec<Voice> {
        self.voices.iter().filter(|v| v.active && v.key == key).copied().collect()
    }
}

#[inline(always)]
fn hermite(t: f32, y0: f32, y1: f32, y2: f32, y3: f32) -> f32 {
    let c0 = y1;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + c0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::testlib;
    use crate::params::VelocityMode;

    fn engine() -> Engine {
        let params = Arc::new(Params::default());
        params.set_mode(VelocityMode::Normal);
        params.reverb.set(0.0);
        Engine::new(Arc::new(testlib::instrument()), params, 48000)
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|x| x * x).sum::<f32>() / buf.len().max(1) as f32).sqrt()
    }

    #[test]
    fn velocity_selects_layer() {
        let mut e = engine();
        e.handle(Event::NoteOn { key: 60, vel: 20 });
        assert_eq!(e.voices_for_key(60)[0].sample, 0);
        e.handle(Event::NoteOn { key: 62, vel: 64 });
        assert_eq!(e.voices_for_key(62)[0].sample, 1);
        e.handle(Event::NoteOn { key: 64, vel: 120 });
        assert_eq!(e.voices_for_key(64)[0].sample, 2);
    }

    #[test]
    fn light_mode_selects_louder_layer_for_soft_playing() {
        let mut e = engine();
        e.params.set_mode(VelocityMode::Light);
        e.handle(Event::NoteOn { key: 60, vel: 30 });
        assert_ne!(e.voices_for_key(60)[0].sample, 0);
    }

    #[test]
    fn note_off_without_pedal_damps_and_triggers_release() {
        let mut e = engine();
        e.handle(Event::NoteOn { key: 60, vel: 100 });
        e.render(4800);
        e.handle(Event::NoteOff { key: 60 });
        let v = e.voices_for_key(60);
        // note voice + damper resonance + key noise
        assert_eq!(v.iter().filter(|v| v.kind == Kind::OneShot).count(), 2);
        let out = e.render(48000 * 2);
        // note decays well below its start within 2 s (dampers on)
        assert!(rms(&out[out.len() - 9600..]) < 1e-3);
    }

    #[test]
    fn sustain_pedal_holds_notes_and_defers_damper_release() {
        let mut e = engine();
        e.handle(Event::Sustain(1.0));
        e.handle(Event::NoteOn { key: 60, vel: 100 });
        e.render(4800);
        e.handle(Event::NoteOff { key: 60 });
        let v = e.voices_for_key(60);
        // only key noise now, damper resonance waits for the pedal
        assert_eq!(v.iter().filter(|v| v.kind == Kind::OneShot).count(), 1);
        e.render(24000);
        let note = e.voices_for_key(60).into_iter().find(|v| v.kind == Kind::Note).unwrap();
        // with the pedal down the only decay is the sample's own; env untouched
        assert!(note.env > 0.99);
        e.handle(Event::Sustain(0.0));
        e.render(480);
        let oneshots = e.voices_for_key(60).into_iter().filter(|v| v.kind == Kind::OneShot).count();
        assert!(oneshots >= 1);
        e.render(48000 * 2);
        assert!(e.voices_for_key(60).iter().all(|v| v.kind != Kind::Note));
    }

    #[test]
    fn half_pedal_damps_partially() {
        let mut full = engine();
        let mut half = engine();
        let mut none = engine();
        for (e, p) in [(&mut full, 1.0), (&mut half, 0.5), (&mut none, 0.0)] {
            e.handle(Event::Sustain(p));
            e.handle(Event::NoteOn { key: 60, vel: 100 });
            e.render(480);
            e.handle(Event::NoteOff { key: 60 });
            e.render(24000);
        }
        let env = |e: &Engine| e.voices.iter().find(|v| v.active && v.kind == Kind::Note).map(|v| v.env).unwrap_or(0.0);
        assert!(env(&full) > env(&half), "{} {}", env(&full), env(&half));
        assert!(env(&half) > env(&none));
    }

    #[test]
    fn voice_stealing_respects_limits() {
        let mut e = engine();
        for i in 0..400u32 {
            let key = 30 + (i % 60) as u8;
            e.handle(Event::NoteOn { key, vel: 100 });
            e.handle(Event::NoteOff { key });
            e.render(64);
            assert!(e.active_voices() <= MAX_VOICES);
        }
        // held notes are kept in preference to released ones
        let mut e = engine();
        e.handle(Event::NoteOn { key: 40, vel: 100 });
        for i in 0..300u32 {
            let key = 50 + (i % 30) as u8;
            e.handle(Event::NoteOn { key, vel: 100 });
            e.handle(Event::NoteOff { key });
        }
        assert!(e.voices_for_key(40).iter().any(|v| v.kind == Kind::Note && !v.fading));
    }

    #[test]
    fn retrigger_fades_previous_strike() {
        let mut e = engine();
        e.handle(Event::NoteOn { key: 60, vel: 100 });
        e.render(480);
        e.handle(Event::NoteOn { key: 60, vel: 100 });
        let v = e.voices_for_key(60);
        assert_eq!(v.iter().filter(|v| v.fading).count(), 1);
        assert_eq!(v.iter().filter(|v| !v.fading && v.kind == Kind::Note).count(), 1);
    }

    #[test]
    fn panic_silences() {
        let mut e = engine();
        for k in 40..80 {
            e.handle(Event::NoteOn { key: k, vel: 127 });
        }
        e.render(480);
        e.handle(Event::Panic);
        e.render(48000);
        assert_eq!(e.active_voices(), 0);
    }

    #[test]
    fn fast_volume_fader_moves_dont_zipper() {
        // a held string chord while the volume fader is thrown 0 -> 127 -> 0
        // in about 0.3 s each way, one CC step per 64 samples
        let params = Arc::new(Params::default());
        params.reverb.set(0.0);
        params.set_instrument(crate::params::InstrumentKind::StringMachine);
        let mut e = Engine::new(Arc::new(testlib::instrument()), params, 48000);
        for k in [60, 64, 67] {
            e.handle(Event::NoteOn { key: k, vel: 100 });
        }
        e.render(9600);
        let mut out = Vec::new();
        for cc in (0..=127u32).chain((0..=127).rev()) {
            e.params.volume.set(cc as f32 / 127.0);
            out.extend(e.render(64));
        }
        let st = crate::analysis::synth_stats(&out, 48000);
        assert_eq!(st.clicks, 0, "worst {}", st.worst_click_ratio);
        assert!(out.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn events_from_queue() {
        let mut e = engine();
        let (mut p, c) = rtrb::RingBuffer::new(64);
        e.add_input(c);
        p.push(Event::NoteOn { key: 60, vel: 90 }).unwrap();
        let out = e.render(4800);
        assert!(rms(&out) > 1e-3);
        assert!(Params::key_is(&e.params.keys_down, 60));
    }

    /// Regression for "low notes cancel out higher ones": on the Moog-style
    /// synth in Poly (the default now, on every preset), a low note added
    /// under a held chord must not cut, retrigger or duck the held notes.
    #[test]
    fn synth_low_note_does_not_cut_held_chord() {
        use crate::synth::VoiceMode;
        for preset in 0..3 {
            let row = crate::analysis::chord_check(Arc::new(testlib::instrument()), 48000, preset);
            assert!(row.ok(), "preset {preset}: {row:?}");
        }
        // and the engine follows the Voices switch: Mono plays one note
        let mut e = engine();
        e.params.set_instrument(InstrumentKind::MonoSynth);
        for k in [60u8, 64, 67, 36] {
            e.handle(Event::NoteOn { key: k, vel: 100 });
        }
        assert_eq!(e.mono.active_voices(), 4);
        for k in [60u8, 64, 67, 36] {
            e.handle(Event::NoteOff { key: k });
        }
        e.render(48000 * 2);
        e.params.set_voice_mode(VoiceMode::Mono);
        for k in [60u8, 64, 67, 36] {
            e.handle(Event::NoteOn { key: k, vel: 100 });
        }
        assert_eq!(e.mono.active_voices(), 1);
        assert_eq!(e.mono.mono_pitch(), Some(36.0), "last-note priority by default");
    }

}
