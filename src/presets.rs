//! Apple reference modes (color presets) over the 0xFF20 vendor HID interface.
//!
//! Protocol per Studio Brightness++ / Boot Camp: usage 0x04 is a write-only enumeration cursor,
//! 0x06/0x08 give validity and a UTF-16 name for the cursor preset, 0x03 is the active preset.
//! Reports are built with Windows' HID parser (HidP_*) from the interface's preparsed data.

use std::ffi::c_void;

use hidapi::HidApi;
use windows::core::PCWSTR;
use windows::Win32::Devices::HumanInterfaceDevice::*;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

const VENDOR_ID: u16 = 0x05ac;
const PAGE: u16 = 0xFF20;
const GENERIC_RW: u32 = 0xC000_0000;

#[derive(Clone)]
pub struct Mode {
    pub index: u32,
    pub name: String,
}

pub struct Presets {
    handle: HANDLE,
    prep: PHIDP_PREPARSED_DATA,
    report_len: usize,
    pub modes: Vec<Mode>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn ok(status: windows::Win32::Foundation::NTSTATUS) -> bool {
    status == HIDP_STATUS_SUCCESS
}

fn feature_value_caps(prep: PHIDP_PREPARSED_DATA) -> Vec<HIDP_VALUE_CAPS> {
    unsafe {
        let mut caps = HIDP_CAPS::default();
        if !ok(HidP_GetCaps(prep, &mut caps)) || caps.NumberFeatureValueCaps == 0 {
            return Vec::new();
        }
        let mut n = caps.NumberFeatureValueCaps;
        let mut v = vec![HIDP_VALUE_CAPS::default(); n as usize];
        if !ok(HidP_GetValueCaps(HidP_Feature, v.as_mut_ptr(), &mut n, prep)) {
            return Vec::new();
        }
        v.truncate(n as usize);
        v
    }
}

fn usage_of(c: &HIDP_VALUE_CAPS) -> u16 {
    unsafe { if c.IsRange { c.Anonymous.Range.UsageMin } else { c.Anonymous.NotRange.Usage } }
}

fn utf16_name(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

impl Presets {
    /// Finds the display's 0xFF20 interface and reads the list of modes.
    pub fn open() -> Option<Self> {
        let api = HidApi::new().ok()?;
        for info in api.device_list().filter(|d| d.vendor_id() == VENDOR_ID && (0x1114..=0x1118).contains(&d.product_id())) {
            let Ok(path) = info.path().to_str() else { continue };
            let w = wide(path);
            let handle = unsafe {
                let mut h = CreateFileW(PCWSTR(w.as_ptr()), GENERIC_RW, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None);
                if h.is_err() {
                    h = CreateFileW(PCWSTR(w.as_ptr()), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None);
                }
                match h {
                    Ok(h) => h,
                    Err(_) => continue,
                }
            };
            let mut prep = PHIDP_PREPARSED_DATA::default();
            let got = unsafe { HidD_GetPreparsedData(handle, &mut prep) };
            if !got || prep.0 == 0 {
                unsafe { let _ = CloseHandle(handle); }
                continue;
            }
            if !feature_value_caps(prep).iter().any(|c| c.UsagePage == PAGE) {
                unsafe { let _ = HidD_FreePreparsedData(prep); let _ = CloseHandle(handle); }
                continue;
            }
            let mut caps = HIDP_CAPS::default();
            unsafe { let _ = HidP_GetCaps(prep, &mut caps); }
            let mut p = Presets { handle, prep, report_len: caps.FeatureReportByteLength as usize, modes: Vec::new() };
            p.enumerate();
            if !p.modes.is_empty() {
                return Some(p);
            }
        }
        None
    }

    fn feature_caps(&self) -> Vec<HIDP_VALUE_CAPS> {
        feature_value_caps(self.prep)
    }

    fn enumerate(&mut self) {
        let caps = self.feature_caps();
        let cursor_max = caps.iter().find(|c| c.UsagePage == PAGE && usage_of(c) == 0x04).map(|c| c.LogicalMax).unwrap_or(0);
        let bound = if cursor_max > 0 && cursor_max <= 128 { cursor_max } else { 64 };
        for i in 0..bound {
            // Cursor is write-only: send a clean zeroed report (a GET on it stalls on some models).
            let mut wr = vec![0u8; self.report_len];
            wr[0] = 0x04;
            if !self.set_usage(0x04, i as u32, &mut wr) || !self.send(&wr) {
                break;
            }
            let mut r5 = vec![0u8; self.report_len];
            r5[0] = 0x05;
            if !self.get(&mut r5) {
                break;
            }
            if self.usage_value(0x06, &r5).unwrap_or(0) == 0 {
                break;
            }
            let mut name = [0u8; 256];
            let got = unsafe {
                HidP_GetUsageValueArray(HidP_Feature, PAGE, Some(0), 0x08, &mut name, self.prep, &r5)
            };
            let name = if ok(got) { utf16_name(&name) } else { String::new() };
            self.modes.push(Mode { index: i as u32, name });
        }
    }

    fn set_usage(&self, usage: u16, v: u32, report: &mut [u8]) -> bool {
        unsafe { ok(HidP_SetUsageValue(HidP_Feature, PAGE, Some(0), usage, v, self.prep, report)) }
    }

    fn usage_value(&self, usage: u16, report: &[u8]) -> Option<u32> {
        let mut v = 0u32;
        unsafe { ok(HidP_GetUsageValue(HidP_Feature, PAGE, Some(0), usage, &mut v, self.prep, report)).then_some(v) }
    }

    fn send(&self, report: &[u8]) -> bool {
        unsafe { HidD_SetFeature(self.handle, report.as_ptr() as *const c_void, report.len() as u32) }
    }

    fn get(&self, report: &mut [u8]) -> bool {
        unsafe { HidD_GetFeature(self.handle, report.as_mut_ptr() as *mut c_void, report.len() as u32) }
    }

    pub fn active(&self) -> Option<u32> {
        let mut r3 = vec![0u8; self.report_len];
        r3[0] = 0x03;
        if !self.get(&mut r3) {
            return None;
        }
        self.usage_value(0x03, &r3)
    }

    pub fn set_active(&self, index: u32) -> bool {
        let mut r3 = vec![0u8; self.report_len];
        r3[0] = 0x03;
        self.set_usage(0x03, index, &mut r3) && self.send(&r3)
    }

    pub fn name_of(&self, index: u32) -> Option<&str> {
        self.modes.iter().find(|m| m.index == index).map(|m| m.name.as_str())
    }

    /// Modes named "Apple ..." are the general-use ones; anything else is a fixed-calibration
    /// reference mode, so brightness and tinting are locked (macOS behaves the same).
    pub fn locks_controls(&self, index: u32) -> bool {
        let general = |n: &str| n.starts_with("Apple Display") || n.starts_with("Apple XDR Display") || n.starts_with("Pro Display XDR");
        if !self.modes.iter().any(|m| general(&m.name)) {
            return false; // unknown naming scheme: never lock on a guess
        }
        self.name_of(index).is_some_and(|n| !general(n))
    }
}

impl Drop for Presets {
    fn drop(&mut self) {
        unsafe {
            let _ = HidD_FreePreparsedData(self.prep);
            let _ = CloseHandle(self.handle);
        }
    }
}
