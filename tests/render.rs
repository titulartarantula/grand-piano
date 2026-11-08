//! Offline render with the real Salamander samples. Set GP_SFZ to the .sfz path
//! (the test is skipped when it isn't set). Writes target/render-test.wav.

use grand_piano::{analysis, library};
use std::sync::Arc;

#[test]
fn render_real_piano() {
    let Ok(sfz) = std::env::var("GP_SFZ") else {
        eprintln!("GP_SFZ not set; skipping the real-sample render test");
        return;
    };
    let t = std::time::Instant::now();
    let inst = Arc::new(library::load_sfz(std::path::Path::new(&sfz), None).expect("load samples"));
    eprintln!("loaded in {:.1} s", t.elapsed().as_secs_f32());
    assert_eq!(inst.velocity_layers(60), 16, "Salamander has 16 velocity layers");
    assert!(inst.release_table[60].len() >= 3, "release samples present");
    let wav = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/render-test.wav");
    let (report, ok) = analysis::full_check(inst, 48000, Some(&wav));
    println!("{report}");
    std::fs::write(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/render-report.txt"), &report).unwrap();
    assert!(ok, "{report}");
}
