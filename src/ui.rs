//! The window: custom-painted, dark and warm. Knobs, velocity-mode switch,
//! sustain meter, live keyboard, MIDI learn, settings and about panels.

use crate::audio_win::{self, AudioHandle, DeviceInfo, Render};
use crate::midi::Midi;
use crate::sink::EventSink;
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2, pos2, vec2};
use grand_piano::download::{self, DownloadProgress};
use grand_piano::engine::{Engine, Event};
use grand_piano::library::{self, Instrument, LoadProgress};
use grand_piano::mapping::Mapper;
use grand_piano::params::{self, InstrumentKind, Params, Target, VelocityMode};
use grand_piano::settings::{AudioMode, Settings};
use grand_piano::synth::{NotePriority, VoiceMode};
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const BG: Color32 = Color32::from_rgb(0x15, 0x12, 0x0f);
const PANEL: Color32 = Color32::from_rgb(0x1d, 0x19, 0x15);
const RAISED: Color32 = Color32::from_rgb(0x27, 0x21, 0x1b);
const LINE: Color32 = Color32::from_rgb(0x3b, 0x32, 0x29);
const TEXT: Color32 = Color32::from_rgb(0xee, 0xe4, 0xd2);
const MUTED: Color32 = Color32::from_rgb(0x93, 0x84, 0x71);
const FAINT: Color32 = Color32::from_rgb(0x5c, 0x51, 0x45);
const ACCENT: Color32 = Color32::from_rgb(0xe6, 0xa9, 0x57);
const LEARN: Color32 = Color32::from_rgb(0xe8, 0x7c, 0x5f);
const OK: Color32 = Color32::from_rgb(0x9c, 0xb8, 0x7a);
const KEY_WHITE: Color32 = Color32::from_rgb(0xec, 0xe5, 0xd8);
const KEY_BLACK: Color32 = Color32::from_rgb(0x1b, 0x18, 0x15);

enum LoadMsg {
    Ready(Arc<Instrument>),
    Failed(String),
}

enum Phase {
    Loading,
    Ready,
    Failed(String),
}

pub struct App {
    settings: Settings,
    params: Arc<Params>,
    mapper: Arc<Mapper>,
    sink: Arc<EventSink>,
    phase: Phase,
    instrument: Option<Arc<Instrument>>,
    load_rx: Option<mpsc::Receiver<LoadMsg>>,
    dl: Arc<DownloadProgress>,
    lp: Arc<LoadProgress>,
    audio: Option<AudioHandle>,
    audio_error: Option<String>,
    audio_retry: Option<Instant>,
    devices: Vec<DeviceInfo>,
    midi: Midi,
    show_settings: bool,
    show_about: bool,
    mouse_key: Option<u8>,
    midi_seen: u32,
    midi_flash: Instant,
    dirty: bool,
    last_save: Instant,
    beat: BeatView,
}

/// What the beat light shows: the newest beat the audio thread reported and the
/// one before it, each with the time it leaves the speakers.
#[derive(Default)]
struct BeatView {
    seq: u32,
    cur: Option<(u64, u8)>,
    prev: Option<(u64, u8)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        let ctx = cc.egui_ctx.clone();
        style(&ctx);
        let settings = Settings::load();
        let params = Arc::new(Params::default());
        let mapper = Arc::new(Mapper::default());
        settings.apply_to(&params, &mapper);
        let sink = Arc::new(EventSink::default());
        let midi = Midi::new(params.clone(), mapper.clone(), sink.clone(), Some(ctx.clone()));
        let mut app = App {
            settings,
            params,
            mapper,
            sink,
            phase: Phase::Loading,
            instrument: None,
            load_rx: None,
            dl: Arc::new(DownloadProgress::default()),
            lp: Arc::new(LoadProgress::default()),
            audio: None,
            audio_error: None,
            audio_retry: None,
            devices: Vec::new(),
            midi,
            show_settings: false,
            show_about: false,
            mouse_key: None,
            midi_seen: 0,
            midi_flash: Instant::now() - Duration::from_secs(5),
            dirty: false,
            last_save: Instant::now(),
            beat: BeatView::default(),
        };
        app.start_loading(ctx);
        app
    }

    fn start_loading(&mut self, ctx: egui::Context) {
        let (tx, rx) = mpsc::channel();
        self.load_rx = Some(rx);
        self.phase = Phase::Loading;
        self.dl = Arc::new(DownloadProgress::default());
        self.lp = Arc::new(LoadProgress::default());
        let (dl, lp) = (self.dl.clone(), self.lp.clone());
        let custom = self.settings.samples_dir.clone();
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<Arc<Instrument>> {
                let sfz = match custom {
                    Some(p) if p.is_file() => p,
                    _ => download::ensure_samples(&download::samples_root(), None, dl)?,
                };
                let t = Instant::now();
                let inst = library::load_sfz(&sfz, Some(lp))?;
                log::info!("loaded {} samples ({} MB) in {:.1} s", inst.samples.len(), inst.memory_bytes / 1_000_000, t.elapsed().as_secs_f32());
                Ok(Arc::new(inst))
            })();
            let _ = tx.send(match res {
                Ok(i) => LoadMsg::Ready(i),
                Err(e) => LoadMsg::Failed(format!("{e:#}")),
            });
            ctx.request_repaint();
        });
    }

    fn start_audio(&mut self) {
        self.audio = None; // stops the old stream first
        let Some(inst) = self.instrument.clone() else { return };
        let params = self.params.clone();
        let consumer = self.sink.new_queue();
        let make = move |rate: u32| -> Box<dyn Render> {
            let mut e = Engine::new(inst, params, rate);
            e.add_input(consumer);
            Box::new(e)
        };
        match audio_win::start(self.settings.audio_device.clone(), self.settings.audio_mode, self.settings.buffer_frames, make) {
            Ok(h) => {
                log::info!("audio: {:?}", h.info);
                self.params.output_latency_ms.set(h.info.estimated_latency_ms);
                self.audio = Some(h);
                self.audio_error = None;
                self.audio_retry = None;
            }
            Err(e) => {
                log::error!("audio start failed: {e:#}");
                self.audio_error = Some(format!("{e:#}"));
                self.audio_retry = Some(Instant::now() + Duration::from_secs(3));
            }
        }
    }

    fn save_now(&mut self) {
        self.settings.capture(&self.params, &self.mapper);
        if let Err(e) = self.settings.save() {
            log::error!("saving settings: {e:#}");
        }
        self.dirty = false;
        self.last_save = Instant::now();
    }

    fn tick(&mut self) {
        if let Some(rx) = &self.load_rx {
            if let Ok(msg) = rx.try_recv() {
                self.load_rx = None;
                match msg {
                    LoadMsg::Ready(inst) => {
                        self.instrument = Some(inst);
                        self.phase = Phase::Ready;
                        self.devices = audio_win::list_output_devices().unwrap_or_default();
                        self.start_audio();
                    }
                    LoadMsg::Failed(e) => self.phase = Phase::Failed(e),
                }
            }
        }
        let preferred = self.settings.midi_port.clone();
        self.midi.poll(preferred.as_deref(), false);
        if let Some(a) = &self.audio {
            if a.failed.load(Ordering::Relaxed) {
                self.audio_error = a.error.lock().unwrap().clone().or(Some("audio device stopped".into()));
                self.audio = None;
                self.audio_retry = Some(Instant::now() + Duration::from_secs(2));
            }
        }
        if let Some(t) = self.audio_retry {
            if Instant::now() >= t && matches!(self.phase, Phase::Ready) {
                self.devices = audio_win::list_output_devices().unwrap_or_default();
                self.start_audio();
            }
        }
        let n = self.params.midi_events.load(Ordering::Relaxed);
        if n != self.midi_seen {
            self.midi_seen = n;
            self.midi_flash = Instant::now();
            self.dirty = true; // a controller may have moved a knob
        }
        if self.dirty && self.last_save.elapsed() > Duration::from_secs(2) {
            self.save_now();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.tick();
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        ui.painter().rect_filled(full, CornerRadius::ZERO, BG);
        match &self.phase {
            Phase::Loading => self.loading_screen(ui, full),
            Phase::Failed(e) => {
                let e = e.clone();
                self.failed_screen(ui, full, &e);
            }
            Phase::Ready => self.main_screen(ui, full),
        }
        if self.show_settings {
            self.settings_window(&ctx);
        }
        if self.show_about {
            self.about_window(&ctx);
        }
        let metro = self.params.metro_running.load(Ordering::Relaxed);
        if metro {
            // the beat light: redraw at least every 16 ms, and right when the next beat is heard
            let now = params::now_us();
            let next = self.beat.cur.map(|(t, _)| t).filter(|t| *t > now).map(|t| Duration::from_micros(t - now));
            ctx.request_repaint_after(next.unwrap_or(Duration::from_millis(16)).min(Duration::from_millis(16)));
        }
        let busy = !matches!(self.phase, Phase::Ready)
            || metro
            || self.mapper.learning().is_some()
            || self.params.keys_sounding[0].load(Ordering::Relaxed) != 0
            || self.params.keys_sounding[1].load(Ordering::Relaxed) != 0
            || self.midi_flash.elapsed() < Duration::from_millis(400);
        ctx.request_repaint_after(Duration::from_millis(if busy { 33 } else { 300 }));
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.save_now();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_now();
        self.audio = None;
        self.midi.disconnect();
    }
}

// ---------------------------------------------------------------- screens

impl App {
    fn loading_screen(&mut self, ui: &mut egui::Ui, full: Rect) {
        let p = ui.painter();
        let c = full.center();
        spaced(p, c - vec2(0.0, 70.0), "GRAND PIANO", 26.0, TEXT, Align2::CENTER_CENTER);
        let stage = self.dl.stage.load(Ordering::Relaxed);
        let (frac, label) = match stage {
            1 => {
                let b = self.dl.bytes.load(Ordering::Relaxed) as f32;
                let t = self.dl.total.load(Ordering::Relaxed).max(1) as f32;
                (b / t, format!("Downloading the piano samples (first run only)   {:.0} / {:.0} MB", b / 1e6, t / 1e6))
            }
            2 => (1.0, "Verifying checksum…".to_string()),
            3 => (1.0, "Unpacking samples…".to_string()),
            _ => {
                let d = self.lp.done.load(Ordering::Relaxed) as f32;
                let t = self.lp.total.load(Ordering::Relaxed).max(1) as f32;
                (d / t, format!("Loading the Salamander Grand Piano   {} / {}", d as u32, t as u32))
            }
        };
        let bar = Rect::from_center_size(c, vec2(420.0, 6.0));
        p.rect_filled(bar, CornerRadius::same(3), RAISED);
        let mut fill = bar;
        fill.set_width(bar.width() * frac.clamp(0.0, 1.0));
        p.rect_filled(fill, CornerRadius::same(3), ACCENT);
        p.text(c + vec2(0.0, 28.0), Align2::CENTER_CENTER, label, FontId::proportional(14.0), MUTED);
    }

