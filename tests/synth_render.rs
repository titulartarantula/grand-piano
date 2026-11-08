//! Offline render of the synth instruments (no samples needed): levels at
//! velocities 40/64/100, clipping, clicks, NaN/denormals, and that notes stop.
//! Writes target/render-<instrument>-<preset>.wav and target/synth-report.txt.

use grand_piano::analysis;

#[test]
fn render_synths() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
    let _ = std::fs::create_dir_all(&dir);
    let (report, ok) = analysis::synth_check(48000, Some(&dir));
    println!("{report}");
    std::fs::write(dir.join("synth-report.txt"), &report).unwrap();
    assert!(ok, "{report}");
}
