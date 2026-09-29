//! Tiny `key=value` settings file in %APPDATA%\StudioBrightness.

use std::path::PathBuf;

pub struct Settings {
    pub tt_on: bool,
    pub manual_on: bool,
    pub warmth: f32,
    pub shift: f32,
    pub auto_on: bool,
    pub bias: f32,
    pub autostart: bool,
    pub tint_mode: i32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { tt_on: false, manual_on: false, warmth: 4800.0, shift: 0.0, auto_on: false, bias: 0.0, autostart: false, tint_mode: 0 }
    }
}

fn path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("StudioBrightness").join("settings.txt")
}

pub fn load() -> Settings {
    let mut s = Settings::default();
    let Ok(text) = std::fs::read_to_string(path()) else { return s };
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let f = v.trim().parse::<f32>().unwrap_or(0.0);
        let b = f != 0.0;
        match k.trim() {
            "tt_on" => s.tt_on = b,
            "manual_on" => s.manual_on = b,
            "warmth" => s.warmth = f.clamp(3000.0, 6500.0),
            "shift" => s.shift = f.clamp(-50.0, 50.0),
            "auto_on" => s.auto_on = b,
            "bias" => s.bias = f.clamp(-40.0, 40.0),
            "autostart" => s.autostart = b,
            "tint_mode" => s.tint_mode = (f as i32).clamp(0, 2),
            _ => {}
        }
    }
    s
}

pub fn save(s: &Settings) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(
        p,
        format!(
            "tt_on={}\nmanual_on={}\nwarmth={}\nshift={}\nauto_on={}\nbias={}\nautostart={}\ntint_mode={}\n",
            s.tt_on as i32, s.manual_on as i32, s.warmth, s.shift, s.auto_on as i32, s.bias, s.autostart as i32, s.tint_mode
        ),
    );
}

/// Adds/removes the HKCU Run entry so the app starts (minimized to tray) at sign-in.
pub fn set_autostart(enable: bool) {
    use windows::core::w;
    use windows::Win32::System::Registry::*;
    let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let name = w!("StudioBrightness");
    unsafe {
        if enable {
            let Ok(exe) = std::env::current_exe() else { return };
            let value: Vec<u16> = format!("\"{}\" --minimized", exe.display()).encode_utf16().chain(std::iter::once(0)).collect();
            let _ = RegSetKeyValueW(HKEY_CURRENT_USER, key, name, REG_SZ.0, Some(value.as_ptr() as *const _), (value.len() * 2) as u32);
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, key, name);
        }
    }
}
