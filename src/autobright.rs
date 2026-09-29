//! Auto brightness: a log-lux curve with a learned bias, 20% lux hysteresis and an
//! asymmetric ramp (brightens quicker than it dims), after Apple's behaviour.

pub struct AutoBright {
    pub on: bool,
    pub bias: f32,
    applied_lux: f64,
    target: Option<f32>,
}

pub fn base(lux: f64) -> f32 {
    (4.0 + 26.0 * (lux + 1.0).log10()).clamp(4.0, 100.0) as f32
}

impl AutoBright {
    pub fn new(on: bool, bias: f32) -> Self {
        Self { on, bias, applied_lux: 0.0, target: None }
    }

    pub fn curve_at(&self, lux: f64) -> f32 {
        (base(lux) + self.bias).clamp(2.0, 100.0)
    }

    pub fn invalidate(&mut self) {
        self.target = None;
    }

    /// The user moved the slider while Auto was on: shift the curve so it passes through here.
    pub fn learn(&mut self, value: f32, lux: f64) {
        self.bias = (value - base(lux)).clamp(-40.0, 40.0);
        self.target = None;
    }

    /// Advances `cur` toward the curve. Returns true if it moved.
    pub fn step(&mut self, lux: f64, cur: &mut f32) -> bool {
        let rel = (lux - self.applied_lux).abs() / self.applied_lux.max(1.0);
        if self.target.is_none() || rel > 0.2 {
            self.target = Some(self.curve_at(lux));
            self.applied_lux = lux;
        }
        let diff = self.target.unwrap() - *cur;
        if diff.abs() < 0.3 {
            return false;
        }
        *cur += diff * if diff > 0.0 { 0.08 } else { 0.03 };
        true
    }

    /// Curve as SVG path data in a 300x100 box (log lux 0.1..10000 across, 0..100% up).
    pub fn paths(&self) -> (String, String) {
        let mut line = String::new();
        let mut basel = String::new();
        for i in 0..=60 {
            let lux = 10f64.powf(-1.0 + i as f64 / 60.0 * 5.0);
            let x = i as f32 / 60.0 * 300.0;
            let cmd = if i == 0 { 'M' } else { 'L' };
            basel.push_str(&format!("{cmd}{x:.1} {:.1} ", 100.0 - base(lux)));
            line.push_str(&format!("{cmd}{x:.1} {:.1} ", 100.0 - self.curve_at(lux)));
        }
        (line, basel)
    }

    pub fn dot(lux: f64, bright: f32) -> (f32, f32) {
        let x = ((lux.max(0.1)).log10() + 1.0) / 5.0;
        (x as f32, 1.0 - bright / 100.0)
    }
}
