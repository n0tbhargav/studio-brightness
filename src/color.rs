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

// ---------------------------------------------------------------------------------------------
// Tint engine: gamma ramp first, with a click-through overlay window as a fallback for drivers
// (or Windows versions) that reject or ignore gamma ramps.
// ---------------------------------------------------------------------------------------------

use std::collections::HashMap;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::ColorSystem::GetDeviceGammaRamp;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Windows rejects ramps that stray too far from linear, so keep each channel above this.
const GAMMA_MIN_MULT: f64 = 0.55;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TintMode {
    Auto,
    Gamma,
    Overlay,
}

impl TintMode {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => TintMode::Gamma,
            2 => TintMode::Overlay,
            _ => TintMode::Auto,
        }
    }
    pub fn index(self) -> i32 {
        match self {
            TintMode::Auto => 0,
            TintMode::Gamma => 1,
            TintMode::Overlay => 2,
        }
    }
    pub fn next(self) -> Self {
        Self::from_index((self.index() + 1) % 3)
    }
    pub fn label(self) -> &'static str {
        match self {
            TintMode::Auto => "tint: auto",
            TintMode::Gamma => "tint: gamma",
            TintMode::Overlay => "tint: overlay",
        }
    }
}

struct Monitor {
    device: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

fn apple_monitors() -> Vec<Monitor> {
    let mut out = Vec::new();
    for device in apple_display_names() {
        let mut dm = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
        let w = wide(&device);
        let ok = unsafe { EnumDisplaySettingsW(PCWSTR(w.as_ptr()), ENUM_CURRENT_SETTINGS, &mut dm).as_bool() };
        if ok {
            let pos = unsafe { dm.Anonymous1.Anonymous2.dmPosition };
            out.push(Monitor { device, x: pos.x, y: pos.y, w: dm.dmPelsWidth as i32, h: dm.dmPelsHeight as i32 });
        }
    }
    out
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_ERASEBKGND {
            let brush = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if brush != 0 {
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                FillRect(HDC(wp.0 as *mut c_void), &rc, HBRUSH(brush as *mut c_void));
            }
            return LRESULT(1);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

fn create_overlay(m: &Monitor) -> Option<HWND> {
    unsafe {
        let hinst = GetModuleHandleW(PCWSTR::null()).ok()?;
        let class = w!("StudioBrightnessTint");
        let wc = WNDCLASSW { lpfnWndProc: Some(overlay_proc), hInstance: hinst.into(), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc); // fails harmlessly if already registered
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!(""),
            WS_POPUP,
            m.x,
            m.y,
            m.w,
            m.h,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .ok()
    }
}

fn set_overlay(hwnd: HWND, mult: [f64; 3]) {
    // Approximate "multiply by mult" with a translucent colour: choose the opacity so the
    // weakest channel maps to black, then split the remaining channels by their ratio.
    let min = mult.iter().cloned().fold(f64::MAX, f64::min);
    let a = (1.0 - min).clamp(0.0, 0.5);
    unsafe {
        if a < 0.01 {
            let _ = ShowWindow(hwnd, SW_HIDE);
            return;
        }
        let c = |m: f64| (0.8 * (m - min) / (1.0 - min).max(0.01) * 255.0).clamp(0.0, 255.0) as u32;
        let (r, g, b) = (c(mult[0]), c(mult[1]), c(mult[2]));
        let old = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        let brush = CreateSolidBrush(COLORREF(r | (g << 8) | (b << 16)));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, brush.0 as isize);
        if old != 0 {
            let _ = DeleteObject(HGDIOBJ(old as *mut c_void));
        }
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), (a * 255.0) as u8, LWA_ALPHA);
        let _ = InvalidateRect(Some(hwnd), None, true);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

/// Tries the gamma ramp on one display and confirms Windows kept it.
fn try_gamma(device: &str, ramp: &[[u16; 256]; 3]) -> bool {
    let dev = wide(device);
    let driver = wide("DISPLAY");
    unsafe {
        let hdc = CreateDCW(PCWSTR(driver.as_ptr()), PCWSTR(dev.as_ptr()), PCWSTR::null(), None);
        if hdc.is_invalid() {
            return false;
        }
        let mut ok = SetDeviceGammaRamp(hdc, ramp.as_ptr() as *const c_void).as_bool();
        if ok {
            let mut back = [[0u16; 256]; 3];
            ok = GetDeviceGammaRamp(hdc, back.as_mut_ptr() as *mut c_void).as_bool()
                && [64usize, 128, 255].iter().all(|&i| (0..3).all(|c| (back[c][i] as i32 - ramp[c][i] as i32).abs() < 1024));
        }
        let _ = DeleteDC(hdc);
        ok
    }
}

pub struct Tinter {
    overlays: HashMap<String, HWND>,
    pub note: String,
}

impl Tinter {
    pub fn new() -> Self {
        Self { overlays: HashMap::new(), note: "not applied yet".into() }
    }

    /// Applies the white point to every Apple display. Returns how many displays were tinted.
    pub fn apply(&mut self, k: f64, shift: f64, mode: TintMode) -> usize {
        let monitors = apple_monitors();
        if monitors.is_empty() {
            self.note = "no Apple display found to tint (needs a monitor with PnP id APP*)".into();
            return 0;
        }
        let m = multipliers(k, shift);
        let mg = [m[0].max(GAMMA_MIN_MULT), m[1].max(GAMMA_MIN_MULT), m[2].max(GAMMA_MIN_MULT)];
        let mut ramp = [[0u16; 256]; 3];
        for c in 0..3 {
            for i in 0..256 {
                ramp[c][i] = ((i as f64 * 257.0) * mg[c]).round().clamp(0.0, 65535.0) as u16;
            }
        }
        let neutral = (k - NEUTRAL_K).abs() < 1.0 && shift.abs() < 0.5;
        let (mut gamma_n, mut overlay_n) = (0, 0);
        for mon in &monitors {
            let gamma_ok = mode != TintMode::Overlay && try_gamma(&mon.device, &ramp);
            if gamma_ok {
                gamma_n += 1;
                if let Some(&h) = self.overlays.get(&mon.device) {
                    unsafe { let _ = ShowWindow(h, SW_HIDE); }
                }
                continue;
            }
            if mode == TintMode::Gamma {
                continue; // user forced gamma only
            }
            if neutral {
                if let Some(&h) = self.overlays.get(&mon.device) {
                    unsafe { let _ = ShowWindow(h, SW_HIDE); }
                }
                continue;
            }
            let hwnd = match self.overlays.get(&mon.device) {
                Some(&h) => Some(h),
                None => create_overlay(mon).inspect(|&h| {
                    self.overlays.insert(mon.device.clone(), h);
                }),
            };
            if let Some(h) = hwnd {
                set_overlay(h, m);
                overlay_n += 1;
            }
        }
        self.note = format!("{} display(s): gamma ramp on {gamma_n}, overlay on {overlay_n}", monitors.len());
        gamma_n + overlay_n
    }

    pub fn reset(&mut self) {
        self.apply(NEUTRAL_K, 0.0, TintMode::Gamma);
        for &h in self.overlays.values() {
            unsafe { let _ = ShowWindow(h, SW_HIDE); }
        }
    }
}
