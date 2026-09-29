//! Ambient light via the Windows Sensor API (works unelevated; no Apple driver needed).
//!
//! Runs on its own thread: WinRT needs COM initialised on the calling thread, sensors only
//! produce readings once a report interval is set, and the UI thread must never block on them.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::core::HSTRING;
use windows::Devices::Enumeration::DeviceInformation;
use windows::Devices::Sensors::{LightSensor, LightSensorReadingChangedEventArgs};
use windows::Foundation::TypedEventHandler;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

#[derive(Clone, Copy)]
pub struct Ambient {
    pub lux: f64,
    /// Correlated colour temperature in Kelvin, if the sensor reports chromaticity.
    pub cct: Option<f64>,
}

#[derive(Default)]
struct Shared {
    ambient: Option<Ambient>,
    /// None while starting, Some(false) if no sensor exists, Some(true) once one is open.
    found: Option<bool>,
    log: String,
    last: String,
}

pub struct Sensor {
    shared: Arc<Mutex<Shared>>,
}

struct Open {
    id: String,
    dev: LightSensor,
    chromaticity: bool,
}

impl Sensor {
    pub fn start() -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let s2 = shared.clone();
        std::thread::spawn(move || worker(s2));
        Self { shared }
    }

    pub fn read(&self) -> Option<Ambient> {
        self.shared.lock().ok().and_then(|s| s.ambient)
    }

    /// None while still probing.
    pub fn found(&self) -> Option<bool> {
        self.shared.lock().ok().and_then(|s| s.found)
    }

    pub fn info(&self) -> String {
        match self.shared.lock() {
            Ok(s) => format!("{}last: {}\n", s.log, s.last),
            Err(_) => "sensor state unavailable\n".into(),
        }
    }
}

fn enumerate() -> windows::core::Result<Vec<(String, String)>> {
    let selector = LightSensor::GetDeviceSelector()?;
    let all = DeviceInformation::FindAllAsyncAqsFilter(&selector)?.join()?;
    let mut out = Vec::new();
    for i in 0..all.Size()? {
        let d = all.GetAt(i)?;
        out.push((d.Id()?.to_string(), d.Name()?.to_string()));
    }
    Ok(out)
}

fn is_apple(id: &str, name: &str) -> bool {
    let (i, n) = (id.to_ascii_lowercase(), name.to_ascii_lowercase());
    i.contains("vid_05ac") || n.contains("apple") || n.contains("studio display")
}

fn configure(dev: &LightSensor) {
    let min = dev.MinimumReportInterval().unwrap_or(0);
    let _ = dev.SetReportInterval(min.max(500));
    // Subscribing keeps some sensor drivers in continuous mode.
    let handler = TypedEventHandler::<LightSensor, LightSensorReadingChangedEventArgs>::new(|_, _| Ok(()));
    let _ = dev.ReadingChanged(&handler);
}

fn worker(sh: Arc<Mutex<Shared>>) {
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    let mut log = String::new();
    let mut open: Vec<Open> = Vec::new();

    match enumerate() {
        Ok(list) if list.is_empty() => log.push_str("enumeration: no light sensors\n"),
        Ok(list) => {
            for (id, name) in &list {
                let apple = is_apple(id, name);
                log.push_str(&format!("found{}: {name} [{id}]\n", if apple { " (Apple)" } else { "" }));
                if apple {
                    if let Ok(op) = LightSensor::FromIdAsync(&HSTRING::from(id.as_str())) {
                        if let Ok(dev) = op.join() {
                            let chromaticity = dev.IsChromaticitySupported().unwrap_or(false);
                            open.push(Open { id: id.clone(), dev, chromaticity });
                        }
                    }
                }
            }
        }
        Err(e) => log.push_str(&format!("enumeration failed: {e}\n")),
    }
    if open.is_empty() {
        match LightSensor::GetDefault() {
            Ok(dev) => {
                log.push_str("no Apple sensor found; using the default light sensor\n");
                let chromaticity = dev.IsChromaticitySupported().unwrap_or(false);
                let id = dev.DeviceId().map(|s| s.to_string()).unwrap_or_default();
                open.push(Open { id, dev, chromaticity });
            }
            Err(e) => log.push_str(&format!("GetDefault failed: {e}\n")),
        }
    }
    for o in &open {
        configure(&o.dev);
        log.push_str(&format!("using: {} (chromaticity {})\n", o.id, o.chromaticity));
    }
    if let Ok(mut s) = sh.lock() {
        s.found = Some(!open.is_empty());
        s.log = log;
    }
    if open.is_empty() {
        return;
    }

    loop {
        let mut best: Option<Ambient> = None;
        let mut last = String::new();
        for o in &open {
            match o.dev.GetCurrentReading() {
                Ok(r) => {
                    let lux = r.IlluminanceInLux().unwrap_or(0.0) as f64;
                    let cct = if o.chromaticity { r.Chromaticity().ok().and_then(|c| mccamy_cct(c.X, c.Y)) } else { None };
                    last.push_str(&format!("{lux:.1} lux, cct {cct:?}; "));
                    // With several sensors the brightest reading wins.
                    if best.map_or(true, |b| lux > b.lux) {
                        best = Some(Ambient { lux, cct });
                    }
                }
                Err(e) => last.push_str(&format!("read error {e}; ")),
            }
        }
        if let Ok(mut s) = sh.lock() {
            s.ambient = best;
            s.last = last;
        }
        std::thread::sleep(Duration::from_millis(500));
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