    fn failed_screen(&mut self, ui: &mut egui::Ui, full: Rect, err: &str) {
        let c = full.center();
        let p = ui.painter();
        spaced(p, c - vec2(0.0, 90.0), "GRAND PIANO", 26.0, TEXT, Align2::CENTER_CENTER);
        p.text(c - vec2(0.0, 40.0), Align2::CENTER_CENTER, "The piano samples couldn't be loaded.", FontId::proportional(16.0), LEARN);
        let galley = p.layout(err.to_string(), FontId::proportional(13.0), MUTED, 640.0);
        p.galley(pos2(c.x - galley.size().x / 2.0, c.y - 18.0), galley, MUTED);
        let r = Rect::from_center_size(c + vec2(0.0, 70.0), vec2(120.0, 36.0));
        if pill_button(ui, r, "Try again", true).clicked() {
            let ctx = ui.ctx().clone();
            self.start_loading(ctx);
        }
    }

    fn main_screen(&mut self, ui: &mut egui::Ui, full: Rect) {
        let m = 26.0;
        let inner = full.shrink(m);
        // ---- header
        let header = Rect::from_min_size(inner.min, vec2(inner.width(), 40.0));
        self.header(ui, header);
        // ---- keyboard at the bottom, with the octave row above it
        let kb_h = (inner.height() * 0.26).clamp(100.0, 180.0);
        let kb = Rect::from_min_max(pos2(inner.min.x, inner.max.y - kb_h), inner.max);
        self.keyboard(ui, kb);
        let kb_row = Rect::from_min_max(pos2(inner.min.x, kb.min.y - 36.0), pos2(inner.max.x, kb.min.y - 8.0));
        self.keyboard_row(ui, kb_row);
        // ---- middle: instrument panel (left) and metronome (right)
        let mid = Rect::from_min_max(pos2(inner.min.x, header.max.y + 14.0), pos2(inner.max.x, kb_row.min.y - 6.0));
        let metro_w = if mid.width() > 1050.0 { 280.0 } else { 250.0 };
        let metro = Rect::from_min_max(pos2(mid.max.x - metro_w, mid.min.y), mid.max);
        let left = Rect::from_min_max(mid.min, pos2(metro.min.x - 14.0, mid.max.y));
        ui.painter().rect_filled(left, CornerRadius::same(18), PANEL);
        ui.painter().rect_filled(metro, CornerRadius::same(18), PANEL);
        // instruments with presets get a second row (presets, Voices / Leslie switch)
        let inst = self.params.instrument();
        let strip_h = if inst.presets().is_empty() { 60.0 } else { 100.0 };
        let strip = Rect::from_min_size(left.min, vec2(left.width(), strip_h));
        self.instrument_strip(ui, strip);
        let row_h = 104.0;
        let mut knob_area = Rect::from_min_max(pos2(left.min.x, strip.max.y), pos2(left.max.x, (left.max.y - row_h).max(strip.max.y + 120.0)));
        let targets: &[Target] = match inst {
            InstrumentKind::GrandPiano => &[Target::Volume, Target::Tone, Target::Reverb, Target::Modulation, Target::Touch],
            InstrumentKind::MonoSynth => &[Target::Volume, Target::Tone, Target::Resonance, Target::Glide, Target::Reverb, Target::Modulation, Target::Touch],
            InstrumentKind::StringMachine | InstrumentKind::ElectricPiano => &[Target::Volume, Target::Tone, Target::Reverb, Target::Modulation, Target::Touch],
            // the organ has no velocity (Touch) and its wheel is the Leslie switch above
            InstrumentKind::Organ => &[Target::Volume, Target::Tone, Target::Reverb],
            InstrumentKind::Polysynth => &[Target::Volume, Target::Tone, Target::Resonance, Target::Reverb, Target::Modulation, Target::Touch],
        };
        if inst == InstrumentKind::Organ {
            // the drawbars take the right part of the knob row
            let split = knob_area.min.x + knob_area.width() * 0.44;
            let bars = Rect::from_min_max(pos2(split, knob_area.min.y + 4.0), pos2(knob_area.max.x - 16.0, knob_area.max.y - 4.0));
            ui.painter().rect_filled(bars, CornerRadius::same(12), Color32::from_rgb(0x19, 0x16, 0x12));
            self.drawbar_panel(ui, bars);
            knob_area.max.x = split;
        }
        let w = knob_area.width() / targets.len() as f32;
        for (i, t) in targets.iter().enumerate() {
            let r = Rect::from_min_size(pos2(knob_area.min.x + w * i as f32, knob_area.min.y), vec2(w, knob_area.height()));
            self.knob(ui, r, *t);
        }
        let row = Rect::from_min_max(pos2(left.min.x, knob_area.max.y), left.max);
        self.mode_row(ui, row);
        self.metronome_panel(ui, metro);
    }

    /// The row above the on-screen keyboard: the hardware range (inferred) on the
    /// left, the app's own octave shift (exact) on the right.
    fn keyboard_row(&mut self, ui: &mut egui::Ui, r: Rect) {
        let p = ui.painter().clone();
        let cy = r.center().y;
        let (lo, hi) = self.sounding_span();
        let hw = self.params.hw_octave.load(Ordering::Relaxed);
        let seen = self.params.hw_seen.load(Ordering::Relaxed);
        spaced(&p, pos2(r.min.x + 4.0, cy), "KEYBOARD", 11.0, MUTED, Align2::LEFT_CENTER);
        let txt = if seen {
            format!("Oct {} (detected) · plays {}–{}", signed(hw), note_name(lo), note_name(hi))
        } else {
            format!("Oct 0 (assumed; updates once you play) · plays {}–{}", note_name(lo), note_name(hi))
        };
        let g = p.layout_no_wrap(txt, FontId::proportional(12.5), TEXT);
        let tx = r.min.x + 92.0;
        p.galley(pos2(tx, cy - g.size().y / 2.0), g.clone(), TEXT);
        let hover = ui.interact(Rect::from_min_size(pos2(tx, cy - 9.0), vec2(g.size().x, 18.0)), ui.make_persistent_id("hw_range"), Sense::hover());
        hover.on_hover_text(
            "The keyboard's own Octave buttons don't send any MIDI, so this is worked out from the notes you play: \
             when a note falls outside the highlighted span, the span moves to the nearest octave that fits the last few seconds of playing.",
        );
        // right: [chip] [−] Oct +1 [+] [chip]
        let oct = self.params.octave.load(Ordering::Relaxed);
        let tr = self.params.transpose.load(Ordering::Relaxed);
        let mut x = r.max.x;
        let chip_w = 70.0;
        self.mapping_chip(ui, pos2(x - chip_w / 2.0, cy), Target::OctaveUp);
        x -= chip_w + 6.0;
        let plus = Rect::from_center_size(pos2(x - 14.0, cy), vec2(28.0, 26.0));
        x -= 28.0;
        let readout = if tr != 0 { format!("Oct {} · Tr {}", signed(oct), signed(tr)) } else { format!("Oct {}", signed(oct)) };
        let rw = if tr != 0 { 110.0 } else { 64.0 };
        let rr = Rect::from_center_size(pos2(x - rw / 2.0, cy), vec2(rw, 26.0));
        x -= rw;
        let minus = Rect::from_center_size(pos2(x - 14.0, cy), vec2(28.0, 26.0));
        x -= 28.0 + 6.0;
        self.mapping_chip(ui, pos2(x - chip_w / 2.0, cy), Target::OctaveDown);
        x -= chip_w + 4.0;
        spaced(&p, pos2(x, cy), "OCTAVE", 11.0, MUTED, Align2::RIGHT_CENTER);
        p.rect_filled(Rect::from_min_max(minus.min, plus.max), CornerRadius::same(13), RAISED);
        p.text(rr.center(), Align2::CENTER_CENTER, readout, FontId::proportional(13.5), if oct != 0 || tr != 0 { ACCENT } else { TEXT });
        for (b, sym, d) in [(minus, "−", -1), (plus, "+", 1)] {
            let resp = ui.interact(b, ui.make_persistent_id(("oct", d)), Sense::click());
            p.text(b.center(), Align2::CENTER_CENTER, sym, FontId::proportional(17.0), if resp.hovered() { ACCENT } else { MUTED });
            if resp.clicked() {
                self.params.step_octave(d);
                self.dirty = true;
            }
        }
        let rresp = ui.interact(rr, ui.make_persistent_id("oct_readout"), Sense::click());
        if rresp.double_clicked() {
            self.params.octave.store(0, Ordering::Relaxed);
            self.dirty = true;
        }
        rresp.on_hover_text("The app's own octave shift (exact), added to every note from the keyboard. Double-click to reset. Transpose is in Settings.");
    }

    /// The notes the hardware keyboard currently plays: its inferred span plus the app's shift.
    fn sounding_span(&self) -> (i32, i32) {
        let hw = self.params.hw_octave.load(Ordering::Relaxed);
        let (lo, hi) = grand_piano::keyrange::RangeDetector::span(hw);
        let s = self.params.note_shift();
        (lo + s, hi + s)
    }

