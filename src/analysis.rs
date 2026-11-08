//! Offline rendering and audio checks (levels, clipping, silence, clicks).

use crate::engine::{Engine, Event};
use crate::library::Instrument;
use crate::params::{Params, VelocityMode};
use std::sync::Arc;

pub fn db(x: f32) -> f32 {
    if x <= 0.0 { -200.0 } else { 20.0 * x.log10() }
}

#[derive(Debug, Clone, Copy)]
pub struct Stats {
    pub peak: f32,
    pub rms: f32,
    /// samples at or above 0.999 (digital clipping)
    pub clipped: usize,
    /// largest ratio of a sample-to-sample jump (2nd difference) to the local level
    pub worst_click_ratio: f32,
    /// number of positions flagged as clicks
    pub clicks: usize,
    /// frame positions of the first flagged clicks
    pub click_at: [usize; 8],
}

pub fn stats(buf: &[f32], sr: u32) -> Stats {
    let n = buf.len() / 2;
    let mut peak = 0f32;
    let mut sum = 0f64;
    let mut clipped = 0;
    for &x in buf {
        peak = peak.max(x.abs());
        sum += (x * x) as f64;
        if x.abs() >= 0.999 {
            clipped += 1;
        }
    }
    let rms = (sum / buf.len().max(1) as f64).sqrt() as f32;
    // click detector: per channel, second difference vs. its local RMS over ±5 ms
    let win = (sr as usize / 200).max(8);
    let mut worst = 0f32;
    let mut clicks = 0;
    let mut click_at = [0usize; 8];
    for ch in 0..2 {
        let x: Vec<f32> = (0..n).map(|i| buf[i * 2 + ch]).collect();
        let d2: Vec<f32> = (0..n)
            .map(|i| if i >= 2 { x[i] - 2.0 * x[i - 1] + x[i - 2] } else { 0.0 })
            .collect();
        // prefix sums of d2^2 for the local RMS
        let mut pre = vec![0f64; n + 1];
        for i in 0..n {
            pre[i + 1] = pre[i] + (d2[i] * d2[i]) as f64;
        }
        let mut last_click = 0usize;
        for i in 2..n {
            let a = d2[i].abs();
            if a < 0.02 {
                continue; // too small to hear
            }
            let lo = i.saturating_sub(win);
            let hi = (i + win).min(n);
            // exclude the sample itself from the local level
            let local = ((pre[hi] - pre[lo] - (a * a) as f64) / (hi - lo - 1).max(1) as f64).sqrt() as f32;
            let ratio = a / local.max(1e-6);
            worst = worst.max(ratio);
            if ratio > 12.0 && i > last_click + win {
                if clicks < 8 {
                    click_at[clicks] = i;
                }
                clicks += 1;
                last_click = i;
            }
        }
    }
    Stats { peak, rms, clipped, worst_click_ratio: worst, clicks, click_at }
}

/// Builds an engine with default settings (optionally a different velocity mode).
pub fn default_engine(inst: Arc<Instrument>, sr: u32, mode: VelocityMode) -> Engine {
    let params = Arc::new(Params::default());
    params.set_mode(mode);
    Engine::new(inst, params, sr)
}

#[derive(Debug, Clone)]
pub struct LevelRow {
    pub mode: VelocityMode,
    pub velocity: u8,
    pub shaped_velocity: u8,
    pub peak_db: f32,
    pub rms_db: f32,
}

/// Renders a C-major triad (C4-E4-G4) held for 1.5 s at each velocity and reports
/// peak and RMS over the first second, at default volume and room.
pub fn level_report(inst: Arc<Instrument>, sr: u32) -> Vec<LevelRow> {
    let mut rows = Vec::new();
    for mode in [VelocityMode::Light, VelocityMode::Normal] {
        for vel in [40u8, 64, 100] {
            let mut e = default_engine(inst.clone(), sr, mode);
            let shaped = e.params().shape(vel);
            for k in [60u8, 64, 67] {
                e.handle(Event::NoteOn { key: k, vel });
            }
            let out = e.render(sr as usize);
            let s = stats(&out, sr);
            rows.push(LevelRow { mode, velocity: vel, shaped_velocity: shaped, peak_db: db(s.peak), rms_db: db(s.rms) });
        }
    }
    rows
}

