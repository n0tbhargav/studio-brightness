//! Apple Studio Display brightness over USB HID feature reports.
//!
//! Feature report 0x01 = [id, u32 little-endian brightness, 0, 0], raw range 400..=60000.
//! Runs as a normal user: Windows lets any process open HID interfaces that aren't
//! keyboards/mice, so no driver or elevation is required.

use hidapi::{HidApi, HidDevice};

const VENDOR_ID: u16 = 0x05ac;
// Studio Display (2022) 0x1114, XDR 0x1116, Studio Display (2026) 0x1118; 0x1115/0x1117 seen in other tools.
const PRODUCT_IDS: std::ops::RangeInclusive<u16> = 0x1114..=0x1118;
const MIN_RAW: u32 = 400;
const MAX_RAW: u32 = 60000;

fn to_percent(raw: u32) -> i32 {
    let raw = raw.clamp(MIN_RAW, MAX_RAW);
    (((raw - MIN_RAW) as f64) * 100.0 / (MAX_RAW - MIN_RAW) as f64).round() as i32
}

fn to_raw(percent: i32) -> u32 {
    let p = percent.clamp(0, 100) as f64;
    MIN_RAW + (p * (MAX_RAW - MIN_RAW) as f64 / 100.0).round() as u32
}

fn get_raw(dev: &HidDevice) -> Option<u32> {
    let mut buf = [0u8; 7];
    buf[0] = 1;
    dev.get_feature_report(&mut buf).ok()?;
    Some(u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]))
}

fn set_raw(dev: &HidDevice, percent: i32) -> bool {
    let r = to_raw(percent).to_le_bytes();
    dev.send_feature_report(&[1, r[0], r[1], r[2], r[3], 0, 0]).is_ok()
}

/// Handles to every Studio Display HID collection that answers the brightness report.
/// Windows exposes each top-level collection as its own device path, so we probe
/// rather than trust the interface number.
#[derive(Default)]
pub struct Displays {
    devs: Vec<HidDevice>,
    pub pids: Vec<u16>,
}

impl Displays {
    pub fn new() -> Self {
        let mut d = Self::default();
        d.refresh();
        d
    }

    pub fn refresh(&mut self) {
        self.devs.clear();
        self.pids.clear();
        let Ok(api) = HidApi::new() else { return };
        for info in api
            .device_list()
            .filter(|d| d.vendor_id() == VENDOR_ID && PRODUCT_IDS.contains(&d.product_id()))
        {
            let Ok(dev) = info.open_device(&api) else { continue };
            if get_raw(&dev).is_some_and(|r| (MIN_RAW..=MAX_RAW).contains(&r)) {
                self.pids.push(info.product_id());
                self.devs.push(dev);
            }
        }
    }

    pub fn found(&self) -> bool {
        !self.devs.is_empty()
    }

    pub fn percent(&self) -> Option<i32> {
        self.devs.first().and_then(get_raw).map(to_percent)
    }

    pub fn set(&mut self, percent: i32) {
        if !self.devs.iter().all(|d| set_raw(d, percent)) {
            // A handle went stale (unplug / sleep): reopen and retry once.
            self.refresh();
            for d in &self.devs {
                set_raw(d, percent);
            }
        }
    }

    /// Nudges relative to the first display's current level.
    pub fn adjust(&mut self, delta: i32) {
        if let Some(cur) = self.percent() {
            self.set(cur + delta);
        }
    }

    pub fn status(&self) -> String {
        match self.pids.first() {
            None => "No Studio Display found. Connect it with its USB-C / Thunderbolt cable.".into(),
            Some(pid) => format!(
                "Studio Display connected (PID 0x{pid:04X}){}",
                if self.devs.len() > 1 { format!(", {} interfaces", self.devs.len()) } else { String::new() }
            ),
        }
    }
}

/// Human-readable dump of every Apple HID interface and the result of probing it.
pub fn diagnostics() -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let api = match HidApi::new() {
        Ok(a) => a,
        Err(e) => return format!("HidApi::new failed: {e}\n"),
    };
    let mut n = 0;
    for d in api.device_list().filter(|d| d.vendor_id() == VENDOR_ID) {
        n += 1;
        let _ = writeln!(
            out,
            "pid={:04x} iface={} usage_page={:04x} usage={:04x}\n  path={:?}",
            d.product_id(), d.interface_number(), d.usage_page(), d.usage(), d.path()
        );
        match d.open_device(&api) {
            Err(e) => {
                let _ = writeln!(out, "  open: FAILED {e}");
            }
            Ok(dev) => {
                let mut buf = [0u8; 7];
                buf[0] = 1;
                match dev.get_feature_report(&mut buf) {
                    Ok(len) => {
                        let _ = writeln!(out, "  open: ok; get_feature_report ok len={len} bytes={buf:02x?} -> {}%", to_percent(u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]])));
                    }
                    Err(e) => {
                        let _ = writeln!(out, "  open: ok; get_feature_report FAILED {e}");
                    }
                }
            }
        }
    }
    if n == 0 {
        out.push_str("No HID interfaces with vendor id 05ac found.\n");
    }
    out
}