    /// Two rows: the instrument selector across the top (with its MIDI-learn
    /// chip), then the preset selector and, on the Moog-style Synth and the
    /// Organ, their Voices / Leslie switch.
    fn instrument_strip(&mut self, ui: &mut egui::Ui, r: Rect) {
        let p = ui.painter().clone();
        let cur = self.params.instrument();
        let pad = 20.0;
        let chip_w = 74.0;
        // ---- row 1: instruments
        let h = 32.0;
        let y = r.min.y + 12.0;
        let n = InstrumentKind::ALL.len() as f32;
        let seg = ((r.width() - 2.0 * pad - chip_w - 10.0) / n).min(132.0);
        let total = Rect::from_min_size(pos2(r.min.x + pad, y), vec2(seg * n, h));
        p.rect_filled(total, CornerRadius::same(16), RAISED);
        for (i, k) in InstrumentKind::ALL.iter().enumerate() {
            let sr = Rect::from_min_size(pos2(total.min.x + seg * i as f32, total.min.y), vec2(seg, h));
            let resp = ui.interact(sr, ui.make_persistent_id(("inst", i)), Sense::click());
            let sel = *k == cur;
            if sel {
                p.rect_filled(sr.shrink(3.0), CornerRadius::same(13), ACCENT);
            } else if resp.hovered() {
                p.rect_filled(sr.shrink(3.0), CornerRadius::same(13), LINE);
            }
            // narrow window: a short name if the full one doesn't fit
            let name = if p.layout_no_wrap(k.label().to_string(), FontId::proportional(10.5), TEXT).size().x > seg - 10.0 { short_label(*k) } else { k.label() };
            let size = fit_size(&p, name, 13.5, 10.5, seg - 10.0);
            p.text(sr.center(), Align2::CENTER_CENTER, name, FontId::proportional(size), if sel { BG } else { TEXT });
            if resp.clicked() {
                self.params.set_instrument(*k);
                self.dirty = true;
            }
            resp.on_hover_text(k.description());
        }
        self.mapping_chip(ui, pos2(total.max.x + 10.0 + chip_w / 2.0, total.center().y), Target::Instrument);
        // ---- row 2: presets (left) and the instrument's switch (right)
        let names = cur.presets();
        if names.is_empty() {
            return;
        }
        let h2 = 30.0;
        let cy = total.max.y + 12.0 + h2 / 2.0;
        // the switch on the right, laid out right to left: [chip] [A|B] LABEL
        let switch: Option<(&str, [&str; 2], usize, Target)> = match cur {
            InstrumentKind::MonoSynth => Some(("VOICES", ["Poly", "Mono"], self.params.voice_mode().id() as usize, Target::Voices)),
            InstrumentKind::Organ => Some(("LESLIE", ["Slow", "Fast"], self.params.leslie_fast.load(Ordering::Relaxed) as usize, Target::Leslie)),
            _ => None,
        };
        let mut right = r.max.x - pad;
        if let Some((label, opts, sel_idx, target)) = switch {
            self.mapping_chip(ui, pos2(right - chip_w / 2.0, cy), target);
            right -= chip_w + 8.0;
            let sw = 64.0;
            let st = Rect::from_min_size(pos2(right - sw * 2.0, cy - h2 / 2.0), vec2(sw * 2.0, h2));
            p.rect_filled(st, CornerRadius::same(15), RAISED);
            for (i, o) in opts.iter().enumerate() {
                let sr = Rect::from_min_size(pos2(st.min.x + sw * i as f32, st.min.y), vec2(sw, h2));
                let resp = ui.interact(sr, ui.make_persistent_id(("switch", label, i)), Sense::click());
                let sel = i == sel_idx;
                if sel {
                    p.rect_filled(sr.shrink(3.0), CornerRadius::same(12), ACCENT);
                } else if resp.hovered() {
                    p.rect_filled(sr.shrink(3.0), CornerRadius::same(12), LINE);
                }
                p.text(sr.center(), Align2::CENTER_CENTER, *o, FontId::proportional(13.5), if sel { BG } else { TEXT });
                if resp.clicked() {
                    match target {
                        Target::Voices => self.params.set_voice_mode(VoiceMode::ALL[i]),
                        _ => self.params.leslie_fast.store(i == 1, Ordering::Relaxed),
                    }
                    self.dirty = true;
                }
                resp.on_hover_text(match (target, i) {
                    (Target::Voices, 0) => "Poly: up to 8 notes at once, so chords work. Glide is off.",
                    (Target::Voices, _) => match self.params.note_priority() {
                        NotePriority::Last => "Mono: one note at a time, legato, with glide. The last key pressed wins (Note priority is in Settings).",
                        NotePriority::High => "Mono: one note at a time, legato, with glide. The highest held key wins (Note priority is in Settings).",
                        NotePriority::Low => "Mono: one note at a time, legato, with glide. The lowest held key wins (Note priority is in Settings).",
                    },
                    (_, 0) => "Slow (chorale). The mod wheel switches too: below half = slow, above = fast.",
                    _ => "Fast (tremolo). The horn gets up to speed in about a second, the heavier drum in a few.",
                });
            }
            spaced(&p, pos2(st.min.x - 10.0, cy), label, 11.0, MUTED, Align2::RIGHT_CENTER);
            right = st.min.x - 10.0 - 64.0 - 16.0;
        }
        // presets: PRESET [a|b|c] [chip]
        let left = r.min.x + pad;
        let label_w = 62.0;
        let n = names.len() as f32;
        let ps = ((right - left - label_w - chip_w - 10.0) / n).clamp(44.0, 118.0);
        let ptotal = Rect::from_min_size(pos2(left + label_w, cy - h2 / 2.0), vec2(ps * n, h2));
        spaced(&p, pos2(left, cy), "PRESET", 11.0, MUTED, Align2::LEFT_CENTER);
        p.rect_filled(ptotal, CornerRadius::same(15), RAISED);
        let sel_idx = self.params.preset_of(cur);
        for (i, name) in names.iter().enumerate() {
            let sr = Rect::from_min_size(pos2(ptotal.min.x + ps * i as f32, ptotal.min.y), vec2(ps, h2));
            let resp = ui.interact(sr, ui.make_persistent_id(("preset", i)), Sense::click());
            let sel = i == sel_idx;
            if sel {
                p.rect_filled(sr.shrink(3.0), CornerRadius::same(12), ACCENT);
            } else if resp.hovered() {
                p.rect_filled(sr.shrink(3.0), CornerRadius::same(12), LINE);
            }
            let size = fit_size(&p, name, 13.5, 10.0, ps - 8.0);
            p.text(sr.center(), Align2::CENTER_CENTER, *name, FontId::proportional(size), if sel { BG } else { TEXT });
            if resp.clicked() {
                self.params.select_preset(cur, i);
                self.dirty = true;
            }
            resp.on_hover_text(preset_hint(cur, i));
        }
        self.mapping_chip(ui, pos2(ptotal.max.x + 10.0 + chip_w / 2.0, cy), Target::Preset);
    }

    /// The organ's drawbars (pull down = louder, 0..8, coloured like the
    /// original: brown sub-octaves, white octaves, black mutations) and its
    /// percussion switches. Drag or scroll a bar; double-click resets it to the preset.
    fn drawbar_panel(&mut self, ui: &mut egui::Ui, r: Rect) {
        let p = ui.painter().clone();
        let inner = r.shrink2(vec2(14.0, 10.0));
        let reg: String = self.params.drawbars.iter().map(|d| char::from(b'0' + d.load(Ordering::Relaxed).min(8))).collect();
        spaced(&p, pos2(inner.min.x, inner.min.y + 14.0), "DRAWBARS", 11.0, MUTED, Align2::LEFT_CENTER);
        p.text(pos2(inner.max.x, inner.min.y + 14.0), Align2::RIGHT_CENTER, &reg, FontId::monospace(13.0), TEXT);
        let perc_h = 30.0;
        let top = inner.min.y + 34.0;
        let bottom = inner.max.y - perc_h - 10.0;
        let slot_h = (bottom - top - 16.0).max(40.0);
        let cw = inner.width() / 9.0;
        let preset = grand_piano::organ::ORGAN_PRESETS[self.params.preset_of(InstrumentKind::Organ)];
        for d in 0..9 {
            let cx = inner.min.x + cw * (d as f32 + 0.5);
            let slot = Rect::from_min_max(pos2(cx - 4.0, top), pos2(cx + 4.0, top + slot_h));
            p.rect_filled(slot, CornerRadius::same(3), RAISED);
            let val = self.params.drawbars[d].load(Ordering::Relaxed).min(8);
            let len = slot_h * val as f32 / 8.0;
            let (fill, ink) = match d {
                0 | 1 => (Color32::from_rgb(0x7a, 0x47, 0x2b), TEXT),
                4 | 6 | 7 => (KEY_BLACK, TEXT),
                _ => (KEY_WHITE, BG),
            };
            if val > 0 {
                p.rect_filled(Rect::from_min_max(pos2(cx - 3.0, top), pos2(cx + 3.0, top + len)), CornerRadius::same(2), FAINT);
            }
            let handle = Rect::from_center_size(pos2(cx, top + len), vec2((cw - 8.0).clamp(14.0, 28.0), 16.0));
            p.rect(handle, CornerRadius::same(4), fill, Stroke::new(1.0, LINE), StrokeKind::Inside);
            p.text(handle.center(), Align2::CENTER_CENTER, val.to_string(), FontId::proportional(11.0), ink);
            p.text(pos2(cx, top + slot_h + 12.0), Align2::CENTER_CENTER, grand_piano::organ::DRAWBAR_NAMES[d], FontId::proportional(10.5), MUTED);
            let hit = Rect::from_min_max(pos2(cx - cw / 2.0, top - 8.0), pos2(cx + cw / 2.0, top + slot_h + 8.0));
            let resp = ui.interact(hit, ui.make_persistent_id(("drawbar", d)), Sense::click_and_drag());
            let mut nv = val as i32;
            if resp.is_pointer_button_down_on() {
                if let Some(pp) = resp.interact_pointer_pos() {
                    nv = (((pp.y - top) / slot_h) * 8.0).round() as i32;
                }
            }
            if resp.hovered() {
                let s = ui.input(|i| i.smooth_scroll_delta.y);
                if s.abs() > 0.5 {
                    nv -= s.signum() as i32;
                }
            }
            if resp.double_clicked() {
                nv = preset.drawbars[d] as i32;
            }
            let nv = nv.clamp(0, 8) as u8;
            if nv != val {
                self.params.drawbars[d].store(nv, Ordering::Relaxed);
                self.dirty = true;
            }
            resp.on_hover_text(format!("{} drawbar: {val} of 8. Drag or scroll; double-click for the preset's value.", grand_piano::organ::DRAWBAR_NAMES[d]));
        }
        // percussion switches
        let py = inner.max.y - perc_h;
        let pw = (inner.width() - 18.0) / 4.0;
        let flags = [
            (&self.params.perc_on, "Perc", "Percussion: a decaying 2nd or 3rd harmonic on the attack, single-triggered (only when no other key is held). Turns the 1' drawbar off, as on the original."),
            (&self.params.perc_third, "3rd", "On: 3rd harmonic (2⅔'). Off: 2nd harmonic (4')."),
            (&self.params.perc_soft, "Soft", "Soft percussion (quieter). Normal volume also lowers the drawbars about 3 dB, as on the original."),
            (&self.params.perc_fast, "Fast", "Fast decay (about half a second). Off: slow (about 2 s)."),
        ];
        let mut toggled = None;
        for (i, (flag, label, tip)) in flags.iter().enumerate() {
            let on = flag.load(Ordering::Relaxed);
            let rr = Rect::from_min_size(pos2(inner.min.x + (pw + 6.0) * i as f32, py), vec2(pw, perc_h - 2.0));
            let resp = toggle_pill(ui, rr, label, on);
            if resp.clicked() {
                toggled = Some(i);
            }
            resp.on_hover_text(*tip);
        }
        if let Some(i) = toggled {
            flags[i].0.fetch_xor(true, Ordering::Relaxed);
            self.dirty = true;
        }
    }

    /// The beat light's brightness (0..1) and the beat-in-bar to show, from the
    /// audio thread's beat times (already shifted by the output latency).
    fn beat_light(&mut self, now: u64) -> (f32, Option<u8>) {
        let seq = self.params.beat_seq.load(Ordering::Acquire);
        if seq != self.beat.seq {
            self.beat.seq = seq;
            self.beat.prev = self.beat.cur;
            self.beat.cur = Some((self.params.beat_audible_us.load(Ordering::Relaxed), self.params.beat_in_bar.load(Ordering::Relaxed)));
        }
        let shown = match self.beat.cur {
            Some((t, b)) if now >= t => Some((t, b)),
            _ => self.beat.prev.filter(|(t, _)| now >= *t),
        };
        match shown {
            Some((t, b)) => {
                let age = (now - t) as f32 / 1e6;
                let base = if b == 0 { 1.0 } else { 0.55 };
                (if age < 0.6 { base * (-age / 0.12).exp() } else { 0.0 }, Some(b))
            }
            None => (0.0, None),
        }
    }

