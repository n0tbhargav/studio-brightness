//! True Tone approximation: decides the white point (Kelvin) the screen should show.

use crate::sensor::Ambient;

pub const NEUTRAL_K: f64 = 6500.0;
pub const WARMEST_K: f64 = 3000.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Auto,
    Manual,
}

impl Mode {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Mode::Auto,
            2 => Mode::Manual,
            _ => Mode::Off,
        }
    }
    pub fn index(self) -> i32 {
        match self {
            Mode::Off => 0,
            Mode::Auto => 1,
            Mode::Manual => 2,
        }
    }
}

pub struct TrueTone {
    pub mode: Mode,
    pub strength: i32,
    pub manual_k: i32,
    current_k: f64,
}

impl TrueTone {
    pub fn new(mode: Mode, strength: i32, manual_k: i32) -> Self {
        Self { mode, strength, manual_k, current_k: NEUTRAL_K }
    }

    pub fn current_k(&self) -> f64 {
        self.current_k
    }

    /// 0 in daytime, 1 at night, with smooth evening/morning ramps.
    fn night_factor(hour: f64) -> f64 {
        let ramp = |a: f64, b: f64| ((hour - a) / (b - a)).clamp(0.0, 1.0);
        if hour < 7.0 {
            1.0 - ramp(5.0, 7.0)
        } else if hour < 17.0 {
            0.0
        } else {
            ramp(17.0, 21.0)
        }
    }

    /// Returns the (smoothed) Kelvin to apply this tick, and a note on what drove it.
    pub fn tick(&mut self, ambient: Option<&Ambient>, hour: f64) -> (f64, &'static str) {
        let (target, note) = match self.mode {
            Mode::Off => (NEUTRAL_K, "off"),
            Mode::Manual => (self.manual_k as f64, "manual"),
            Mode::Auto => {
                let s = self.strength as f64 / 100.0;
                let (room_k, note) = match ambient {
                    Some(Ambient { lux, cct: Some(c) }) if *lux >= 1.0 => (c.clamp(WARMEST_K, NEUTRAL_K), "sensor"),
                    _ => (NEUTRAL_K - Self::night_factor(hour) * (NEUTRAL_K - 3600.0), "time of day"),
                };
                (NEUTRAL_K - (NEUTRAL_K - room_k) * s, note)
            }
        };
        // Ease toward the target so it never visibly jumps; manual/off snap.
        self.current_k = if self.mode == Mode::Auto {
            self.current_k + (target - self.current_k) * 0.2
        } else {
            target
        };
        (self.current_k, note)
    }
}
