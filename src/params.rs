//! Parameters shared between the UI, the MIDI thread and the audio thread.
//! Everything is lock-free (atomics), so the audio thread never blocks.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};

#[derive(Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }
    #[inline]
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    #[inline]
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed)
    }
}

/// Things a MIDI controller can be mapped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Target {
    Volume,
    Tone,
    Reverb,
    Modulation,
    Touch,
    Sustain,
    VelocityMode,
}

impl Target {
    pub const ALL: [Target; 7] = [
        Target::Volume,
        Target::Tone,
        Target::Reverb,
        Target::Modulation,
        Target::Touch,
        Target::Sustain,
        Target::VelocityMode,
    ];
    pub fn id(self) -> u8 {
        Target::ALL.iter().position(|t| *t == self).unwrap() as u8
    }
    pub fn from_id(id: u8) -> Option<Target> {
        Target::ALL.get(id as usize).copied()
    }
    pub fn label(self) -> &'static str {
        match self {
            Target::Volume => "Volume",
            Target::Tone => "Tone",
            Target::Reverb => "Room",
            Target::Modulation => "Shimmer",
            Target::Touch => "Touch",
            Target::Sustain => "Sustain",
            Target::VelocityMode => "Velocity mode",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VelocityMode {
    /// Standard: what the keyboard sends, shaped by the Touch knob (centre = linear).
    Normal,
    /// Easy: soft playing still gives medium-loud notes (concave curve + raised floor).
    Light,
    /// Every note at one level, set by the Touch knob.
    Fixed,
}

impl VelocityMode {
    pub const ALL: [VelocityMode; 3] = [VelocityMode::Normal, VelocityMode::Light, VelocityMode::Fixed];
    pub fn id(self) -> u8 {
        match self {
            VelocityMode::Normal => 0,
            VelocityMode::Light => 1,
            VelocityMode::Fixed => 2,
        }
    }
    pub fn from_id(id: u8) -> VelocityMode {
        match id {
            0 => VelocityMode::Normal,
            2 => VelocityMode::Fixed,
            _ => VelocityMode::Light,
        }
    }
    pub fn next(self) -> VelocityMode {
        VelocityMode::from_id((self.id() + 1) % 3)
    }
    pub fn label(self) -> &'static str {
        match self {
            VelocityMode::Normal => "Normal",
            VelocityMode::Light => "Light",
            VelocityMode::Fixed => "Fixed",
        }
    }
}

/// Velocity curve. `touch` is the Touch knob (0..1) for Normal/Light; `fixed` is the
/// Fixed-mode level (1..127). Returns 1..=127.
pub fn shape_velocity(mode: VelocityMode, touch: f32, fixed: f32, vel: u8) -> u8 {
    let v = (vel.max(1) as f32) / 127.0;
    let out = match mode {
        VelocityMode::Normal => {
            // touch 0.5 = linear; higher = lighter (concave), lower = heavier (convex)
            let gamma = 2f32.powf((0.5 - touch) * 2.0);
            127.0 * v.powf(gamma)
        }
        VelocityMode::Light => {
            // touch 0.5 (default): floor 50, gamma 0.5. Range: floor 32..68, gamma 0.75..0.25
            let floor = 32.0 + 36.0 * touch;
            let gamma = 0.75 - 0.5 * touch;
            floor + (127.0 - floor) * v.powf(gamma)
        }
        VelocityMode::Fixed => fixed,
    };
    out.round().clamp(1.0, 127.0) as u8
}

/// Master volume knob position (0..1) -> linear gain. 0.8 = 0 dB, 1.0 = +12 dB.
pub fn volume_gain(pos: f32) -> f32 {
    if pos <= 0.001 {
        0.0
    } else {
        let db = -48.0 + 60.0 * pos;
        10f32.powf(db / 20.0)
    }
}

pub fn volume_db(pos: f32) -> f32 {
    if pos <= 0.001 { f32::NEG_INFINITY } else { -48.0 + 60.0 * pos }
}

pub struct Params {
    pub volume: AtomicF32,
    pub tone: AtomicF32,
    pub reverb: AtomicF32,
    pub modulation: AtomicF32,
    pub touch_normal: AtomicF32,
    pub touch_light: AtomicF32,
    pub fixed_velocity: AtomicF32, // 1..127
    pub velocity_mode: AtomicU8,
    pub sustain: AtomicF32, // 0..1, last value received (display only; engine gets events)