    fn metronome_panel(&mut self, ui: &mut egui::Ui, r: Rect) {
        let p = ui.painter().clone();
        let inner = r.shrink2(vec2(18.0, 12.0));
        let running = self.params.metro_running.load(Ordering::Relaxed);
        let bpb = self.params.beats_per_bar.load(Ordering::Relaxed).clamp(2, 6);
        let blink = self.params.blink_on.load(Ordering::Relaxed);
        let now = params::now_us();
        let (lamp, beat_idx) = if running { self.beat_light(now) } else { (0.0, None) };
        let lamp = if blink { lamp } else { 0.0 };
        // ---- title and beat dots
        let ay = inner.min.y + 12.0;
        spaced(&p, pos2(inner.min.x, ay), "METRONOME", 11.5, MUTED, Align2::LEFT_CENTER);
        for b in 0..bpb {
            let x = inner.max.x - (bpb - 1 - b) as f32 * 16.0 - 5.0;
            let lit = blink && beat_idx == Some(b);
            let rad = if b == 0 { 5.0 } else { 3.8 };
            p.circle_filled(pos2(x, ay), rad, if lit { ACCENT } else { LINE });
        }
        // ---- tempo knob, big BPM readout, beat lamp, tap
        let b_top = ay + 18.0;
        let bh = 124.0;
        let knob_r = Rect::from_min_size(pos2(inner.min.x - 6.0, b_top), vec2(inner.width() * 0.46, bh));
        self.knob(ui, knob_r, Target::Tempo);
        let right = Rect::from_min_max(pos2(knob_r.max.x + 6.0, b_top), pos2(inner.max.x, b_top + bh));
        let lamp_c = pos2(right.min.x + 13.0, b_top + 30.0);
        if lamp > 0.01 {
            p.circle_filled(lamp_c, 11.0 + 9.0 * lamp, ACCENT.gamma_multiply(0.22 * lamp));
        }
        p.circle_filled(lamp_c, 11.0, lerp_color(RAISED, ACCENT, lamp.min(1.0)));
        p.circle_stroke(lamp_c, 11.0, Stroke::new(1.0, LINE));
        let bpm = self.params.tempo_bpm();
        p.text(pos2(right.max.x, b_top + 30.0), Align2::RIGHT_CENTER, format!("{bpm:.0}"), FontId::proportional(40.0), TEXT);
        spaced(&p, pos2(right.max.x, b_top + 60.0), "BPM", 10.5, MUTED, Align2::RIGHT_CENTER);
        let tap_r = Rect::from_min_size(pos2(right.max.x - 76.0, b_top + 74.0), vec2(76.0, 28.0));
        let tap = pill_button(ui, tap_r, "TAP", false);
        if tap.clicked() {
            self.params.tap(params::now_us());
            self.dirty = true;
        }
        tap.on_hover_text("Tap on the beat a few times to set the tempo.");
        self.mapping_chip(ui, pos2(tap_r.center().x, tap_r.max.y + 13.0), Target::TapTempo);
        // ---- start / stop
        let c_top = b_top + bh + 6.0;
        let btn = Rect::from_min_size(pos2(inner.min.x, c_top), vec2(inner.width() - 80.0, 46.0));
        let resp = ui.interact(btn, ui.make_persistent_id("metro_start"), Sense::click());
        let fill = match (running, resp.hovered()) {
            (true, true) => lerp_color(ACCENT, TEXT, 0.2),
            (true, false) => ACCENT,
            (false, true) => LINE,
            (false, false) => RAISED,
        };
        p.rect_filled(btn, CornerRadius::same(23), fill);
        let fg = if running { BG } else { TEXT };
        let ic = pos2(btn.center().x - 34.0, btn.center().y);
        if running {
            p.rect_filled(Rect::from_center_size(ic, vec2(13.0, 13.0)), CornerRadius::same(2), fg);
        } else {
            p.add(Shape::convex_polygon(vec![ic + vec2(-6.0, -8.0), ic + vec2(9.0, 0.0), ic + vec2(-6.0, 8.0)], fg, Stroke::NONE));
        }
        p.text(pos2(ic.x + 18.0, btn.center().y), Align2::LEFT_CENTER, if running { "STOP" } else { "START" }, FontId::proportional(17.0), fg);
        if resp.clicked() {
            self.params.toggle_metronome();
        }
        self.mapping_chip(ui, pos2(btn.max.x + 40.0, btn.center().y), Target::MetronomeStartStop);
        // ---- click / blink toggles, beats per bar
        let d_top = btn.max.y + 10.0;
        let step_w = 96.0;
        let tw = (inner.width() - step_w - 12.0) / 2.0;
        let click_on = self.params.click_on.load(Ordering::Relaxed);
        if toggle_pill(ui, Rect::from_min_size(pos2(inner.min.x, d_top), vec2(tw, 28.0)), "Click", click_on).clicked() {
            self.params.click_on.store(!click_on, Ordering::Relaxed);
            self.dirty = true;
        }
        if toggle_pill(ui, Rect::from_min_size(pos2(inner.min.x + tw + 6.0, d_top), vec2(tw, 28.0)), "Blink", blink).clicked() {
            self.params.blink_on.store(!blink, Ordering::Relaxed);
            self.dirty = true;
        }
        let st = Rect::from_min_size(pos2(inner.max.x - step_w, d_top), vec2(step_w, 28.0));
        p.rect_filled(st, CornerRadius::same(14), RAISED);
        let minus = Rect::from_min_size(st.min, vec2(28.0, 28.0));
        let plus = Rect::from_min_size(pos2(st.max.x - 28.0, st.min.y), vec2(28.0, 28.0));
        for (rr, sym, d) in [(minus, "−", -1i32), (plus, "+", 1)] {
            let rsp = ui.interact(rr, ui.make_persistent_id(("bpb", d)), Sense::click());
            p.text(rr.center(), Align2::CENTER_CENTER, sym, FontId::proportional(16.0), if rsp.hovered() { ACCENT } else { MUTED });
            if rsp.clicked() {
                let v = (bpb as i32 + d).clamp(2, 6) as u8;
                self.params.beats_per_bar.store(v, Ordering::Relaxed);
                self.dirty = true;
            }
        }
        p.text(st.center(), Align2::CENTER_CENTER, format!("{bpb}/bar"), FontId::proportional(13.0), TEXT);
        // ---- click level
        let e_y = d_top + 28.0 + 18.0;
        spaced(&p, pos2(inner.min.x, e_y), "LEVEL", 10.5, MUTED, Align2::LEFT_CENTER);
        let bar = Rect::from_min_max(pos2(inner.min.x + 58.0, e_y - 4.0), pos2(inner.max.x, e_y + 4.0));
        p.rect_filled(bar, CornerRadius::same(4), RAISED);
        let lvl = self.params.click_level.get();
        let mut f = bar;
        f.set_width(bar.width() * lvl);
        p.rect_filled(f, CornerRadius::same(4), if click_on { ACCENT } else { FAINT });
        let lr = ui.interact(bar.expand(8.0), ui.make_persistent_id("click_level"), Sense::click_and_drag());
        if lr.is_pointer_button_down_on() {
            if let Some(pp) = lr.interact_pointer_pos() {
                self.params.click_level.set(((pp.x - bar.min.x) / bar.width()).clamp(0.0, 1.0));
                self.dirty = true;
            }
        }
        lr.on_hover_text(format!("Click level {:.0}%", lvl * 100.0));
    }

    fn header(&mut self, ui: &mut egui::Ui, r: Rect) {
        let p = ui.painter().clone();
        spaced(&p, pos2(r.min.x, r.center().y - 7.0), "GRAND PIANO", 19.0, TEXT, Align2::LEFT_CENTER);
        p.text(pos2(r.min.x, r.center().y + 13.0), Align2::LEFT_CENTER, self.params.instrument().description(), FontId::proportional(12.0), MUTED);
        // right side, laid out right-to-left
        let mut x = r.max.x;
        let info_r = Rect::from_center_size(pos2(x - 16.0, r.center().y), vec2(32.0, 32.0));
        if icon_button(ui, info_r, "i").clicked() {
            self.show_about = !self.show_about;
        }
        x -= 40.0;
        let gear_r = Rect::from_center_size(pos2(x - 16.0, r.center().y), vec2(32.0, 32.0));
        if icon_button(ui, gear_r, "gear").clicked() {
            self.show_settings = !self.show_settings;
            if self.show_settings {
                self.devices = audio_win::list_output_devices().unwrap_or_default();
                self.midi.poll(self.settings.midi_port.clone().as_deref(), true);
            }
        }
        x -= 52.0;
        // latency readout
        let (lat_text, lat_color) = match (&self.audio, &self.audio_error) {
            (Some(a), _) => (
                format!("{:.1} ms  ·  {} @ {} kHz", a.info.estimated_latency_ms, a.info.period_frames, a.info.sample_rate as f32 / 1000.0),
                if a.info.estimated_latency_ms < 10.0 { MUTED } else { ACCENT },
            ),
            (None, Some(_)) => ("audio unavailable".to_string(), LEARN),
            _ => ("starting audio…".to_string(), MUTED),
        };
        let g = p.layout_no_wrap(lat_text, FontId::monospace(12.0), lat_color);
        let chip = Rect::from_min_size(pos2(x - g.size().x - 20.0, r.center().y - 13.0), vec2(g.size().x + 20.0, 26.0));
        p.rect(chip, CornerRadius::same(13), PANEL, Stroke::new(1.0, LINE), StrokeKind::Inside);
        p.galley(chip.center() - g.size() / 2.0, g, lat_color);
        let lat_resp = ui.interact(chip, ui.make_persistent_id("latency"), Sense::click());
        if lat_resp.clicked() {
            self.show_settings = true;
        }
        lat_resp.on_hover_text("Estimated output latency: buffer + Windows audio engine + driver + 1 ms limiter look-ahead. Click for audio settings.");
        x = chip.min.x - 16.0;
        // MIDI status
        let flash = self.midi_flash.elapsed().as_secs_f32();
        let (dot, name) = match &self.midi.connected {
            Some(n) => {
                let a = (1.0 - flash / 0.25).clamp(0.0, 1.0);
                (lerp_color(OK, ACCENT, a), n.clone())
            }
            None => (LEARN, "no MIDI keyboard".to_string()),
        };
        let g = p.layout_no_wrap(name, FontId::proportional(13.0), if self.midi.connected.is_some() { TEXT } else { LEARN });
        let tx = x - g.size().x;
        p.galley(pos2(tx, r.center().y - g.size().y / 2.0), g, TEXT);
        p.circle_filled(pos2(tx - 12.0, r.center().y), 4.5, dot);
        if flash < 0.25 && self.midi.connected.is_some() {
            p.circle_stroke(pos2(tx - 12.0, r.center().y), 4.5 + 6.0 * flash / 0.25, Stroke::new(1.0, ACCENT.gamma_multiply(1.0 - flash / 0.25)));
        }
    }

