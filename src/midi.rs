//! MIDI input: auto-connects to the Oxygen (any port whose name contains
//! "oxygen"), with a manual picker, and handles unplug/replug by polling.

use crate::sink::EventSink;
use grand_piano::engine::Event;
use grand_piano::mapping::Mapper;
use grand_piano::keyrange::{NoteShifter, RangeDetector};
use grand_piano::params::{Params, bend_from_14bit, now_us};
use midir::{Ignore, MidiInput, MidiInputConnection};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub fn list_ports() -> Vec<String> {
    let Ok(mi) = MidiInput::new("Grand Piano scan") else { return Vec::new() };
    mi.ports().iter().filter_map(|p| mi.port_name(p).ok()).collect()
}

pub fn is_oxygen(name: &str) -> bool {
    name.to_ascii_lowercase().contains("oxygen")
}

/// Picks the port to use: the saved choice if present, else an Oxygen, else the first.
pub fn choose_port(ports: &[String], preferred: Option<&str>) -> Option<String> {
    if let Some(p) = preferred {
        if let Some(found) = ports.iter().find(|n| n.as_str() == p) {
            return Some(found.clone());
        }
    }
    ports.iter().find(|n| is_oxygen(n)).or(ports.first()).cloned()
}

pub struct Midi {
    conn: Option<MidiInputConnection<()>>,
    pub connected: Option<String>,
    pub ports: Vec<String>,
    pub error: Option<String>,
    last_scan: Instant,
    params: Arc<Params>,
    mapper: Arc<Mapper>,
    sink: Arc<EventSink>,
    repaint: Option<egui::Context>,
}

impl Midi {
    pub fn new(params: Arc<Params>, mapper: Arc<Mapper>, sink: Arc<EventSink>, repaint: Option<egui::Context>) -> Midi {
        Midi {
            conn: None,
            connected: None,
            ports: Vec::new(),
            error: None,
            last_scan: Instant::now() - Duration::from_secs(10),
            params,
            mapper,
            sink,
            repaint,
        }
    }

    pub fn disconnect(&mut self) {
        if let Some(c) = self.conn.take() {
            c.close();
        }
        self.connected = None;
        // release anything still held so notes don't hang
        self.sink.send(Event::AllNotesOff);
        self.sink.send(Event::Sustain(0.0));
    }

    pub fn connect(&mut self, name: &str) -> anyhow::Result<()> {
        self.disconnect();
        let mut mi = MidiInput::new("Grand Piano")?;
        mi.ignore(Ignore::All);
        let port = mi
            .ports()
            .into_iter()
            .find(|p| mi.port_name(p).ok().as_deref() == Some(name))
            .ok_or_else(|| anyhow::anyhow!("MIDI port '{name}' not found"))?;
        let params = self.params.clone();
        let mapper = self.mapper.clone();
        let sink = self.sink.clone();
        let repaint = self.repaint.clone();
        let mut keys = KeyState::default();
        let conn = mi
            .connect(
                &port,
                "grand-piano-in",
                move |_ts, msg, _| {
                    handle_message(msg, &params, &mapper, &sink, &mut keys);
                    if let Some(c) = &repaint {
                        c.request_repaint();
                    }
                },
                (),
            )
            .map_err(|e| anyhow::anyhow!("could not open '{name}': {e}"))?;
        self.conn = Some(conn);
        self.connected = Some(name.to_string());
        self.error = None;
        log::info!("MIDI connected: {name}");
        Ok(())
    }

    /// Call regularly from the UI thread. Rescans ports every 1.5 s (or now if `force`),
    /// drops a vanished port, and auto-connects when nothing is connected.
    pub fn poll(&mut self, preferred: Option<&str>, force: bool) {
        if !force && self.last_scan.elapsed() < Duration::from_millis(1500) {
            return;
        }
        self.last_scan = Instant::now();
        self.ports = list_ports();
        if let Some(c) = &self.connected {
            if !self.ports.contains(c) {
                log::warn!("MIDI port '{c}' disappeared");
                self.disconnect();
            }
        }
        if self.connected.is_none() {
            if let Some(p) = choose_port(&self.ports, preferred) {
                if let Err(e) = self.connect(&p) {
                    self.error = Some(e.to_string());
                }
            }
        }
    }
}

/// Per-connection note state: the app's octave shift per held key, and the
/// hardware range inference.
#[derive(Default)]
pub struct KeyState {
    shifter: NoteShifter,
    range: RangeDetector,
}

pub fn handle_message(msg: &[u8], params: &Params, mapper: &Mapper, sink: &EventSink, keys: &mut KeyState) {
    if msg.is_empty() {
        return;
    }
    let status = msg[0] & 0xF0;
    let d1 = msg.get(1).copied().unwrap_or(0) & 0x7F;
    let d2 = msg.get(2).copied().unwrap_or(0) & 0x7F;
    match status {
        0x90 if d2 > 0 => {
            let hw = keys.range.note(d1, now_us());
            params.hw_octave.store(hw, Ordering::Relaxed);
            params.hw_seen.store(true, Ordering::Relaxed);
            let (release, play) = keys.shifter.note_on(d1, params.note_shift());
            if let Some(k) = release {
                sink.send(Event::NoteOff { key: k });
            }
            if let Some(k) = play {
                sink.send(Event::NoteOn { key: k, vel: d2 });
            }
        }
        0x90 | 0x80 => {
            if let Some(k) = keys.shifter.note_off(d1) {
                sink.send(Event::NoteOff { key: k });
            }
        }
        0xB0 => {
            if d1 == 120 || d1 == 123 {
                keys.shifter.clear();
            }
            if let Some(ev) = mapper.handle_cc(params, d1, d2) {
                sink.send(ev);
            }
        }
        // pitch bend: 14-bit, LSB first; the engine smooths it per sample
        0xE0 => params.pitch_bend.set(bend_from_14bit(d1, d2)),
        _ => return,
    }
    params.midi_events.fetch_add(1, Ordering::Relaxed);
}
