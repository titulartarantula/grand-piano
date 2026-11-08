//! A multi-producer front for the engine's single-producer event queue.
//! MIDI and UI threads lock briefly; the audio thread only pops (lock-free).

use grand_piano::engine::Event;
use std::sync::Mutex;

#[derive(Default)]
pub struct EventSink {
    producer: Mutex<Option<rtrb::Producer<Event>>>,
}

impl EventSink {
    /// Creates a fresh queue for a new engine and returns its consumer.
    pub fn new_queue(&self) -> rtrb::Consumer<Event> {
        let (p, c) = rtrb::RingBuffer::new(4096);
        *self.producer.lock().unwrap() = Some(p);
        c
    }
    pub fn send(&self, e: Event) {
        if let Some(p) = self.producer.lock().unwrap().as_mut() {
            let _ = p.push(e);
        }
    }
}