/// A test performance: scales, chords with pedal, half pedal, fast repeats,
/// a fast glissando (voice stealing), the full key range, then a long tail.
pub fn test_performance(sr: u32) -> Vec<(usize, Event)> {
    let s = |secs: f32| (secs * sr as f32) as usize;
    let mut ev = Vec::new();
    let mut t = 0.0f32;
    // 1. C major scale C3..C6, legato, rising velocity
    let scale = [0, 2, 4, 5, 7, 9, 11];
    let mut keys = Vec::new();
    for oct in 0..3 {
        for d in scale {
            keys.push(48 + oct * 12 + d);
        }
    }
    keys.push(84);
    for (i, k) in keys.iter().enumerate() {
        let vel = 30 + (i * 4) as u8;
        ev.push((s(t), Event::NoteOn { key: *k, vel }));
        ev.push((s(t + 0.2), Event::NoteOff { key: *k }));
        t += 0.18;
    }
    t += 0.5;
    // 2. chords with the sustain pedal
    for (i, chord) in [[48u8, 55, 64, 72], [45, 52, 60, 69], [41, 48, 57, 65], [43, 50, 59, 67]].iter().enumerate() {
        ev.push((s(t), Event::Sustain(0.0)));
        ev.push((s(t + 0.05), Event::Sustain(1.0)));
        for k in chord {
            ev.push((s(t + 0.02), Event::NoteOn { key: *k, vel: 70 + (i * 10) as u8 }));
            ev.push((s(t + 0.4), Event::NoteOff { key: *k }));
        }
        t += 1.2;
    }
    ev.push((s(t), Event::Sustain(0.0)));
    t += 1.0;
    // 3. half pedal
    ev.push((s(t), Event::Sustain(0.5)));
    for k in [60u8, 64, 67, 72] {
        ev.push((s(t), Event::NoteOn { key: k, vel: 90 }));
        ev.push((s(t + 0.2), Event::NoteOff { key: k }));
    }
    t += 1.5;
    ev.push((s(t), Event::Sustain(0.0)));
    t += 0.5;
    // 4. fast repeated notes (retrigger)
    for i in 0..16 {
        ev.push((s(t), Event::NoteOn { key: 67, vel: 60 + (i % 4) * 15 }));
        ev.push((s(t + 0.05), Event::NoteOff { key: 67 }));
        t += 0.08;
    }
    t += 0.5;
    // 5. glissando with pedal down over the whole keyboard: voice stealing
    ev.push((s(t), Event::Sustain(1.0)));
    for k in 21u8..=108 {
        ev.push((s(t), Event::NoteOn { key: k, vel: 110 }));
        ev.push((s(t + 0.03), Event::NoteOff { key: k }));
        t += 0.012;
    }
    t += 2.0;
    ev.push((s(t), Event::Sustain(0.0)));
    t += 1.0;
    // 6. fortissimo low cluster + high chord (loudness / limiter)
    for k in [24u8, 31, 36, 43, 48, 76, 79, 84, 88] {
        ev.push((s(t), Event::NoteOn { key: k, vel: 127 }));
        ev.push((s(t + 1.0), Event::NoteOff { key: k }));
    }
    t += 1.5;
    // 7. quiet single notes
    for k in [60u8, 62, 64] {
        ev.push((s(t), Event::NoteOn { key: k, vel: 20 }));
        ev.push((s(t + 0.4), Event::NoteOff { key: k }));
        t += 0.5;
    }
    // tail
    ev.push((s(t + 6.0), Event::AllNotesOff));
    ev.sort_by_key(|e| e.0);
    ev
}

/// Renders a timed event list.
pub fn render_events(engine: &mut Engine, events: &[(usize, Event)], extra_frames: usize) -> Vec<f32> {
    let total = events.last().map(|e| e.0).unwrap_or(0) + extra_frames;
    let mut out = vec![0f32; total * 2];
    let mut pos = 0;
    let mut i = 0;
    while pos < total {
        while i < events.len() && events[i].0 <= pos {
            engine.handle(events[i].1);
            i += 1;
        }
        let next = if i < events.len() { events[i].0.min(total) } else { total };
        let end = next.max(pos + 1).min(pos + 128).min(total);
        engine.process(&mut out[pos * 2..end * 2]);
        pos = end;
    }
    out
}

pub fn write_wav(path: &std::path::Path, buf: &[f32], sr: u32) -> anyhow::Result<()> {
    let spec = hound::WavSpec { channels: 2, sample_rate: sr, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec)?;
    for &x in buf {
        w.write_sample((x.clamp(-1.0, 1.0) * 8_388_607.0) as i32)?;
    }
    w.finalize()?;
    Ok(())
}