    fn mapping_chip(&mut self, ui: &mut egui::Ui, center: Pos2, t: Target) {
        let learning = self.mapper.learning() == Some(t);
        let ccs = self.mapper.ccs_for(t);
        let text = if learning {
            "move a control…".to_string()
        } else if ccs.is_empty() {
            "learn".to_string()
        } else {
            let list: Vec<String> = ccs.iter().map(|c| c.to_string()).collect();
            format!("CC {}", list.join(" · "))
        };
        let p = ui.painter();
        let g = p.layout_no_wrap(text, FontId::proportional(11.0), if learning { BG } else { MUTED });
        let r = Rect::from_center_size(center, vec2(g.size().x + 18.0, 20.0));
        let resp = ui.interact(r, ui.make_persistent_id(("chip", t.id())), Sense::click());
        let pulse = (ui.input(|i| i.time) * 4.0).sin() as f32 * 0.5 + 0.5;
        if learning {
            p.rect_filled(r, CornerRadius::same(10), lerp_color(LEARN, ACCENT, pulse * 0.4));
        } else {
            let stroke = if resp.hovered() { ACCENT } else { LINE };
            p.rect(r, CornerRadius::same(10), Color32::TRANSPARENT, Stroke::new(1.0, stroke), StrokeKind::Inside);
        }
        p.galley(r.center() - g.size() / 2.0, g, if learning { BG } else { MUTED });
        if resp.clicked() {
            if learning {
                self.mapper.cancel_learn();
            } else {
                self.mapper.start_learn(t);
            }
        }
        if resp.secondary_clicked() {
            self.mapper.clear_target(t);
            self.dirty = true;
        }
        resp.on_hover_text("Click, then move a knob, fader or wheel on your keyboard to map it. Right-click to clear.");
        if learning {
            self.dirty = true;
        }
    }

