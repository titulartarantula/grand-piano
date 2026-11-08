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
use grand_piano::params::{self, Params, Target, VelocityMode};
use grand_piano::settings::{AudioMode, Settings};
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
        let busy = !matches!(self.phase, Phase::Ready)
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
        // ---- keyboard at the bottom
        let kb_h = (inner.height() * 0.30).clamp(110.0, 190.0);
        let kb = Rect::from_min_max(pos2(inner.min.x, inner.max.y - kb_h), inner.max);
        self.keyboard(ui, kb);
        // ---- middle area: knobs + mode row
        let mid = Rect::from_min_max(pos2(inner.min.x, header.max.y + 14.0), pos2(inner.max.x, kb.min.y - 18.0));
        ui.painter().rect_filled(mid, CornerRadius::same(18), PANEL);
        let knob_h = (mid.height() * 0.62).max(150.0);
        let knob_area = Rect::from_min_size(mid.min, vec2(mid.width(), knob_h));
        let targets = [Target::Volume, Target::Tone, Target::Reverb, Target::Modulation, Target::Touch];
        let w = knob_area.width() / targets.len() as f32;
        for (i, t) in targets.iter().enumerate() {
            let r = Rect::from_min_size(pos2(knob_area.min.x + w * i as f32, knob_area.min.y), vec2(w, knob_h));
            self.knob(ui, r, *t);
        }
        let row = Rect::from_min_max(pos2(mid.min.x, knob_area.max.y), mid.max);
        self.mode_row(ui, row);
    }

    fn header(&mut self, ui: &mut egui::Ui, r: Rect) {
        let p = ui.painter().clone();
        spaced(&p, pos2(r.min.x, r.center().y - 7.0), "GRAND PIANO", 19.0, TEXT, Align2::LEFT_CENTER);
        p.text(pos2(r.min.x, r.center().y + 13.0), Align2::LEFT_CENTER, "Salamander Grand · Yamaha C5", FontId::proportional(12.0), MUTED);
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
        let label = match t {
            Target::Touch if mode == VelocityMode::Fixed => "LEVEL",
            Target::Touch => "TOUCH",
            other => match other {
                Target::Volume => "VOLUME",
                Target::Tone => "TONE",
                Target::Reverb => "ROOM",
                Target::Modulation => "SHIMMER",
                _ => "",
            },
        };
        let radius = (area.width() * 0.26).min(area.height() * 0.24).clamp(30.0, 58.0);
        let center = pos2(area.center().x, area.min.y + 34.0 + radius + 8.0);
        let p = ui.painter().clone();
        spaced(&p, pos2(center.x, area.min.y + 26.0), label, 11.5, MUTED, Align2::CENTER_CENTER);
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
            v = default_of(t, mode);
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
        let value = value_text(&self.params, t);
        p.text(pos2(center.x, center.y + ring + 18.0), Align2::CENTER_CENTER, value, FontId::proportional(15.0), TEXT);
        self.mapping_chip(ui, pos2(center.x, center.y + ring + 44.0), t);
    }

    fn mode_row(&mut self, ui: &mut egui::Ui, row: Rect) {
        let p = ui.painter().clone();
        let cy = row.min.y + row.height() * 0.42;
        // velocity mode segmented switch, centred
        let seg_w = 112.0;
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
        let desc = match mode {
            VelocityMode::Normal => "Standard response: play hard for loud. Touch knob shapes the curve.",
            VelocityMode::Light => "Soft playing still sounds full. Touch knob sets how light.",
            VelocityMode::Fixed => "Every note at the same level. The Level knob sets it.",
        };
        p.text(pos2(total.center().x, total.max.y + 16.0), Align2::CENTER_CENTER, desc, FontId::proportional(12.5), MUTED);
        self.mapping_chip(ui, pos2(total.center().x, total.max.y + 42.0), Target::VelocityMode);

        // left: output meter, voices, last velocity
        let left_x = row.min.x + 34.0;
        let peak = self.params.out_peak.get();
        let meter = Rect::from_min_size(pos2(left_x, cy - 4.0), vec2(150.0, 8.0));
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
        let info = if vin > 0 { format!("{voices} voices   ·   velocity {vin} → {vout}") } else { format!("{voices} voices") };
        p.text(pos2(left_x, meter.max.y + 16.0), Align2::LEFT_CENTER, info, FontId::proportional(12.5), MUTED);
        if gr < -0.5 {
            p.text(pos2(left_x, meter.max.y + 34.0), Align2::LEFT_CENTER, format!("limiter {gr:.1} dB"), FontId::proportional(11.5), LEARN);
        }

        // right: sustain pedal
        let sx = row.max.x - 34.0;
        let sus = self.params.sustain.get();
        let bar = Rect::from_min_size(pos2(sx - 150.0, cy - 4.0), vec2(150.0, 8.0));
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
            p.rect_filled(kr.translate(vec2(0.0, 2.0)), CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, Color32::from_black_alpha(70));
            p.rect_filled(kr, CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, col);
        }
        // a thin felt strip
        p.rect_filled(Rect::from_min_size(r.min - vec2(0.0, 3.0), vec2(r.width(), 4.0)), CornerRadius::ZERO, Color32::from_rgb(0x5a, 0x24, 0x22));
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

                section(ui, "CONTROLLER MAPPING");
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
                ui.label(egui::RichText::new("Tips: drag or scroll a knob; Shift for fine; double-click to reset.").color(MUTED).size(12.0));
            });
        self.show_about = open;
    }
}

// ---------------------------------------------------------------- helpers

fn default_of(t: Target, mode: VelocityMode) -> f32 {
    match t {
        Target::Volume => params::DEFAULT_VOLUME,
        Target::Tone => params::DEFAULT_TONE,
        Target::Reverb => params::DEFAULT_REVERB,
        Target::Modulation => params::DEFAULT_MOD,
        Target::Touch if mode == VelocityMode::Fixed => (params::DEFAULT_FIXED - 1.0) / 126.0,
        Target::Touch => params::DEFAULT_TOUCH,
        _ => 0.0,
    }
}

fn value_text(p: &Params, t: Target) -> String {
    let v = p.get_target(t);
    match t {
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

/// A small app icon: amber ring around three keys.
pub fn icon() -> egui::IconData {
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
