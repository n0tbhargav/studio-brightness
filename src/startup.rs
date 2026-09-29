//! Single instance, "launch again opens the flyout", and the Start menu shortcut.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, ERROR_ALREADY_EXISTS, WAIT_OBJECT_0};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

static SHOW_EVENT: AtomicIsize = AtomicIsize::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Returns false if another instance is running; that instance is asked to open its flyout.
pub fn claim_single_instance() -> bool {
    unsafe {
        let mutex_name = wide("Local\\StudioBrightness.Instance");
        let event_name = wide("Local\\StudioBrightness.Show");
        let m = CreateMutexW(None, true, PCWSTR(mutex_name.as_ptr()));
        let already = m.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS;
        if already {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            if let Ok(ev) = OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(event_name.as_ptr())) {
                let _ = SetEvent(ev);
                let _ = CloseHandle(ev);
            }
            return false;
        }
        // The mutex handle is intentionally leaked: it lives as long as the process.
        if let Ok(ev) = CreateEventW(None, false, false, PCWSTR(event_name.as_ptr())) {
            SHOW_EVENT.store(ev.0 as isize, Ordering::SeqCst);
        }
        true
    }
}

/// True once per request from a second launch (Start menu click, double-click, ...).
pub fn take_show_request() -> bool {
    let h = SHOW_EVENT.load(Ordering::SeqCst);
    h != 0 && unsafe { WaitForSingleObject(HANDLE(h as *mut _), 0) == WAIT_OBJECT_0 }
}

/// Creates or refreshes `Studio Brightness.lnk` in the per-user Start menu (no admin needed).
pub fn ensure_start_menu_shortcut() {
    std::thread::spawn(|| {
        let Some(appdata) = std::env::var_os("APPDATA") else { return };
        let Ok(exe) = std::env::current_exe() else { return };
        let dir = std::path::PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs");
        let lnk = dir.join("Studio Brightness.lnk");
        let _ = std::fs::create_dir_all(&dir);
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { return };
            let exe_s = HSTRING::from(exe.to_string_lossy().as_ref());
            let _ = link.SetPath(&exe_s);
            let _ = link.SetIconLocation(&exe_s, 0);
            let _ = link.SetDescription(&HSTRING::from("Studio Display brightness, True Tone and color modes"));
            if let Some(parent) = exe.parent() {
                let _ = link.SetWorkingDirectory(&HSTRING::from(parent.to_string_lossy().as_ref()));
            }
            if let Ok(pf) = link.cast::<IPersistFile>() {
                let path = wide(&lnk.to_string_lossy());
                let _ = pf.Save(PCWSTR(path.as_ptr()), true);
            }
        }
    });
}
