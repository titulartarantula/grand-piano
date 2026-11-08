//! Prints the waveform around given times in a 24-bit stereo WAV (debug helper).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut r = hound::WavReader::open(&args[1]).unwrap();
    let sr = r.spec().sample_rate as f32;
    let x: Vec<f32> = r.samples::<i32>().map(|s| s.unwrap() as f32 / 8388607.0).collect();
    for t in &args[2..] {
        let t: f32 = t.parse().unwrap();
        let c = (t * sr) as usize;
        for ch in 0..2 {
            let mut best = (0f32, 0usize);
            for i in c - 400..c + 400 {
                let d = x[i * 2 + ch] - 2.0 * x[(i - 1) * 2 + ch] + x[(i - 2) * 2 + ch];
                if d.abs() > best.0 { best = (d.abs(), i); }
            }
            let i = best.1;
            let seg: Vec<String> = (i - 8..i + 8).map(|k| format!("{:.3}", x[k * 2 + ch])).collect();
            println!("t={t} ch{ch} at {:.4}s d2={:.3}: {}", i as f32 / sr, best.0, seg.join(" "));
        }
    }
}
