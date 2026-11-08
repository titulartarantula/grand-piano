//! Sample library: loads an SFZ instrument and its FLAC/WAV samples into memory
//! and builds fast lookup tables for the audio thread.

use crate::sfz::{self, Region, RegionKind};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One decoded sample. Stored as normalised 16-bit stereo (each file is scaled to
/// full scale before quantising, so quiet velocity layers keep their resolution);
/// `scale` turns the stored values back into the original level.
pub struct Sample {
    pub name: String,
    pub data: Vec<i16>, // interleaved L,R; padded with 4 silent frames
    pub frames: usize,
    pub rate: u32,
    pub scale: f32,
}

impl Sample {
    #[inline(always)]
    pub fn frame(&self, i: usize) -> (f32, f32) {
        let j = i * 2;
        (self.data[j] as f32, self.data[j + 1] as f32)
    }

    pub fn from_f32_stereo(name: &str, rate: u32, left: &[f32], right: &[f32]) -> Sample {
        let frames = left.len();
        let peak = left
            .iter()
            .chain(right.iter())
            .fold(0f32, |m, v| m.max(v.abs()))
            .max(1e-9);
        let q = 32767.0 / peak;
        let mut data = Vec::with_capacity((frames + 4) * 2);
        for i in 0..frames {
            data.push((left[i] * q).round() as i16);
            data.push((right[i] * q).round() as i16);
        }
        data.extend_from_slice(&[0; 8]);
        Sample { name: name.to_string(), data, frames, rate, scale: peak / 32767.0 }
    }
}

#[derive(Clone, Debug)]
pub struct RegionInfo {
    pub region: Region,
    pub sample: usize,
}

pub struct Instrument {
    pub name: String,
    pub samples: Vec<Sample>,
    pub regions: Vec<RegionInfo>,
    /// note_table[key][vel] = region index of the note sample to play
    pub note_table: Vec<[u16; 128]>,
    /// release regions (damper resonance + key noise) per key
    pub release_table: Vec<Vec<u16>>,
    pub pedal_down: Vec<u16>,
    pub pedal_up: Vec<u16>,
    pub memory_bytes: usize,
}

pub const NONE: u16 = u16::MAX;

impl Instrument {
    pub fn build(name: &str, samples: Vec<Sample>, regions: Vec<RegionInfo>) -> Instrument {
        let mut note_table = vec![[NONE; 128]; 128];
        let mut release_table = vec![Vec::new(); 128];
        let mut pedal_down = Vec::new();
        let mut pedal_up = Vec::new();
        for (idx, ri) in regions.iter().enumerate() {
            let r = &ri.region;
            match r.kind {
                RegionKind::Note => {
                    for k in r.lokey..=r.hikey {
                        for v in r.lovel..=r.hivel {
                            if note_table[k as usize][v as usize] == NONE {
                                note_table[k as usize][v as usize] = idx as u16;
                            }
                        }
                    }
                }
                RegionKind::DamperRelease | RegionKind::KeyNoise => {
                    for k in r.lokey..=r.hikey {
                        release_table[k as usize].push(idx as u16);
                    }
                }
                RegionKind::PedalDown => pedal_down.push(idx as u16),
                RegionKind::PedalUp => pedal_up.push(idx as u16),
            }
        }
        // velocity 0 is never used for note-on, mirror velocity 1
        for row in note_table.iter_mut() {
            row[0] = row[1];
        }
        let memory_bytes = samples.iter().map(|s| s.data.len() * 2).sum();
        Instrument { name: name.to_string(), samples, regions, note_table, release_table, pedal_down, pedal_up, memory_bytes }
    }

    /// Lowest and highest key that has a note sample.
    pub fn key_range(&self) -> (u8, u8) {
        let mut lo = 127;
        let mut hi = 0;
        for k in 0..128 {
            if self.note_table[k][100] != NONE {
                lo = lo.min(k as u8);
                hi = hi.max(k as u8);
            }
        }
        (lo, hi)
    }

    /// Number of distinct velocity layers for a key.
    pub fn velocity_layers(&self, key: u8) -> usize {
        let mut v: Vec<u16> = self.note_table[key as usize][1..].to_vec();
        v.dedup();
        v.retain(|x| *x != NONE);
        v.len()
    }
}

/// Progress of a load: (done, total) sample files.
#[derive(Default)]
pub struct LoadProgress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
}

fn decode_flac(path: &Path) -> anyhow::Result<(u32, Vec<f32>, Vec<f32>)> {
    let mut reader = claxon::FlacReader::open(path)
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let info = reader.streaminfo();
    let ch = info.channels as usize;
    let norm = 1.0 / (1i64 << (info.bits_per_sample - 1)) as f32;
    let cap = info.samples.unwrap_or(0) as usize;
    let mut l = Vec::with_capacity(cap);
    let mut r = Vec::with_capacity(cap);
    let mut i = 0usize;
    for s in reader.samples() {
        let v = s.map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))? as f32 * norm;
        match (i % ch, ch) {
            (0, 1) => {
                l.push(v);
                r.push(v);
            }
            (0, _) => l.push(v),
            (1, _) => r.push(v),
            _ => {}
        }
        i += 1;
    }
    Ok((info.sample_rate, l, r))
}

fn decode_wav(path: &Path) -> anyhow::Result<(u32, Vec<f32>, Vec<f32>)> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let ch = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let norm = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 * norm)).collect::<Result<_, _>>()?
        }
    };
    let mut l = Vec::with_capacity(samples.len() / ch);
    let mut r = Vec::with_capacity(samples.len() / ch);
    for f in samples.chunks(ch) {
        l.push(f[0]);
        r.push(if ch > 1 { f[1] } else { f[0] });
    }
    Ok((spec.sample_rate, l, r))
}

