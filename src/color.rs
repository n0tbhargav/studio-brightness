//! Host-side white-point shift via the GPU gamma ramp (what Night Light / f.lux do).
//! Needs no elevation. Applied only to Apple displays, found by their EDID PnP id "APP".

use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::ColorSystem::SetDeviceGammaRamp;

pub const NEUTRAL_K: f64 = 6500.0;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

/// Approximate RGB of a black-body white at `k` Kelvin (Tanner Helland's fit), 0..=255.
fn kelvin_rgb(k: f64) -> [f64; 3] {
    let t = k.clamp(1000.0, 10000.0) / 100.0;
    let r = if t <= 66.0 { 255.0 } else { 329.698727446 * (t - 60.0).powf(-0.1332047592) };
    let g = if t <= 66.0 {
        99.4708025861 * t.ln() - 161.1195681661
    } else {
        288.1221695283 * (t - 60.0).powf(-0.0755148492)
    };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.5177312231 * (t - 10.0).ln() - 305.0447927307
    };
    [r.clamp(0.0, 255.0), g.clamp(0.0, 255.0), b.clamp(0.0, 255.0)]
}

/// Per-channel multipliers relative to 6500 K (6500 K with shift 0 is the identity ramp).
/// `shift` runs -50 (green) to +50 (magenta).
pub fn multipliers(k: f64, shift: f64) -> [f64; 3] {
    let want = kelvin_rgb(k);
    let base = kelvin_rgb(NEUTRAL_K);
    let mut m = [want[0] / base[0], want[1] / base[1], want[2] / base[2]];
    let s = shift / 50.0;
    if s > 0.0 {
        m[1] *= 1.0 - 0.25 * s;
    } else {
        m[0] *= 1.0 + 0.15 * s;
        m[2] *= 1.0 + 0.15 * s;
    }
    let peak = m.iter().cloned().fold(f64::MIN, f64::max);
    [m[0] / peak, m[1] / peak, m[2] / peak]
}

/// Device names (e.g. `\\.\DISPLAY2`) of attached adapters that drive an Apple monitor.
pub fn apple_display_names() -> Vec<String> {
    let mut found = Vec::new();
    unsafe {
        let mut i = 0;
        loop {
            let mut adapter = DISPLAY_DEVICEW { cb: size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
            if !EnumDisplayDevicesW(PCWSTR::null(), i, &mut adapter, 0).as_bool() {
                break;
            }
            i += 1;
            if adapter.StateFlags.0 & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP.0 == 0 {
                continue;
            }
            let mut j = 0;
            loop {
                let mut mon = DISPLAY_DEVICEW { cb: size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
                if !EnumDisplayDevicesW(PCWSTR(adapter.DeviceName.as_ptr()), j, &mut mon, 0).as_bool() {
                    break;
                }
                j += 1;
                if from_wide(&mon.DeviceID).to_ascii_uppercase().starts_with("MONITOR\\APP") {
                    found.push(from_wide(&adapter.DeviceName));
                    break;
                }
            }
        }
    }
    found
}

/// Sets the white point of every Apple display. Returns how many accepted it.
pub fn apply(k: f64, shift: f64) -> usize {
    let m = multipliers(k, shift);
    let mut ramp = [[0u16; 256]; 3];
    for c in 0..3 {
        for i in 0..256 {
            ramp[c][i] = ((i as f64 * 257.0) * m[c]).round().clamp(0.0, 65535.0) as u16;
        }
    }
    let mut ok = 0;
    for name in apple_display_names() {
        let dev = wide(&name);
        let driver = wide("DISPLAY");
        unsafe {
            let hdc = CreateDCW(PCWSTR(driver.as_ptr()), PCWSTR(dev.as_ptr()), PCWSTR::null(), None);
            if hdc.is_invalid() {
                continue;
            }
            if SetDeviceGammaRamp(hdc, ramp.as_ptr() as *const c_void).as_bool() {
                ok += 1;
            }
            let _ = DeleteDC(hdc);
        }
    }
    ok
}

pub fn reset() {
    apply(NEUTRAL_K, 0.0);
}

pub fn describe() -> String {
    let names = apple_display_names();
    if names.is_empty() {
        "Apple displays for tinting: none found (no monitor with PnP id APP*)\n".into()
    } else {
        format!("Apple displays for tinting: {}\n", names.join(", "))
    }
}
