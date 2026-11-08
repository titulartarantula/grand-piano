//! Octave shift and keyboard-range tracking.
//!
//! - `NoteShifter` applies the app's own octave shift and transpose to
//!   incoming MIDI notes. It remembers, per held key, which note it actually
//!   sent, so changing the octave while a key is down still releases the right
//!   note (no stuck notes). This part is exact.
//! - `RangeDetector` *infers* where the hardware keyboard's 49-key span sits.
//!   The Oxygen 49's own Octave buttons shift the note numbers inside the
//!   keyboard and send no MIDI message, so the app can't be told; it can only
//!   watch the notes that arrive. It keeps the notes of the last few seconds and,
//!   when a note falls outside the current span, picks the nearest octave shift
//!   that contains them.

/// The Oxygen 49's default span: C2..C6 (MIDI 36..84, with middle C = 60 = C4).
pub const HW_LOWEST: i32 = 36;
pub const HW_KEYS: i32 = 49;
/// Octave shifts the detector considers (the Oxygen 49 offers 3 down / 4 up).
pub const HW_SHIFT_MIN: i32 = -3;
pub const HW_SHIFT_MAX: i32 = 4;
/// How long a played note counts for the inference.
pub const HW_WINDOW_US: u64 = 6_000_000;

pub const OCTAVE_MIN: i32 = -3;
pub const OCTAVE_MAX: i32 = 3;
pub const TRANSPOSE_MAX: i32 = 12;

/// Applies octave/transpose to incoming notes and remembers what each held key sent.
pub struct NoteShifter {
    /// raw key -> note sent (-1 = not held)
    sent: [i16; 128],
}

impl Default for NoteShifter {
    fn default() -> Self {
        NoteShifter { sent: [-1; 128] }
    }
}

impl NoteShifter {
    /// Note-on for `raw` with the current shift (semitones). Returns the notes
    /// to release first (a still-held strike of the same raw key) and the note
    /// to play (None if the shifted note is outside 0..127).
    pub fn note_on(&mut self, raw: u8, shift: i32) -> (Option<u8>, Option<u8>) {
        let raw = (raw & 127) as usize;
        let release = if self.sent[raw] >= 0 { Some(self.sent[raw] as u8) } else { None };
        let n = raw as i32 + shift;
        if (0..=127).contains(&n) {
            self.sent[raw] = n as i16;
            (release, Some(n as u8))
        } else {
            self.sent[raw] = -1;
            (release, None)
        }
    }

    /// Note-off for `raw`: the note that key actually started, whatever the shift is now.
    pub fn note_off(&mut self, raw: u8) -> Option<u8> {
        let raw = (raw & 127) as usize;
        let s = self.sent[raw];
        self.sent[raw] = -1;
        if s >= 0 { Some(s as u8) } else { None }
    }

    pub fn clear(&mut self) {
        self.sent = [-1; 128];
    }
}

/// Infers the hardware keyboard's octave shift from the notes it sends.
pub struct RangeDetector {
    notes: [(u8, u64); 32],
    len: usize,
    next: usize,
    shift: i32,
    seen: bool,
}

impl Default for RangeDetector {
    fn default() -> Self {
        RangeDetector { notes: [(0, 0); 32], len: 0, next: 0, shift: 0, seen: false }
    }
}

impl RangeDetector {
    /// Current inferred shift (octaves) and whether any note has been seen yet.
    pub fn shift(&self) -> (i32, bool) {
        (self.shift, self.seen)
    }

    pub fn span(shift: i32) -> (i32, i32) {
        let lo = HW_LOWEST + 12 * shift;
        (lo, lo + HW_KEYS - 1)
    }

    fn fits(shift: i32, key: i32) -> bool {
        let (lo, hi) = Self::span(shift);
        key >= lo && key <= hi
    }