/// Trim trailing near-silence so long decays don't waste memory.
fn trim_tail(l: &mut Vec<f32>, r: &mut Vec<f32>, rate: u32) {
    let thr = 10f32.powf(-96.0 / 20.0);
    let mut end = l.len();
    while end > 0 && l[end - 1].abs() < thr && r[end - 1].abs() < thr {
        end -= 1;
    }
    // keep 10 ms after the last audible sample
    let end = (end + rate as usize / 100).min(l.len());
    l.truncate(end);
    r.truncate(end);
}

pub fn load_sfz(sfz_path: &Path, progress: Option<Arc<LoadProgress>>) -> anyhow::Result<Instrument> {
    let text = std::fs::read_to_string(sfz_path)?;
    let regions = sfz::parse(&text)?;
    let base = sfz_path.parent().unwrap_or(Path::new("."));
    let mut files: Vec<String> = regions.iter().map(|r| r.sample.clone()).collect();
    files.sort();
    files.dedup();
    if let Some(p) = &progress {
        p.total.store(files.len(), Ordering::Relaxed);
    }
    let samples: Vec<Sample> = files
        .par_iter()
        .map(|f| -> anyhow::Result<Sample> {
            let path: PathBuf = base.join(f);
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            let (rate, mut l, mut r) = if ext == "flac" { decode_flac(&path)? } else { decode_wav(&path)? };
            trim_tail(&mut l, &mut r, rate);
            let s = Sample::from_f32_stereo(f, rate, &l, &r);
            if let Some(p) = &progress {
                p.done.fetch_add(1, Ordering::Relaxed);
            }
            Ok(s)
        })
        .collect::<anyhow::Result<_>>()?;
    let infos = regions
        .into_iter()
        .map(|r| {
            let idx = files.binary_search(&r.sample).expect("sample listed");
            RegionInfo { region: r, sample: idx }
        })
        .collect();
    let name = sfz_path.file_stem().and_then(|s| s.to_str()).unwrap_or("piano").to_string();
    Ok(Instrument::build(&name, samples, infos))
}

#[cfg(test)]
pub mod testlib {
    //! A tiny synthetic instrument for unit tests: sine "piano" notes with
    //! three velocity layers at different levels, damper releases and key noise.
    use super::*;
    use crate::sfz::Trigger;

    pub fn region(sample: &str, lokey: u8, hikey: u8, lovel: u8, hivel: u8, center: u8, kind: RegionKind) -> Region {
        Region {
            sample: sample.into(),
            lokey,
            hikey,
            lovel,
            hivel,
            keycenter: center,
            pitch_keytrack: if kind == RegionKind::KeyNoise { 0.0 } else { 1.0 },
            volume_db: 0.0,
            amp_veltrack: 0.73,
            ampeg_release: 1.0,
            rt_decay: 0.0,
            trigger: if matches!(kind, RegionKind::DamperRelease | RegionKind::KeyNoise) { Trigger::Release } else { Trigger::Attack },
            lorand: 0.0,
            hirand: 1.0,
            kind,
        }
    }

    pub fn sine(name: &str, freq: f32, amp: f32, secs: f32, rate: u32) -> Sample {
        let n = (secs * rate as f32) as usize;
        let l: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                amp * (std::f32::consts::TAU * freq * t).sin() * (-t * 0.8).exp()
            })
            .collect();
        Sample::from_f32_stereo(name, rate, &l, &l)
    }

    pub fn instrument() -> Instrument {
        let rate = 48000;
        let samples = vec![
            sine("soft", 261.63, 0.1, 4.0, rate),
            sine("mid", 261.63, 0.3, 4.0, rate),
            sine("loud", 261.63, 0.8, 4.0, rate),
            sine("harm", 261.63, 0.05, 1.0, rate),
            sine("noise", 2000.0, 0.02, 0.2, rate),
        ];
        let regions = vec![
            RegionInfo { region: region("soft", 0, 127, 1, 40, 60, RegionKind::Note), sample: 0 },
            RegionInfo { region: region("mid", 0, 127, 41, 90, 60, RegionKind::Note), sample: 1 },
            RegionInfo { region: region("loud", 0, 127, 91, 127, 60, RegionKind::Note), sample: 2 },
            RegionInfo { region: region("harm", 0, 88, 1, 127, 60, RegionKind::DamperRelease), sample: 3 },
            RegionInfo { region: region("noise", 0, 127, 1, 127, 60, RegionKind::KeyNoise), sample: 4 },
        ];
        Instrument::build("test", samples, regions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velocity_layer_table() {
        let inst = testlib::instrument();
        assert_eq!(inst.note_table[60][1], 0);
        assert_eq!(inst.note_table[60][40], 0);
        assert_eq!(inst.note_table[60][41], 1);
        assert_eq!(inst.note_table[60][90], 1);
        assert_eq!(inst.note_table[60][91], 2);
        assert_eq!(inst.note_table[60][127], 2);
        assert_eq!(inst.velocity_layers(60), 3);
        assert_eq!(inst.release_table[60].len(), 2);
        assert_eq!(inst.release_table[100].len(), 1); // no damper resonance above 88
    }

    #[test]
    fn normalised_storage_keeps_level() {
        let s = testlib::sine("x", 100.0, 0.1, 0.1, 48000);
        let peak = (0..s.frames).map(|i| s.frame(i).0.abs() * s.scale).fold(0f32, f32::max);
        assert!((peak - 0.1).abs() < 0.002, "{peak}");
        assert!(s.data.iter().any(|v| v.abs() > 32000)); // full-scale storage
    }
}