    fn knob(&mut self, ui: &mut egui::Ui, area: Rect, t: Target) {
        let mode = self.params.mode();
        let inst = self.params.instrument();
        let label = match t {
            Target::Touch if mode == VelocityMode::Fixed => "LEVEL",
            Target::Touch => "TOUCH",
            Target::Volume => "VOLUME",
            Target::Tone => match inst {
                InstrumentKind::GrandPiano | InstrumentKind::ElectricPiano => "TONE",
                InstrumentKind::Organ => "DRIVE",
                _ => "CUTOFF",
            },
            Target::Reverb => "ROOM",
            Target::Modulation => match inst {
                InstrumentKind::GrandPiano => "SHIMMER",
                InstrumentKind::ElectricPiano => "TREMOLO",
                InstrumentKind::Organ => "LESLIE",
                _ => "VIBRATO",
            },
            Target::Resonance => "RESONANCE",
            Target::Glide => "GLIDE",
            Target::Tempo => "TEMPO",
            _ => "",
        };
        // the metronome's tempo knob is compact: its value is the big BPM readout
        let compact = t == Target::Tempo;
        let radius = if compact {
            ((area.height() - 70.0) / 2.0 - 10.0).min(area.width() * 0.3).clamp(18.0, 40.0)
        } else {
            ((area.height() - 106.0) / 2.0).min(area.width() * 0.26).clamp(22.0, 58.0)
        };
        let center = if compact { pos2(area.center().x, area.min.y + 22.0 + radius + 10.0) } else { pos2(area.center().x, area.min.y + 34.0 + radius + 8.0) };
        let p = ui.painter().clone();
        let label_size = (area.width() / 10.0).clamp(8.5, 11.5);
        spaced(&p, pos2(center.x, if compact { area.min.y + 10.0 } else { area.min.y + 26.0 }), label, label_size, MUTED, Align2::CENTER_CENTER);
        let hit = Rect::from_center_size(center, Vec2::splat(radius * 2.3));
        let resp = ui.interact(hit, ui.make_persistent_id(("knob", t.id())), Sense::click_and_drag());
        let mut v = self.params.get_target(t);
        if resp.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let d = resp.drag_delta();
            v += (d.x - d.y) / if fine { 900.0 } else { 220.0 };
        }
        if resp.hovered() {
            let s = ui.input(|i| i.smooth_scroll_delta.y);
            v += s / 600.0;
        }
        if resp.double_clicked() {
            v = default_of(&self.params, t);
        }
        let v = v.clamp(0.0, 1.0);
        if (v - self.params.get_target(t)).abs() > 1e-6 {
            self.params.set_target(t, v);
            self.dirty = true;
        }
        // body
        let a0 = std::f32::consts::PI * 0.75;
        let sweep = std::f32::consts::PI * 1.5;
        let ring = radius + 10.0;
        p.add(arc(center, ring, a0, a0 + sweep, Stroke::new(4.0, RAISED)));
        if v > 0.001 {
            let col = if self.mapper.learning() == Some(t) { LEARN } else { ACCENT };
            p.add(arc(center, ring, a0, a0 + sweep * v, Stroke::new(4.0, col)));
        }
        p.circle_filled(center + vec2(0.0, 3.0), radius, Color32::from_black_alpha(90));
        p.circle_filled(center, radius, RAISED);
        p.circle_filled(center - vec2(0.0, radius * 0.08), radius * 0.86, Color32::from_rgb(0x2e, 0x27, 0x20));
        p.circle_stroke(center, radius, Stroke::new(1.0, LINE));
        let ang = a0 + sweep * v;
        let dir = vec2(ang.cos(), ang.sin());
        p.line_segment([center + dir * radius * 0.35, center + dir * radius * 0.78], Stroke::new(3.0, if resp.hovered() || resp.dragged() { ACCENT } else { TEXT }));
        // value
        if compact {
            self.mapping_chip(ui, pos2(center.x, center.y + ring + 16.0), t);
        } else {
            let value = value_text(&self.params, t);
            p.text(pos2(center.x, center.y + ring + 18.0), Align2::CENTER_CENTER, value, FontId::proportional(15.0), TEXT);
            self.mapping_chip(ui, pos2(center.x, center.y + ring + 44.0), t);
        }
    }

    fn mode_row(&mut self, ui: &mut egui::Ui, row: Rect) {
        let p = ui.painter().clone();
        let cy = row.min.y + row.height() * 0.42;
        // velocity mode segmented switch, centred
        let meter_w = (row.width() * 0.2).clamp(100.0, 150.0);
        let seg_w = ((row.width() - 2.0 * (meter_w + 44.0)) / 3.0).clamp(70.0, 112.0);
        let seg_h = 42.0;
        let total = Rect::from_center_size(pos2(row.center().x, cy), vec2(seg_w * 3.0, seg_h));
        spaced(&p, pos2(total.center().x, total.min.y - 16.0), "VELOCITY", 11.5, MUTED, Align2::CENTER_CENTER);
        p.rect_filled(total, CornerRadius::same(21), RAISED);
        let mode = self.params.mode();
        for (i, m) in VelocityMode::ALL.iter().enumerate() {
            let r = Rect::from_min_size(pos2(total.min.x + seg_w * i as f32, total.min.y), vec2(seg_w, seg_h));
            let resp = ui.interact(r, ui.make_persistent_id(("mode", i)), Sense::click());
            let sel = *m == mode;
            if sel {
                p.rect_filled(r.shrink(3.0), CornerRadius::same(18), ACCENT);
            } else if resp.hovered() {
                p.rect_filled(r.shrink(3.0), CornerRadius::same(18), LINE);
            }
            p.text(r.center(), Align2::CENTER_CENTER, m.label(), FontId::proportional(15.0), if sel { BG } else { TEXT });
            if resp.clicked() {
                self.params.set_mode(*m);
                self.dirty = true;
            }
        }
        let organ = !self.params.instrument().velocity_sensitive();
        let desc = match mode {
            _ if organ => "Organ keys are on/off: velocity doesn't change the sound. The mode applies to the other instruments.",
            VelocityMode::Normal => "Standard response: play hard for loud. Touch knob shapes the curve.",
            VelocityMode::Light => "Soft playing still sounds full. Touch knob sets how light.",
            VelocityMode::Fixed => "Every note at the same level. The Level knob sets it.",
        };
        p.text(pos2(total.center().x, total.max.y + 16.0), Align2::CENTER_CENTER, desc, FontId::proportional(12.5), if organ { ACCENT } else { MUTED });
        self.mapping_chip(ui, pos2(total.center().x, total.max.y + 42.0), Target::VelocityMode);

        // left: output meter, voices, last velocity
        let left_x = row.min.x + 34.0;
        let peak = self.params.out_peak.get();
        let meter = Rect::from_min_size(pos2(left_x, cy - 4.0), vec2(meter_w, 8.0));
        spaced(&p, pos2(left_x, meter.min.y - 16.0), "OUTPUT", 11.5, MUTED, Align2::LEFT_CENTER);
        p.rect_filled(meter, CornerRadius::same(4), RAISED);
        let db = grand_piano::analysis::db(peak).max(-48.0);
        let frac = (db + 48.0) / 48.0;
        let mut f = meter;
        f.set_width(meter.width() * frac);
        let gr = self.params.limiter_gr_db.get();
        p.rect_filled(f, CornerRadius::same(4), if gr < -0.5 { LEARN } else { ACCENT });
        let voices = self.params.active_voices.load(Ordering::Relaxed);
        let vin = self.params.last_velocity_in.load(Ordering::Relaxed);
        let vout = self.params.last_velocity_out.load(Ordering::Relaxed);
        p.text(pos2(left_x, meter.max.y + 16.0), Align2::LEFT_CENTER, format!("{voices} voices"), FontId::proportional(12.5), MUTED);
        if vin > 0 {
            p.text(pos2(left_x, meter.max.y + 32.0), Align2::LEFT_CENTER, format!("velocity {vin} → {vout}"), FontId::proportional(12.0), MUTED);
        }
        if gr < -0.5 {
            p.text(pos2(left_x, meter.max.y + 48.0), Align2::LEFT_CENTER, format!("limiter {gr:.1} dB"), FontId::proportional(11.5), LEARN);
        }

        // right: sustain pedal
        let sx = row.max.x - 34.0;
        let sus = self.params.sustain.get();
        let bar = Rect::from_min_size(pos2(sx - meter_w, cy - 4.0), vec2(meter_w, 8.0));
        spaced(&p, pos2(bar.max.x, bar.min.y - 16.0), "SUSTAIN", 11.5, MUTED, Align2::RIGHT_CENTER);
        p.rect_filled(bar, CornerRadius::same(4), RAISED);
        let mut f = bar;
        f.set_width(bar.width() * sus);
        p.rect_filled(f, CornerRadius::same(4), ACCENT);
        let resp = ui.interact(bar.expand(8.0), ui.make_persistent_id("sustain"), Sense::click_and_drag());
        if resp.is_pointer_button_down_on() {
            if let Some(pp) = resp.interact_pointer_pos() {
                let v = ((pp.x - bar.min.x) / bar.width()).clamp(0.0, 1.0);
                self.params.sustain.set(v);
                self.sink.send(Event::Sustain(v));
            }
        } else if resp.drag_stopped() || resp.clicked() {
            self.params.sustain.set(0.0);
            self.sink.send(Event::Sustain(0.0));
        }
        self.mapping_chip(ui, pos2(bar.center().x + 40.0, bar.max.y + 22.0), Target::Sustain);

        // pitch bend indicator: centre mark, fill from the centre towards the bend
        let inst = self.params.instrument();
        let wheel = self.params.pitch_bend.get();
        let semis = self.params.bend_semis(inst);
        let by = (bar.max.y + 46.0).min(row.max.y - 8.0);
        let bb = Rect::from_min_size(pos2(bar.min.x + 48.0, by - 3.0), vec2(bar.width() - 48.0, 6.0));
        spaced(&p, pos2(bar.min.x, by), "BEND", 10.5, MUTED, Align2::LEFT_CENTER);
        p.rect_filled(bb, CornerRadius::same(3), RAISED);
        let c = bb.center().x;
        let ignored = inst == InstrumentKind::GrandPiano && !self.params.piano_bend.load(Ordering::Relaxed);
        if wheel.abs() > 0.002 {
            let x = c + wheel * bb.width() / 2.0;
            let fr = Rect::from_min_max(pos2(c.min(x), bb.min.y), pos2(c.max(x), bb.max.y));
            p.rect_filled(fr, CornerRadius::same(3), if ignored { FAINT } else { ACCENT });
        }
        p.line_segment([pos2(c, bb.min.y - 3.0), pos2(c, bb.max.y + 3.0)], Stroke::new(1.0, MUTED));
        let txt = if ignored { "off".to_string() } else if semis.abs() < 0.05 { "0".into() } else { format!("{semis:+.1}") };
        let bend_hover = ui.interact(bb.expand2(vec2(48.0, 6.0)), ui.make_persistent_id("bend_ind"), Sense::hover());
        p.text(pos2(bb.max.x, by - 12.0), Align2::RIGHT_CENTER, txt, FontId::proportional(10.5), MUTED);
        bend_hover.on_hover_text("Pitch wheel. Ranges are set per instrument in Settings; the piano ignores it unless allowed there.");
    }

    fn keyboard(&mut self, ui: &mut egui::Ui, r: Rect) {
        let p = ui.painter().clone();
        const LO: u8 = 21;
        const HI: u8 = 108;
        let is_black = |k: u8| matches!(k % 12, 1 | 3 | 6 | 8 | 10);
        let whites: Vec<u8> = (LO..=HI).filter(|k| !is_black(*k)).collect();
        let ww = r.width() / whites.len() as f32;
        let mut white_x = [0f32; 128];
        for (i, k) in whites.iter().enumerate() {
            white_x[*k as usize] = r.min.x + ww * i as f32;
        }
        let bw = ww * 0.62;
        let bh = r.height() * 0.62;
        let key_rect = |k: u8| -> Rect {
            if is_black(k) {
                let x = white_x[(k - 1) as usize] + ww - bw / 2.0;
                Rect::from_min_size(pos2(x, r.min.y), vec2(bw, bh))
            } else {
                Rect::from_min_size(pos2(white_x[k as usize], r.min.y), vec2(ww, r.height()))
            }
        };
        // shadow + frame
        p.rect_filled(r.expand(6.0), CornerRadius::same(10), PANEL);
        // keys outside the hardware keyboard's current span are drawn dimmer
        let (span_lo, span_hi) = self.sounding_span();
        let outside = |k: u8| (k as i32) < span_lo || (k as i32) > span_hi;
        let down = |k: u8| Params::key_is(&self.params.keys_down, k);
        let sounding = |k: u8| Params::key_is(&self.params.keys_sounding, k);
        for k in whites.iter().copied() {
            let kr = key_rect(k).shrink2(vec2(0.6, 0.0));
            let col = if down(k) {
                ACCENT
            } else if sounding(k) {
                lerp_color(KEY_WHITE, ACCENT, 0.28)
            } else {
                KEY_WHITE
            };
            let col = if outside(k) && !down(k) { lerp_color(col, BG, 0.42) } else { col };
            p.rect_filled(kr, CornerRadius { nw: 0, ne: 0, sw: 4, se: 4 }, col);
            if k % 12 == 0 {
                p.text(pos2(kr.center().x, kr.max.y - 11.0), Align2::CENTER_CENTER, format!("C{}", k / 12 - 1), FontId::proportional(9.5), FAINT);
            }
        }
        for k in (LO..=HI).filter(|k| is_black(*k)) {
            let kr = key_rect(k);
            let col = if down(k) {
                ACCENT
            } else if sounding(k) {
                lerp_color(KEY_BLACK, ACCENT, 0.35)
            } else {
                KEY_BLACK
            };
            let col = if outside(k) && !down(k) { lerp_color(col, BG, 0.5) } else { col };
            p.rect_filled(kr.translate(vec2(0.0, 2.0)), CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, Color32::from_black_alpha(70));
            p.rect_filled(kr, CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, col);
        }
        // a thin felt strip
        p.rect_filled(Rect::from_min_size(r.min - vec2(0.0, 3.0), vec2(r.width(), 4.0)), CornerRadius::ZERO, Color32::from_rgb(0x5a, 0x24, 0x22));
        // the hardware keyboard's span (inferred, plus the app's octave shift):
        // dim the keys outside it and mark it with an amber bar
        let lo_k = (span_lo.max(LO as i32)) as u8;
        let hi_k = (span_hi.min(HI as i32)) as u8;
        if lo_k <= hi_k {
            let x0 = key_rect(lo_k).min.x;
            let x1 = key_rect(hi_k).max.x;
            p.rect_filled(Rect::from_min_max(pos2(x0, r.min.y - 5.0), pos2(x1, r.min.y - 1.0)), CornerRadius::same(2), ACCENT);
        }
        // mouse playing
        let resp = ui.interact(r, ui.make_persistent_id("keyboard"), Sense::click_and_drag());
        let pointer_key = if resp.is_pointer_button_down_on() {
            resp.interact_pointer_pos().and_then(|pp| {
                (LO..=HI).filter(|k| is_black(*k)).find(|k| key_rect(*k).contains(pp)).or_else(|| whites.iter().copied().find(|k| key_rect(*k).contains(pp))).map(|k| {
                    let kr = key_rect(k);
                    let depth = ((pp.y - kr.min.y) / kr.height()).clamp(0.0, 1.0);
                    (k, (40.0 + 87.0 * depth) as u8)
                })
            })
        } else {
            None
        };
        let cur = pointer_key.map(|x| x.0);
        if cur != self.mouse_key {
            if let Some(k) = self.mouse_key {
                self.sink.send(Event::NoteOff { key: k });
            }
            if let Some((k, vel)) = pointer_key {
                self.sink.send(Event::NoteOn { key: k, vel });
            }
            self.mouse_key = cur;
        }
    }

    // ---------------------------------------------------------- windows

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        let mut restart_audio = false;
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, -30.0))
            .fixed_size(vec2(560.0, 0.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
                section(ui, "AUDIO OUTPUT");
                let current = self
                    .settings
                    .audio_device
                    .as_ref()
                    .and_then(|id| self.devices.iter().find(|d| &d.id == id).map(|d| d.name.clone()))
                    .unwrap_or_else(|| {
                        let def = self.devices.iter().find(|d| d.is_default).map(|d| d.name.clone()).unwrap_or_default();
                        format!("Windows default ({def})")
                    });
                egui::ComboBox::from_id_salt("dev").width(520.0).selected_text(current).show_ui(ui, |ui| {
                    if ui.selectable_label(self.settings.audio_device.is_none(), "Windows default").clicked() {
                        self.settings.audio_device = None;
                        restart_audio = true;
                    }
                    for d in self.devices.clone() {
                        let sel = self.settings.audio_device.as_deref() == Some(d.id.as_str());
                        if ui.selectable_label(sel, &d.name).clicked() {
                            self.settings.audio_device = Some(d.id.clone());
                            restart_audio = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Mode");
                    egui::ComboBox::from_id_salt("mode").width(200.0).selected_text(self.settings.audio_mode.label()).show_ui(ui, |ui| {
                        for m in AudioMode::ALL {
                            if ui.selectable_label(self.settings.audio_mode == m, m.label()).clicked() {
                                self.settings.audio_mode = m;
                                restart_audio = true;
                            }
                        }
                    });
                    ui.label("Buffer");
                    let rate = self.audio.as_ref().map(|a| a.info.sample_rate).unwrap_or(48000) as f32;
                    egui::ComboBox::from_id_salt("buf")
                        .width(150.0)
                        .selected_text(format!("{} ({:.1} ms)", self.settings.buffer_frames, self.settings.buffer_frames as f32 / rate * 1000.0))
                        .show_ui(ui, |ui| {
                            for b in [32u32, 64, 96, 128, 192, 256, 384, 512, 1024] {
                                if ui.selectable_label(self.settings.buffer_frames == b, format!("{b} ({:.1} ms)", b as f32 / rate * 1000.0)).clicked() {
                                    self.settings.buffer_frames = b;
                                    restart_audio = true;
                                }
                            }
                        });
                });
                if let Some(a) = &self.audio {
                    let i = &a.info;
                    let mut lines = vec![
                        format!("{} · {}", i.device_name, i.format),
                        format!(
                            "{} · {} frames per callback ({:.2} ms) · WASAPI buffer {} · driver latency {:.1} ms",
                            i.mode.label(),
                            i.period_frames,
                            i.period_frames as f32 / i.sample_rate as f32 * 1000.0,
                            i.buffer_frames,
                            i.stream_latency_ms
                        ),
                        format!("Estimated output latency ~{:.1} ms · late callbacks: {}", i.estimated_latency_ms, a.glitches.load(Ordering::Relaxed)),
                    ];
                    if let Some((def, fund, min, max)) = i.engine_periods {
                        lines.push(format!("Driver engine periods: default {def}, min {min}, max {max}, step {fund} frames"));
                    }
                    if !i.note.is_empty() {
                        lines.push(i.note.clone());
                    }
                    for l in lines {
                        ui.label(egui::RichText::new(l).color(MUTED).size(12.5));
                    }
                } else if let Some(e) = &self.audio_error {
                    ui.label(egui::RichText::new(format!("Audio error: {e}")).color(LEARN));
                }
                ui.label(egui::RichText::new("Exclusive mode is lowest latency but takes the device over (other apps go silent). If you hear crackles, raise the buffer.").color(FAINT).size(12.0));

                section(ui, "MIDI INPUT");
                ui.horizontal(|ui| {
                    let sel = self.settings.midi_port.clone().unwrap_or_else(|| "Automatic (Oxygen)".into());
                    egui::ComboBox::from_id_salt("midi").width(360.0).selected_text(sel).show_ui(ui, |ui| {
                        if ui.selectable_label(self.settings.midi_port.is_none(), "Automatic (Oxygen)").clicked() {
                            self.settings.midi_port = None;
                            self.midi.disconnect();
                            self.dirty = true;
                        }
                        for name in self.midi.ports.clone() {
                            if ui.selectable_label(self.settings.midi_port.as_deref() == Some(name.as_str()), &name).clicked() {
                                self.settings.midi_port = Some(name.clone());
                                if let Err(e) = self.midi.connect(&name) {
                                    self.midi.error = Some(e.to_string());
                                }
                                self.dirty = true;
                            }
                        }
                    });
                    if ui.button("Rescan").clicked() {
                        self.midi.disconnect();
                        self.midi.poll(self.settings.midi_port.clone().as_deref(), true);
                    }
                });
                let status = match (&self.midi.connected, &self.midi.error) {
                    (Some(n), _) => format!("Connected: {n}"),
                    (None, Some(e)) => format!("Not connected: {e}"),
                    (None, None) if self.midi.ports.is_empty() => "No MIDI keyboards found. Plug one in; it connects automatically.".into(),
                    _ => "Not connected".into(),
                };
                ui.label(egui::RichText::new(status).color(MUTED).size(12.5));

                section(ui, "KEYBOARD");
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 18.0), egui::Label::new(egui::RichText::new("Transpose").color(TEXT)));
                    let mut tr = self.params.transpose.load(Ordering::Relaxed);
                    if ui.add(egui::DragValue::new(&mut tr).range(-12..=12).suffix(" semitones")).changed() {
                        self.params.transpose.store(tr, Ordering::Relaxed);
                        self.dirty = true;
                    }
                    if ui.small_button("Reset").clicked() {
                        self.params.transpose.store(0, Ordering::Relaxed);
                        self.dirty = true;
                    }
                    ui.label(egui::RichText::new("Added to the octave shift (main screen); both apply to notes from the keyboard.").color(MUTED).size(12.0));
                });

                section(ui, "MOOG-STYLE SYNTH");
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 18.0), egui::Label::new(egui::RichText::new("Voices").color(TEXT)));
                    let cur = self.params.voice_mode();
                    for m in VoiceMode::ALL {
                        if ui.selectable_label(cur == m, m.label()).clicked() {
                            self.params.set_voice_mode(m);
                            self.dirty = true;
                        }
                    }
                    ui.label(egui::RichText::new("Also on the main screen. Poly plays chords; Mono is one note with legato and glide.").color(MUTED).size(12.0));
                });
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(110.0, 18.0), egui::Label::new(egui::RichText::new("Note priority").color(TEXT)));
                    let cur = self.params.note_priority();
                    for pr in NotePriority::ALL {
                        if ui.selectable_label(cur == pr, pr.label()).clicked() {
                            self.params.set_note_priority(pr);
                            self.dirty = true;
                        }
                    }
                    ui.label(egui::RichText::new("Mono only: which held key sounds (Last = newest, High = highest, Low = lowest).").color(MUTED).size(12.0));
                });

                section(ui, "PITCH BEND");
                ui.label(egui::RichText::new("Semitones up and down per instrument; 0 = off. The organ's is off by default, like the original.").color(MUTED).size(12.0));
                for inst in InstrumentKind::ALL {
                    let i = inst.id() as usize;
                    ui.horizontal(|ui| {
                        ui.add_sized(vec2(110.0, 18.0), egui::Label::new(egui::RichText::new(inst.label()).color(TEXT)));
                        if inst == InstrumentKind::GrandPiano {
                            let mut on = self.params.piano_bend.load(Ordering::Relaxed);
                            if ui.checkbox(&mut on, "Allow (a real piano can't bend)").changed() {
                                self.params.piano_bend.store(on, Ordering::Relaxed);
                                self.dirty = true;
                            }
                            if !on {
                                return;
                            }
                        }
                        let mut up = self.params.bend_up[i].load(Ordering::Relaxed) as i32;
                        let mut down = self.params.bend_down[i].load(Ordering::Relaxed) as i32;
                        ui.label(egui::RichText::new("up").color(MUTED));
                        let a = ui.add(egui::DragValue::new(&mut up).range(0..=12).suffix(" st"));
                        ui.label(egui::RichText::new("down").color(MUTED));
                        let b = ui.add(egui::DragValue::new(&mut down).range(0..=12).suffix(" st"));
                        if a.changed() || b.changed() {
                            self.params.bend_up[i].store(up as u8, Ordering::Relaxed);
                            self.params.bend_down[i].store(down as u8, Ordering::Relaxed);
                            self.dirty = true;
                        }
                        if ui.small_button("±2").clicked() {
                            self.params.bend_up[i].store(2, Ordering::Relaxed);
                            self.params.bend_down[i].store(2, Ordering::Relaxed);
                            self.dirty = true;
                        }
                        if ui.small_button("up 2 / down 12").clicked() {
                            self.params.bend_up[i].store(2, Ordering::Relaxed);
                            self.params.bend_down[i].store(12, Ordering::Relaxed);
                            self.dirty = true;
                        }
                    });
                }

                section(ui, "CONTROLLER MAPPING");
                egui::ScrollArea::vertical().id_salt("mapping").max_height(250.0).show(ui, |ui| {
                for t in Target::ALL {
                    let ccs = self.mapper.ccs_for(t);
                    ui.horizontal(|ui| {
                        ui.add_sized(vec2(110.0, 18.0), egui::Label::new(egui::RichText::new(t.label()).color(TEXT)));
                        let txt = if ccs.is_empty() { "—".to_string() } else { ccs.iter().map(|c| format!("CC {c}")).collect::<Vec<_>>().join(", ") };
                        ui.add_sized(vec2(160.0, 18.0), egui::Label::new(egui::RichText::new(txt).color(MUTED)));
                        let learning = self.mapper.learning() == Some(t);
                        if ui.button(if learning { "Waiting… (cancel)" } else { "Learn" }).clicked() {
                            if learning { self.mapper.cancel_learn() } else { self.mapper.start_learn(t) }
                        }
                        if ui.button("Clear").clicked() {
                            self.mapper.clear_target(t);
                            self.dirty = true;
                        }
                    });
                }
                });
                ui.horizontal(|ui| {
                    if ui.button("Reset to Oxygen 49 defaults").clicked() {
                        self.mapper.reset_to_defaults();
                        self.dirty = true;
                    }
                    let last = self.mapper.last_cc.load(Ordering::Relaxed);
                    if last >= 0 {
                        ui.label(egui::RichText::new(format!("Last control moved: CC {last}")).color(MUTED).size(12.5));
                    }
                });
                ui.label(egui::RichText::new(format!("Settings file: {}", grand_piano::settings::settings_path().display())).color(FAINT).size(11.5));
            });
        self.show_settings = open;
        if restart_audio {
            self.dirty = true;
            self.save_now();
            self.start_audio();
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_about;
        egui::Window::new("About")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, -30.0))
            .fixed_size(vec2(520.0, 0.0))
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(format!("Grand Piano {}", grand_piano::VERSION)).size(18.0).color(TEXT));
                ui.label(egui::RichText::new("A simple, low-latency sampled grand for MIDI keyboards.").color(MUTED));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(grand_piano::ATTRIBUTION).color(TEXT).size(13.0));
                ui.hyperlink_to("Creative Commons Attribution 3.0", "https://creativecommons.org/licenses/by/3.0/");
                ui.hyperlink_to("FreePats: Salamander Grand Piano", "https://freepats.zenvoid.org/Piano/acoustic-grand-piano.html");
                ui.add_space(6.0);
                if let Some(i) = &self.instrument {
                    ui.label(egui::RichText::new(format!("{} samples loaded · {} MB in memory · 16 velocity layers · release samples", i.samples.len(), i.memory_bytes / 1_000_000)).color(MUTED).size(12.0));
                }
                ui.label(egui::RichText::new("Shimmer (mod wheel): a stereo chorus whose depth and mix follow the wheel, with a gentle stereo tremolo in the top half.").color(MUTED).size(12.0));
                ui.label(egui::RichText::new("The Moog-style Synth, String Machine, Electric Piano, Organ and Polysynth are synthesized in real time (no samples). On the synths, Tone is the filter cutoff and the mod wheel adds vibrato; on the Electric Piano, Tone is brightness and the wheel is tremolo; on the Organ, Tone is drive and the wheel switches the rotating speaker between slow and fast. Names such as Moog, Rhodes, Wurlitzer, Hammond, B3, Leslie, Juno, Prophet and Oberheim only describe the kind of instrument; this app has no connection with their makers, and the names are their owners' trademarks.").color(MUTED).size(12.0));
                ui.label(egui::RichText::new("Tips: drag or scroll a knob; Shift for fine; double-click to reset.").color(MUTED).size(12.0));
            });
        self.show_about = open;
    }
}