    /// Records a raw (unshifted) note-on at `now_us`; returns the shift.
    pub fn note(&mut self, raw: u8, now_us: u64) -> i32 {
        self.seen = true;
        self.notes[self.next] = (raw, now_us);
        self.next = (self.next + 1) % self.notes.len();
        self.len = (self.len + 1).min(self.notes.len());
        let key = raw as i32;
        if Self::fits(self.shift, key) {
            return self.shift;
        }
        // recent notes, newest first
        let mut recent: Vec<i32> = Vec::with_capacity(self.len);
        for i in 0..self.len {
            let idx = (self.next + self.notes.len() - 1 - i) % self.notes.len();
            let (k, t) = self.notes[idx];
            if now_us.saturating_sub(t) <= HW_WINDOW_US {
                recent.push(k as i32);
            }
        }
        // use as many recent notes as some shift can hold (the newest always counts)
        for take in (1..=recent.len()).rev() {
            let set = &recent[..take];
            let best = (HW_SHIFT_MIN..=HW_SHIFT_MAX)
                .filter(|s| set.iter().all(|k| Self::fits(*s, *k)))
                .min_by_key(|s| ((s - self.shift).abs(), s.abs()));
            if let Some(s) = best {
                self.shift = s;
                return s;
            }
        }
        // a note no 49-key span can reach from these shifts: leave it
        self.shift
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octave_change_while_held_releases_the_right_note() {
        let mut s = NoteShifter::default();
        // play C4 with Oct +1 -> C5 sounds
        assert_eq!(s.note_on(60, 12), (None, Some(72)));
        // octave changed to -1 while the key is held; a second key uses the new shift
        assert_eq!(s.note_on(64, -12), (None, Some(52)));
        // releasing the first key releases what it started (72), not 48
        assert_eq!(s.note_off(60), Some(72));
        assert_eq!(s.note_off(64), Some(52));
        // a second note-off for the same key does nothing
        assert_eq!(s.note_off(60), None);
    }

    #[test]
    fn retrigger_without_release_and_range_limits() {
        let mut s = NoteShifter::default();
        assert_eq!(s.note_on(60, 0), (None, Some(60)));
        // same raw key again (no note-off in between) at a new shift: release the old first
        assert_eq!(s.note_on(60, 12), (Some(60), Some(72)));
        assert_eq!(s.note_off(60), Some(72));
        // shifted outside MIDI's range: nothing plays, and the release is a no-op
        assert_eq!(s.note_on(120, 36), (None, None));
        assert_eq!(s.note_off(120), None);
        assert_eq!(s.note_on(5, -12), (None, None));
        // transpose by semitones
        assert_eq!(s.note_on(60, 3), (None, Some(63)));
        s.clear();
        assert_eq!(s.note_off(60), None);
    }

    #[test]
    fn detects_the_hardware_octave() {
        let mut d = RangeDetector::default();
        assert_eq!(d.shift(), (0, false));
        let t = 1_000_000;
        // notes inside C2..C6 keep the default
        for k in [36u8, 60, 84] {
            assert_eq!(d.note(k, t), 0);
        }
        assert_eq!(d.shift(), (0, true));
        // after the player presses Octave + on the keyboard, notes arrive 12 higher.
        // 96 doesn't fit C2..C6. No span holds 36 (a second ago) and 96 together,
        // so the oldest note is dropped and 60..96 gives +1 (48..96)
        assert_eq!(d.note(96, t + 1_000_000), 1);
        // later, only the new range is played
        let t2 = t + 10_000_000;
        assert_eq!(d.note(50, t2), 1);
        assert_eq!(d.note(96, t2 + 100), 1);
        // two octaves down: the lowest key now sends 12
        assert_eq!(d.note(12, t2 + 20_000_000), -2);
        // inside the current span: no change even near the edge
        assert_eq!(d.note(60, t2 + 20_000_100), -2);
        // the smallest shift that holds every recent note is chosen
        let mut d = RangeDetector::default();
        d.note(90, 0);
        d.note(100, 10);
        // 90 alone -> +1 (48..96); 100 doesn't fit, +2 (60..108) holds both
        assert_eq!(d.shift().0, 2);
    }
}
