//! Apple Studio Display brightness over USB HID feature reports.
//!
//! Feature report 0x01 = [id, u32 little-endian brightness, 0, 0], raw range 400..=60000.
//! Runs as a normal user: Windows lets any process open HID interfaces that aren't
//! keyboards/mice, so no driver or elevation is required.

use hidapi::{HidApi, HidDevice};

const VENDOR_ID: u16 = 0x05ac;
const PRODUCT_ID: u16 = 0x1114;
const INTERFACE: i32 = 7;
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

/// Opens every attached Studio Display's brightness interface.
fn open_all() -> Vec<HidDevice> {
    let Ok(api) = HidApi::new() else { return Vec::new() };
    api.device_list()
        .filter(|d| {
            d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID && d.interface_number() == INTERFACE
        })
        .filter_map(|d| d.open_device(&api).ok())
        .collect()
}

fn get(dev: &HidDevice) -> Option<i32> {
    let mut buf = [0u8; 7];
    buf[0] = 1;
    dev.get_feature_report(&mut buf).ok()?;
    Some(to_percent(u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]])))
}

fn set(dev: &HidDevice, percent: i32) {
    let r = to_raw(percent).to_le_bytes();
    let _ = dev.send_feature_report(&[1, r[0], r[1], r[2], r[3], 0, 0]);
}

pub fn set_all(percent: i32) {
    for dev in open_all() {
        set(&dev, percent);
    }
}

/// Nudges each display relative to its own current level.
pub fn adjust(delta: i32) {
    for dev in open_all() {
        if let Some(cur) = get(&dev) {
            set(&dev, cur + delta);
        }
    }
}