/// Full offline check used by the test and by `--render-test`. Returns a report
/// string and whether everything passed.
pub fn full_check(inst: Arc<Instrument>, sr: u32, wav: Option<&std::path::Path>) -> (String, bool) {
    use std::fmt::Write;
    let mut r = String::new();
    let mut ok = true;
    let _ = writeln!(r, "Instrument: {} ({} samples, {} MB in RAM)", inst.name, inst.samples.len(), inst.memory_bytes / 1_000_000);
    let (lo, hi) = inst.key_range();
    let _ = writeln!(r, "Key range {lo}..{hi}, velocity layers at C4: {}", inst.velocity_layers(60));
    let _ = writeln!(r, "\nLevels (C-major triad C4-E4-G4, first 1 s, default volume and room):");
    let _ = writeln!(r, "  mode    vel -> played   peak dBFS   RMS dBFS");
    for row in level_report(inst.clone(), sr) {
        let _ = writeln!(
            r,
            "  {:<7} {:>3} -> {:>3}      {:>7.1}    {:>7.1}",
            row.mode.label(),
            row.velocity,
            row.shaped_velocity,
            row.peak_db,
            row.rms_db
        );
        if row.mode == VelocityMode::Light && (row.rms_db < -30.0 || row.peak_db > -0.5) {
            ok = false;
            let _ = writeln!(r, "  ^ FAIL: level out of the audible/safe range");
        }
    }
    let mut e = default_engine(inst, sr, VelocityMode::Light);
    let events = test_performance(sr);
    let out = render_events(&mut e, &events, sr as usize);
    if let Some(p) = wav {
        if let Err(err) = write_wav(p, &out, sr) {
            let _ = writeln!(r, "could not write {}: {err}", p.display());
        } else {
            let _ = writeln!(r, "\nWrote {}", p.display());
        }
    }
    let st = stats(&out, sr);
    let _ = writeln!(r, "\nTest performance: {:.1} s", out.len() as f32 / 2.0 / sr as f32);
    let _ = writeln!(r, "  peak {:.2} dBFS, RMS {:.1} dBFS, clipped samples {}", db(st.peak), db(st.rms), st.clipped);
    let _ = writeln!(r, "  click detector: {} flagged, worst ratio {:.1}", st.clicks, st.worst_click_ratio);
    if st.clipped > 0 {
        ok = false;
        let _ = writeln!(r, "  FAIL: clipping");
    }
    if st.clicks > 0 {
        ok = false;
        let _ = writeln!(r, "  FAIL: clicks at {:?} s", st.click_at.iter().take(st.clicks.min(8)).map(|f| (*f as f32 / sr as f32 * 100.0).round() / 100.0).collect::<Vec<_>>());
    }
    // silence check: every 250 ms window during which a key is held must be audible
    let mut held: Vec<(usize, usize)> = Vec::new();
    for (i, (t, e)) in events.iter().enumerate() {
        if let Event::NoteOn { key, .. } = e {
            let off = events[i..]
                .iter()
                .find(|(_, e2)| matches!(e2, Event::NoteOff { key: k } if k == key))
                .map(|x| x.0)
                .unwrap_or(*t);
            held.push((*t, off));
        }
    }
    let w = sr as usize / 4;
    let mut silent = 0;
    let mut checked = 0;
    let mut i = 0;
    while (i + 1) * w < out.len() / 2 {
        let (a, b) = (i * w, (i + 1) * w);
        if held.iter().any(|(on, off)| *on <= a && *off >= b) {
            checked += 1;
            let s = stats(&out[a * 2..b * 2], sr);
            if db(s.rms) < -60.0 {
                silent += 1;
            }
        }
        i += 1;
    }
    let _ = writeln!(r, "  250 ms windows with a key held: {checked}");
    let _ = writeln!(r, "  silent 250 ms windows while playing: {silent}");
    if silent > 0 {
        ok = false;
        let _ = writeln!(r, "  FAIL: unexpected silence");
    }
    // tail: the last 0.5 s must have decayed
    let tail = stats(&out[out.len() - sr as usize..], sr);
    let _ = writeln!(r, "  tail RMS {:.1} dBFS", db(tail.rms));
    if db(tail.rms) > -50.0 {
        ok = false;
        let _ = writeln!(r, "  FAIL: notes did not decay");
    }
    let _ = writeln!(r, "\n{}", if ok { "ALL CHECKS PASSED" } else { "SOME CHECKS FAILED" });
    (r, ok)
}
