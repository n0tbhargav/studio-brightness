//! Ambient light via the Windows Sensor API (works unelevated; no Apple driver needed).

use windows::Devices::Sensors::LightSensor;

pub struct Ambient {
    pub lux: f64,
    /// Correlated colour temperature in Kelvin, if the sensor reports chromaticity.
    pub cct: Option<f64>,
}

pub struct Sensor {
    dev: LightSensor,
    pub chromaticity: bool,
}

impl Sensor {
    pub fn new() -> Option<Self> {
        let dev = LightSensor::GetDefault().ok()?;
        let chromaticity = dev.IsChromaticitySupported().unwrap_or(false);
        Some(Self { dev, chromaticity })
    }

    pub fn id(&self) -> String {
        self.dev.DeviceId().map(|s| s.to_string()).unwrap_or_default()
    }

    pub fn read(&self) -> Option<Ambient> {
        let r = self.dev.GetCurrentReading().ok()?;
        let lux = r.IlluminanceInLux().ok()? as f64;
        let cct = if self.chromaticity {
            r.Chromaticity().ok().and_then(|c| mccamy_cct(c.X, c.Y))
        } else {
            None
        };
        Some(Ambient { lux, cct })
    }
}

/// McCamy's approximation from CIE 1931 xy; None for the all-zero "dark" report.
fn mccamy_cct(x: f64, y: f64) -> Option<f64> {
    if x <= 0.0 || y <= 0.0 || y >= 0.1858 + 0.9 {
        return None;
    }
    let n = (x - 0.3320) / (0.1858 - y);
    let cct = 437.0 * n.powi(3) + 3601.0 * n.powi(2) + 6861.0 * n + 5517.0;
    (1500.0..=15000.0).contains(&cct).then_some(cct)
}