// ---------------------------------------------------------------- helpers

fn default_of(p: &Params, t: Target) -> f32 {
    let inst = p.instrument();
    let mono = &grand_piano::synth::MONO_PRESETS[p.preset_of(InstrumentKind::MonoSynth)];
    let poly = &grand_piano::poly::POLY_PRESETS[p.preset_of(InstrumentKind::Polysynth)];
    match t {
        Target::Volume => params::DEFAULT_VOLUME,
        Target::Tone => match inst {
            InstrumentKind::GrandPiano => params::DEFAULT_TONE,
            InstrumentKind::MonoSynth => mono.cutoff,
            InstrumentKind::StringMachine => grand_piano::strings::STRING_PRESETS[p.preset_of(inst)].cutoff,
            InstrumentKind::ElectricPiano => grand_piano::ep::EP_PRESETS[p.preset_of(inst)].tone,
            InstrumentKind::Organ => grand_piano::organ::ORGAN_PRESETS[p.preset_of(inst)].drive,
            InstrumentKind::Polysynth => poly.cutoff,
        },
        Target::Reverb => params::DEFAULT_REVERB,
        Target::Modulation => params::DEFAULT_MOD,
        Target::Touch if p.mode() == VelocityMode::Fixed => (params::DEFAULT_FIXED - 1.0) / 126.0,
        Target::Touch => params::DEFAULT_TOUCH,
        Target::Resonance if inst == InstrumentKind::Polysynth => poly.resonance,
        Target::Resonance => mono.resonance,
        Target::Glide => mono.glide,
        Target::Tempo => (params::DEFAULT_TEMPO - params::TEMPO_MIN) / (params::TEMPO_MAX - params::TEMPO_MIN),
        _ => 0.0,
    }
}