    // ---- telemetry (audio/MIDI -> UI) ----
    pub keys_down: [AtomicU64; 2],
    pub keys_sounding: [AtomicU64; 2],
    pub active_voices: AtomicU32,
    pub out_peak: AtomicF32,
    pub limiter_gr_db: AtomicF32,
    pub midi_events: AtomicU32,
    pub last_velocity_in: AtomicU8,
    pub last_velocity_out: AtomicU8,
}

pub const DEFAULT_VOLUME: f32 = 0.8;
pub const DEFAULT_TONE: f32 = 0.5;
pub const DEFAULT_REVERB: f32 = 0.22;
pub const DEFAULT_MOD: f32 = 0.0;
pub const DEFAULT_TOUCH: f32 = 0.5;
pub const DEFAULT_FIXED: f32 = 100.0;

impl Default for Params {
    fn default() -> Self {
        Params {
            volume: AtomicF32::new(DEFAULT_VOLUME),
            tone: AtomicF32::new(DEFAULT_TONE),
            reverb: AtomicF32::new(DEFAULT_REVERB),
            modulation: AtomicF32::new(DEFAULT_MOD),
            touch_normal: AtomicF32::new(DEFAULT_TOUCH),
            touch_light: AtomicF32::new(DEFAULT_TOUCH),
            fixed_velocity: AtomicF32::new(DEFAULT_FIXED),
            velocity_mode: AtomicU8::new(VelocityMode::Light.id()),
            sustain: AtomicF32::new(0.0),
            keys_down: Default::default(),
            keys_sounding: Default::default(),
            active_voices: AtomicU32::new(0),
            out_peak: AtomicF32::new(0.0),
            limiter_gr_db: AtomicF32::new(0.0),
            midi_events: AtomicU32::new(0),
            last_velocity_in: AtomicU8::new(0),
            last_velocity_out: AtomicU8::new(0),
        }
    }
}

impl Params {
    pub fn mode(&self) -> VelocityMode {
        VelocityMode::from_id(self.velocity_mode.load(Ordering::Relaxed))
    }
    pub fn set_mode(&self, m: VelocityMode) {
        self.velocity_mode.store(m.id(), Ordering::Relaxed)
    }
    /// The Touch knob position (0..1) for the current mode.
    pub fn touch_pos(&self) -> f32 {
        match self.mode() {
            VelocityMode::Normal => self.touch_normal.get(),
            VelocityMode::Light => self.touch_light.get(),
            VelocityMode::Fixed => (self.fixed_velocity.get() - 1.0) / 126.0,
        }
    }
    pub fn set_touch_pos(&self, p: f32) {
        let p = p.clamp(0.0, 1.0);
        match self.mode() {
            VelocityMode::Normal => self.touch_normal.set(p),
            VelocityMode::Light => self.touch_light.set(p),
            VelocityMode::Fixed => self.fixed_velocity.set(1.0 + 126.0 * p),
        }
    }
    pub fn shape(&self, vel: u8) -> u8 {
        let m = self.mode();
        let touch = match m {
            VelocityMode::Normal => self.touch_normal.get(),
            _ => self.touch_light.get(),
        };
        shape_velocity(m, touch, self.fixed_velocity.get(), vel)
    }
    /// Get a knob-style target's position (0..1).
    pub fn get_target(&self, t: Target) -> f32 {
        match t {
            Target::Volume => self.volume.get(),
            Target::Tone => self.tone.get(),
            Target::Reverb => self.reverb.get(),
            Target::Modulation => self.modulation.get(),
            Target::Touch => self.touch_pos(),
            Target::Sustain => self.sustain.get(),
            Target::VelocityMode => self.mode().id() as f32 / 2.0,
        }
    }
    pub fn set_target(&self, t: Target, v: f32) {
        let v = v.clamp(0.0, 1.0);
        match t {
            Target::Volume => self.volume.set(v),
            Target::Tone => self.tone.set(v),
            Target::Reverb => self.reverb.set(v),
            Target::Modulation => self.modulation.set(v),
            Target::Touch => self.set_touch_pos(v),
            Target::Sustain => self.sustain.set(v),
            Target::VelocityMode => self.set_mode(VelocityMode::from_id((v * 2.0).round() as u8)),
        }
    }
    pub fn key_bit(arr: &[AtomicU64; 2], key: u8, on: bool) {
        let (w, b) = ((key / 64) as usize, key % 64);
        if on {
            arr[w].fetch_or(1 << b, Ordering::Relaxed);
        } else {
            arr[w].fetch_and(!(1 << b), Ordering::Relaxed);
        }
    }
    pub fn key_is(arr: &[AtomicU64; 2], key: u8) -> bool {
        let (w, b) = ((key / 64) as usize, key % 64);
        arr[w].load(Ordering::Relaxed) & (1 << b) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_mode_is_loud_for_soft_playing() {
        // soft playing (vel 30) in Light mode lands in the medium-loud range
        let v = shape_velocity(VelocityMode::Light, 0.5, 100.0, 30);
        assert!(v >= 85, "{v}");
        // still dynamic: hard playing is louder than soft
        assert!(shape_velocity(VelocityMode::Light, 0.5, 100.0, 127) > v);
        assert_eq!(shape_velocity(VelocityMode::Light, 0.5, 100.0, 127), 127);
        // monotonic
        let mut last = 0;
        for vel in 1..=127 {
            let o = shape_velocity(VelocityMode::Light, 0.5, 100.0, vel);
            assert!(o >= last);
            last = o;
        }
    }

    #[test]
    fn normal_mode_centre_is_linear() {
        for vel in [1u8, 20, 64, 100, 127] {
            assert_eq!(shape_velocity(VelocityMode::Normal, 0.5, 100.0, vel), vel);
        }
        // lighter touch raises soft notes
        assert!(shape_velocity(VelocityMode::Normal, 1.0, 100.0, 40) > 40);
        assert!(shape_velocity(VelocityMode::Normal, 0.0, 100.0, 40) < 40);
    }

    #[test]
    fn fixed_mode() {
        for vel in [1u8, 64, 127] {
            assert_eq!(shape_velocity(VelocityMode::Fixed, 0.5, 100.0, vel), 100);
        }
    }

    #[test]
    fn volume_taper() {
        assert!((volume_gain(0.8) - 1.0).abs() < 1e-4);
        assert!((20.0 * volume_gain(1.0).log10() - 12.0).abs() < 1e-3);
        assert_eq!(volume_gain(0.0), 0.0);
    }

    #[test]
    fn touch_knob_follows_mode() {
        let p = Params::default();
        p.set_mode(VelocityMode::Fixed);
        p.set_touch_pos(1.0);
        assert_eq!(p.shape(10), 127);
        p.set_mode(VelocityMode::Light);
        assert!((p.touch_pos() - 0.5).abs() < 1e-6);
    }
}
