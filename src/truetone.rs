//! True Tone approximation and manual tint: decides the white point (Kelvin) and tint shift.

use crate::sensor::Ambient;

pub const NEUTRAL_K: f64 = 6500.0;
pub const WARMEST_K: f64 = 3000.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Off,
    Manual,
    Sensor,
    TimeOfDay,
}

pub struct TrueTone {
    pub tt_on: bool,
    pub manual_on: bool,
    pub warmth: f32,
    pub shift: f32,
    cur_k: f64,
}

impl TrueTone {
    pub fn new(tt_on: bool, manual_on: bool, warmth: f32, shift: f32) -> Self {
        Self { tt_on, manual_on, warmth, shift, cur_k: NEUTRAL_K }
    }

    pub fn cur_k(&self) -> f64 {
        self.cur_k
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

    /// One 100 ms step. Returns the Kelvin/shift to show and what drove it. `locked` (a
    /// reference mode is active) forces neutral.
    pub fn tick(&mut self, ambient: Option<&Ambient>, hour: f64, locked: bool) -> (f64, f64, Source) {
        let (target, shift, src) = if locked {
            (NEUTRAL_K, 0.0, Source::Off)
        } else if self.manual_on {
            (self.warmth as f64, self.shift as f64, Source::Manual)
        } else if self.tt_on {
            match ambient {
                Some(Ambient { lux, cct: Some(c) }) if *lux >= 1.0 => (c.clamp(WARMEST_K, NEUTRAL_K), 0.0, Source::Sensor),
                _ => (NEUTRAL_K - Self::night_factor(hour) * (NEUTRAL_K - 3600.0), 0.0, Source::TimeOfDay),
            }
        } else {
            (NEUTRAL_K, 0.0, Source::Off)
        };
        // True Tone eases toward the target; manual and off snap.
        self.cur_k = if src == Source::Sensor || src == Source::TimeOfDay {
            self.cur_k + (target - self.cur_k) * 0.2
        } else {
            target
        };
        (self.cur_k, shift, src)
    }
}
