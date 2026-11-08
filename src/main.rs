#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("Grand Piano's desktop app is Windows-only (WASAPI). The engine and tests build anywhere: cargo test --no-default-features");
}

#[cfg(windows)]
mod audio_win;
#[cfg(windows)]
mod midi;
#[cfg(windows)]
mod sink;
#[cfg(windows)]
mod ui;

#[cfg(windows)]
fn main() {
    use std::io::Write;
    let args: Vec<String> = std::env::args().collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let arg_after = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let data = grand_piano::settings::data_dir();
    let _ = std::fs::create_dir_all(&data);

    // log to %LOCALAPPDATA%\GrandPiano\grandpiano.log
    if let Ok(file) = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(data.join("grandpiano.log")) {
        env_logger::Builder::new()
            .filter_level(log::LevelFilter::Info)
            .target(env_logger::Target::Pipe(Box::new(file)))
            .format(|buf, rec| writeln!(buf, "{} {:5} {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0), rec.level(), rec.args()))
            .init();
    }
    std::panic::set_hook(Box::new(|info| {
        log::error!("panic: {info}");
        let _ = std::fs::write(grand_piano::settings::data_dir().join("crash.txt"), format!("{info}\n{}", std::backtrace::Backtrace::force_capture()));
    }));

    if has("--selftest") || has("--download-samples") || has("--render-test") || has("--synth-test") || has("--help") {
        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(u32::MAX);
        }
        let code = cli::run(&args, has, arg_after);
        std::process::exit(code);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Grand Piano")
            .with_inner_size([1180.0, 700.0])
            .with_min_inner_size([960.0, 620.0])
            .with_icon(std::sync::Arc::new(ui::icon())),
        persist_window: true,
        persistence_path: Some(grand_piano::settings::config_dir().join("window.ron")),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native("Grand Piano", options, Box::new(|cc| Ok(Box::new(ui::App::new(cc))))) {
        log::error!("window: {e}");
    }
}

#[cfg(windows)]
mod cli {
    use crate::{audio_win, midi};
    use grand_piano::{analysis, download, library, settings};
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    struct Out {
        text: String,
    }
    impl Out {
        fn line(&mut self, s: impl AsRef<str>) {
            println!("{}", s.as_ref());
            self.text.push_str(s.as_ref());
            self.text.push('\n');
        }
    }

    pub fn run(args: &[String], has: impl Fn(&str) -> bool, arg_after: impl Fn(&str) -> Option<String>) -> i32 {
        let mut o = Out { text: String::new() };
        let mut code = 0;
        if has("--help") {
            o.line("GrandPiano.exe [--selftest] [--download-samples [--archive FILE]] [--render-test [OUT.wav]] [--synth-test]");
            o.line("  --synth-test renders the synth presets offline and checks levels, clipping and clicks (no samples, no audio device).");
            o.line("  --render-test also renders the Mono Synth and String Machine presets (render-<instrument>-<preset>.wav next to the log).");
            return 0;
        }
        o.line(format!("Grand Piano {} self-test", grand_piano::VERSION));
        o.line(format!("args: {:?}", &args[1..]));
        let s = settings::Settings::load();
        o.line(format!("settings: {}", settings::settings_path().display()));

        if has("--download-samples") {
            let archive = arg_after("--archive").map(std::path::PathBuf::from);
            let prog = Arc::new(download::DownloadProgress::default());
            let p2 = prog.clone();
            let t = std::thread::spawn(move || {
                let mut last = 0;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    let st = p2.stage.load(Ordering::Relaxed);
                    let b = p2.bytes.load(Ordering::Relaxed) / 1_000_000;
                    if st == 1 && b != last {
                        println!("  downloading… {b} MB");
                        last = b;
                    }
                    if st >= 4 {
                        break;
                    }
                }
            });
            match download::ensure_samples(&download::samples_root(), archive.as_deref(), prog.clone()) {
                Ok(p) => o.line(format!("samples OK (SHA-256 verified): {}", p.display())),
                Err(e) => {
                    o.line(format!("SAMPLE DOWNLOAD FAILED: {e:#}"));
                    code = 2;
                }
            }
            prog.stage.store(4, Ordering::Relaxed);
            let _ = t.join();
        }

        let sfz = download::find_installed(&download::samples_root());
        o.line(format!(
            "samples: {}",
            sfz.as_ref().map(|p| format!("installed, verified ({})", p.display())).unwrap_or_else(|| "not installed (downloaded on first launch)".into())
        ));

        if has("--selftest") {
            o.line("");
            o.line("MIDI inputs:");
            let ports = midi::list_ports();
            if ports.is_empty() {
                o.line("  (none)");
            }
            let chosen = midi::choose_port(&ports, s.midi_port.as_deref());
            for p in &ports {
                let tag = if Some(p) == chosen.as_ref() { "  <- would connect" } else { "" };
                o.line(format!("  {p}{}{tag}", if midi::is_oxygen(p) { "  [Oxygen]" } else { "" }));
            }
            o.line("");
            o.line("Audio outputs:");
            match audio_win::list_output_devices() {
                Ok(devs) => {
                    for d in &devs {
                        o.line(format!("  {}{}", d.name, if d.is_default { "  [Windows default]" } else { "" }));
                    }
                }
                Err(e) => o.line(format!("  error: {e:#}")),
            }
            o.line("");
            o.line("Opening the output (silence, 0.5 s) in each mode at 128 frames:");
            for mode in settings::AudioMode::ALL {
                match audio_win::start(s.audio_device.clone(), mode, 128, |_| Box::new(audio_win::Silence)) {
                    Ok(h) => {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        let i = &h.info;
                        o.line(format!(
                            "  {:<20} -> {} | {} | {} frames/callback ({:.2} ms), buffer {}, driver {:.1} ms, est. latency {:.1} ms, late callbacks {}{}",
                            mode.label(),
                            i.mode.label(),
                            i.format,
                            i.period_frames,
                            i.period_frames as f32 / i.sample_rate as f32 * 1000.0,
                            i.buffer_frames,
                            i.stream_latency_ms,
                            i.estimated_latency_ms,
                            h.glitches.load(Ordering::Relaxed),
                            if i.note.is_empty() { String::new() } else { format!(" ({})", i.note) }
                        ));
                        if let Some((d, f, mn, mx)) = i.engine_periods {
                            o.line(format!("  {:<20}    engine periods: default {d}, fundamental {f}, min {mn}, max {mx}", ""));
                        }
                        if h.failed.load(Ordering::Relaxed) {
                            o.line(format!("    stream failed: {:?}", h.error.lock().unwrap()));
                            code = 3;
                        }
                    }
                    Err(e) => o.line(format!("  {:<20} -> FAILED: {e:#}", mode.label())),
                }
            }
        }

        if has("--synth-test") && !has("--render-test") {
            o.line("");
            let (report, ok) = analysis::synth_check(48000, None);
            for l in report.lines() {
                o.line(l);
            }
            if !ok {
                code = 5;
            }
        }

        if has("--render-test") {
            o.line("");
            // the synths need no samples: always check them
            let (report, ok) = analysis::synth_check(48000, Some(&settings::data_dir()));
            for l in report.lines() {
                o.line(l);
            }
            if !ok {
                code = 5;
            }
            o.line("");
            match &sfz {
                None => {
                    o.line("piano render test skipped: samples not installed");
                    if code == 0 {
                        code = 4;
                    }
                }
                Some(p) => {
                    let t = std::time::Instant::now();
                    match library::load_sfz(p, None) {
                        Ok(inst) => {
                            o.line(format!("loaded {} samples in {:.1} s", inst.samples.len(), t.elapsed().as_secs_f32()));
                            let wav = arg_after("--render-test")
                                .filter(|a| !a.starts_with("--"))
                                .map(std::path::PathBuf::from)
                                .unwrap_or_else(|| settings::data_dir().join("render-test.wav"));
                            let (report, ok) = analysis::full_check(Arc::new(inst), 48000, Some(&wav));
                            for l in report.lines() {
                                o.line(l);
                            }
                            if !ok {
                                code = 5;
                            }
                        }
                        Err(e) => {
                            o.line(format!("loading samples failed: {e:#}"));
                            code = 4;
                        }
                    }
                }
            }
        }
        let _ = std::fs::write(settings::data_dir().join("selftest.txt"), &o.text);
        code
    }
}
