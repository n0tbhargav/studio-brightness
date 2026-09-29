//! Tiny `key=value` settings file in %APPDATA%\StudioBrightness.

use std::path::PathBuf;

pub struct Settings {
    pub tt_mode: i32,
    pub tt_strength: i32,
    pub tt_warmth: i32,
    pub autostart: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { tt_mode: 0, tt_strength: 80, tt_warmth: 5000, autostart: false }
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
        let n = v.trim().parse::<i32>().unwrap_or(0);
        match k.trim() {
            "tt_mode" => s.tt_mode = n.clamp(0, 2),
            "tt_strength" => s.tt_strength = n.clamp(0, 100),
            "tt_warmth" => s.tt_warmth = n.clamp(3000, 6500),
            "autostart" => s.autostart = n != 0,
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
            "tt_mode={}\ntt_strength={}\ntt_warmth={}\nautostart={}\n",
            s.tt_mode, s.tt_strength, s.tt_warmth, s.autostart as i32
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
            let value: Vec<u16> = format!("\"{}\" --minimized", exe.display())
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key,
                name,
                REG_SZ.0,
                Some(value.as_ptr() as *const _),
                (value.len() * 2) as u32,
            );
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, key, name);
        }
    }
}