fn value_text(p: &Params, t: Target) -> String {
    let v = p.get_target(t);
    let inst = p.instrument();
    let synth = inst.is_synth();
    match t {
        Target::Tone if inst == InstrumentKind::ElectricPiano => format!("{:.0}%", v * 100.0),
        Target::Tone if inst == InstrumentKind::Organ => if v < 0.005 { "clean".into() } else { format!("{:.0}%", v * 100.0) },
        Target::Modulation if inst == InstrumentKind::Organ => if p.leslie_fast.load(Ordering::Relaxed) { "fast".into() } else { "slow".into() },
        Target::Tone if synth => {
            let hz = grand_piano::synth::cutoff_hz(v);
            if hz >= 1000.0 { format!("{:.1} kHz", hz / 1000.0) } else { format!("{hz:.0} Hz") }
        }
        Target::Modulation if synth => if v < 0.005 { "off".into() } else { format!("{:.0}%", v * 100.0) },
        Target::Resonance => if v >= 0.97 { "self-osc".into() } else { format!("{:.0}%", v * 100.0) },
        Target::Glide => {
            let secs = grand_piano::synth::glide_secs(v);
            if p.voice_mode() == VoiceMode::Poly {
                "mono only".into()
            } else if secs < 0.0005 {
                "off".into()
            } else {
                format!("{:.0} ms", secs * 1000.0)
            }
        }
        Target::Tempo => format!("{:.0} BPM", p.tempo_bpm()),
        Target::Volume => {
            let db = params::volume_db(v);
            if db.is_infinite() { "off".into() } else { format!("{db:+.1} dB") }
        }
        Target::Tone => {
            if v < 0.48 {
                format!("warm {:.0}%", (0.5 - v) * 200.0)
            } else if v > 0.52 {
                format!("bright {:.0}%", (v - 0.5) * 200.0)
            } else {
                "neutral".into()
            }
        }
        Target::Reverb => format!("{:.0}%", v * 100.0),
        Target::Modulation => if v < 0.005 { "off".into() } else { format!("{:.0}%", v * 100.0) },
        Target::Touch => match p.mode() {
            VelocityMode::Fixed => format!("velocity {:.0}", p.fixed_velocity.get()),
            VelocityMode::Light => format!("light {:.0}%", v * 100.0),
            VelocityMode::Normal => {
                if (v - 0.5).abs() < 0.02 {
                    "linear".into()
                } else if v > 0.5 {
                    format!("lighter {:.0}%", (v - 0.5) * 200.0)
                } else {
                    format!("heavier {:.0}%", (0.5 - v) * 200.0)
                }
            }
        },
        _ => String::new(),
    }
}

fn arc(c: Pos2, r: f32, a0: f32, a1: f32, stroke: Stroke) -> Shape {
    let n = (((a1 - a0).abs() * r / 4.0) as usize).clamp(4, 128);
    let pts: Vec<Pos2> = (0..=n).map(|i| {
        let a = a0 + (a1 - a0) * i as f32 / n as f32;
        c + vec2(a.cos(), a.sin()) * r
    }).collect();
    Shape::line(pts, stroke)
}

/// The largest font size from `max` down to `min` (0.5 steps) at which `text` fits in `width`.
fn fit_size(p: &egui::Painter, text: &str, max: f32, min: f32, width: f32) -> f32 {
    let mut size = max;
    while size > min && p.layout_no_wrap(text.to_string(), FontId::proportional(size), TEXT).size().x > width {
        size -= 0.5;
    }
    size
}

fn short_label(k: InstrumentKind) -> &'static str {
    match k {
        InstrumentKind::GrandPiano => "Piano",
        InstrumentKind::MonoSynth => "Moog-style",
        InstrumentKind::StringMachine => "Strings",
        InstrumentKind::ElectricPiano => "E-Piano",
        InstrumentKind::Organ => "Organ",
        InstrumentKind::Polysynth => "Polysynth",
    }
}

/// One line about each preset, for the preset buttons' tooltips.
fn preset_hint(inst: InstrumentKind, i: usize) -> &'static str {
    match (inst, i) {
        (InstrumentKind::MonoSynth, 0) => "Bass: punchy saws with a square sub-octave and a short filter snap.",
        (InstrumentKind::MonoSynth, 1) => "Lead: saw, square and an octave-up saw, with glide (in Mono).",
        (InstrumentKind::MonoSynth, _) => "Poly: two detuned saws and a triangle sub, slower envelopes.",
        (InstrumentKind::StringMachine, 0) => "Strings: bright 8' + 4' ensemble.",
        (InstrumentKind::StringMachine, 1) => "Phased: the ensemble through a slow phaser.",
        (InstrumentKind::StringMachine, _) => "Dark: 16' + 8', slower attack, lower cutoff.",
        (InstrumentKind::ElectricPiano, 0) => "Suitcase: warm tine piano with stereo autopan (wheel). Barks when you dig in.",
        (InstrumentKind::ElectricPiano, 1) => "Bright Tines: bell-like tine attack with a little FM-piano sparkle. Stereo autopan (wheel).",
        (InstrumentKind::ElectricPiano, _) => "Wurli: reedy, nasal reed piano that growls when played hard. Mono tremolo (wheel).",
        (InstrumentKind::Organ, 0) => "Jazz: drawbars 888000000, no percussion.",
        (InstrumentKind::Organ, 1) => "Gospel: 888800000 with 3rd-harmonic percussion.",
        (InstrumentKind::Organ, 2) => "Rock: all drawbars out (888888888), overdriven.",
        (InstrumentKind::Organ, _) => "Ballad: soft flutes (008800000), clean.",
        (InstrumentKind::Polysynth, 0) => "80s Pad: Juno-style saw + PWM pulse + sub, slow attack, chorus II.",
        (InstrumentKind::Polysynth, 1) => "Brass Stab: Prophet-style two saws with a snappy filter and a touch of poly-mod.",
        (InstrumentKind::Polysynth, 2) => "Synthwave Lead: Prophet-style saw + square an octave up, resonant, chorus I.",
        (InstrumentKind::Polysynth, 3) => "Juno Strings: PWM pulse ensemble through the Juno-style chorus I.",
        (InstrumentKind::Polysynth, _) => "OB Brass: Oberheim-style fat detuned saws through a 12 dB filter.",
        _ => "",
    }
}

fn signed(v: i32) -> String {
    if v > 0 { format!("+{v}") } else { v.to_string() }
}

/// MIDI note name with middle C (60) = C4.
fn note_name(k: i32) -> String {
    const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let k = k.clamp(0, 127);
    format!("{}{}", N[(k % 12) as usize], k / 12 - 1)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Letter-spaced caps.
fn spaced(p: &egui::Painter, pos: Pos2, text: &str, size: f32, color: Color32, align: Align2) {
    let font = FontId::proportional(size);
    let spacing = size * 0.22;
    let glyphs: Vec<_> = text.chars().map(|c| p.layout_no_wrap(c.to_string(), font.clone(), color)).collect();
    let width: f32 = glyphs.iter().map(|g| g.size().x + spacing).sum::<f32>() - spacing;
    let h = glyphs.iter().map(|g| g.size().y).fold(0.0, f32::max);
    let rect = align.anchor_size(pos, vec2(width, h));
    let mut x = rect.min.x;
    for g in glyphs {
        let w = g.size().x;
        p.galley(pos2(x, rect.min.y), g, color);
        x += w + spacing;
    }
}

fn icon_button(ui: &mut egui::Ui, r: Rect, glyph: &str) -> egui::Response {
    let resp = ui.interact(r, ui.make_persistent_id(("icon", glyph)), Sense::click());
    let p = ui.painter();
    p.circle(r.center(), r.width() / 2.0, if resp.hovered() { RAISED } else { PANEL }, Stroke::new(1.0, LINE));
    let col = if resp.hovered() { ACCENT } else { MUTED };
    if glyph == "gear" {
        // drawn gear: ring + 8 teeth (the default fonts have no gear glyph)
        let c = r.center();
        for i in 0..8 {
            let a = i as f32 * std::f32::consts::TAU / 8.0;
            let d = vec2(a.cos(), a.sin());
            p.line_segment([c + d * 5.0, c + d * 8.5], Stroke::new(2.6, col));
        }
        p.circle_stroke(c, 5.2, Stroke::new(2.0, col));
    } else {
        p.text(r.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(15.0), col);
    }
    resp
}

fn pill_button(ui: &mut egui::Ui, r: Rect, text: &str, primary: bool) -> egui::Response {
    let resp = ui.interact(r, ui.make_persistent_id(("pill", text)), Sense::click());
    let p = ui.painter();
    let fill = if primary { if resp.hovered() { lerp_color(ACCENT, TEXT, 0.2) } else { ACCENT } } else { RAISED };
    p.rect_filled(r, CornerRadius::same((r.height() / 2.0) as u8), fill);
    p.text(r.center(), Align2::CENTER_CENTER, text, FontId::proportional(14.0), if primary { BG } else { TEXT });
    resp
}

/// An on/off pill: a dot that lights amber when on, and a label.
fn toggle_pill(ui: &mut egui::Ui, r: Rect, text: &str, on: bool) -> egui::Response {
    let resp = ui.interact(r, ui.make_persistent_id(("toggle", text)), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same((r.height() / 2.0) as u8), if resp.hovered() { LINE } else { RAISED });
    let dot = pos2(r.min.x + 15.0, r.center().y);
    p.circle_filled(dot, 4.5, if on { ACCENT } else { FAINT });
    p.text(pos2(dot.x + 11.0, r.center().y), Align2::LEFT_CENTER, text, FontId::proportional(13.0), if on { TEXT } else { MUTED });
    resp
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(title).size(11.5).color(ACCENT).strong());
}

pub fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = CornerRadius::same(14);
    v.extreme_bg_color = RAISED;
    v.faint_bg_color = RAISED;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.inactive.bg_fill = RAISED;
    v.widgets.inactive.weak_bg_fill = RAISED;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.hovered.bg_fill = LINE;
    v.widgets.hovered.weak_bg_fill = LINE;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.bg_fill = ACCENT.gamma_multiply(0.6);
    v.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.6);
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(120) };
    v.window_shadow = egui::epaint::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(140) };
    ctx.set_visuals(v);
}

/// The window/taskbar icon: assets/icon-256.png (the same artwork as the .ico
/// embedded in the exe). Falls back to a drawn icon if it can't be decoded.
pub fn icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")).unwrap_or_else(|_| drawn_icon())
}

/// A small drawn icon: amber ring around three keys.
fn drawn_icon() -> egui::IconData {
    let n = 64usize;
    let mut rgba = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5 - 32.0, y as f32 + 0.5 - 32.0);
            let d = (fx * fx + fy * fy).sqrt();
            let mut c = [0u8, 0, 0, 0];
            if d < 31.0 {
                c = [0x1d, 0x19, 0x15, 255];
            }
            if (27.0..30.5).contains(&d) {
                c = [0xe6, 0xa9, 0x57, 255];
            }
            // keys
            if (-14.0..14.0).contains(&fx) && (-12.0..14.0).contains(&fy) {
                let col = ((fx + 14.0) / 9.33) as i32;
                let gap = ((fx + 14.0) % 9.33) < 1.2;
                if !gap {
                    c = if col == 1 { [0xe6, 0xa9, 0x57, 255] } else { [0xec, 0xe5, 0xd8, 255] };
                }
            }
            rgba[(y * n + x) * 4..(y * n + x) * 4 + 4].copy_from_slice(&c);
        }
    }
    egui::IconData { rgba, width: n as u32, height: n as u32 }
}
