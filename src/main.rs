#![cfg_attr(windows, windows_subsystem = "windows")]

mod color;
mod display;
mod sensor;
mod settings;
mod truetone;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use slint::{CloseRequestResponse, ComponentHandle, SharedString, Timer, TimerMode};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use windows::Win32::System::SystemInformation::GetLocalTime;

use sensor::{Ambient, Sensor};
use truetone::{Mode, TrueTone};

slint::include_modules!();

const STEP: i32 = 10;
const PRESETS: [i32; 5] = [0, 25, 50, 75, 100];

struct State {
    displays: display::Displays,
    tt: TrueTone,
    settings: settings::Settings,
    sensor: Option<Sensor>,
    sensor_probed: bool,
    ambient: Option<Ambient>,
    tint_ok: bool,
    note: &'static str,
}

fn local_hour() -> f64 {
    let t = unsafe { GetLocalTime() };
    t.wHour as f64 + t.wMinute as f64 / 60.0
}

fn sun_icon() -> Icon {
    const N: u32 = 32;
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
            let d = (dx * dx + dy * dy).sqrt();
            let angle = dy.atan2(dx).rem_euclid(std::f32::consts::FRAC_PI_4);
            let on_ray = (11.0..15.0).contains(&d)
                && (angle < 0.2 || angle > std::f32::consts::FRAC_PI_4 - 0.2);
            let on = d <= 7.0 || on_ray;
            rgba.extend_from_slice(if on { &[245, 185, 58, 255] } else { &[0, 0, 0, 0] });
        }
    }
    Icon::from_rgba(rgba, N, N).expect("valid icon")
}

fn ambient_text(s: &State) -> String {
    match (&s.sensor, &s.ambient) {
        (None, _) => "No ambient light sensor found".into(),
        (Some(_), None) => "Waiting for sensor…".into(),
        (Some(_), Some(a)) => match a.cct {
            Some(k) => format!("{:.0} lux  ·  {:.0} K", a.lux, k),
            None => format!("{:.0} lux  ·  color temperature unavailable", a.lux),
        },
    }
}

fn tt_note(s: &State) -> String {
    let mut n = String::from(
        "True Tone here is approximated with a software color shift (a display gamma ramp), \
         so it can't change the panel's own white point. It works without admin rights, but \
         may be ignored in HDR mode and affects color-managed apps.",
    );
    if s.tt.mode == Mode::Auto {
        n.push_str(&format!("\n\nDriven by: {}.", s.note));
    }
    if s.tt.mode != Mode::Off && !s.tint_ok {
        n.push_str("\n\nCouldn't apply the tint: no Apple display found, or Windows rejected the color ramp.");
    }
    n
}

fn push_ui(ui: &AppWindow, s: &State) {
    ui.set_display_found(s.displays.found());
    ui.set_display_status(SharedString::from(s.displays.status()));
    ui.set_tt_mode(s.tt.mode.index());
    ui.set_tt_strength(s.tt.strength as f32);
    ui.set_tt_warmth(s.tt.manual_k as f32);
    ui.set_ambient_text(SharedString::from(ambient_text(s)));
    ui.set_whitepoint_text(SharedString::from(if s.tt.mode == Mode::Off {
        "Neutral (6500 K)".to_string()
    } else {
        format!("{:.0} K", s.tt.current_k())
    }));
    ui.set_tt_note(SharedString::from(tt_note(s)));
    ui.set_autostart(s.settings.autostart);
}

fn sync_brightness(ui: &AppWindow, s: &State) {
    if let Some(p) = s.displays.percent() {
        ui.set_brightness(p as f32);
    }
}

/// Applies the current True Tone state to the screen.
fn apply_tint(s: &mut State) {
    let ambient = s.ambient.as_ref();
    let (k, note) = s.tt.tick(ambient, local_hour());
    s.note = note;
    if s.tt.mode == Mode::Off {
        color::reset();
        s.tint_ok = true;
    } else {
        s.tint_ok = color::apply_kelvin(k) > 0;
    }
}

fn save(s: &mut State) {
    s.settings.tt_mode = s.tt.mode.index();
    s.settings.tt_strength = s.tt.strength;
    s.settings.tt_warmth = s.tt.manual_k;
    settings::save(&s.settings);
}

fn diagnostics_text(s: &State) -> String {
    let mut out = String::from("== HID ==\n");
    out.push_str(&display::diagnostics());
    out.push_str("\n== Color ==\n");
    out.push_str(&color::describe());
    out.push_str("\n== Ambient sensor ==\n");
    match &s.sensor {
        None => out.push_str("No Windows light sensor found.\n"),
        Some(sen) => out.push_str(&format!(
            "id={}\nchromaticity_supported={}\nlast reading: {}\n",
            sen.id(),
            sen.chromaticity,
            ambient_text(s)
        )),
    }
    out
}

fn main() {
    let start_hidden = std::env::args().any(|a| a == "--minimized");
    let ui = AppWindow::new().expect("create window");
    ui.set_version(SharedString::from(env!("CARGO_PKG_VERSION")));
    ui.window().on_close_requested(|| CloseRequestResponse::HideWindow);

    let cfg = settings::load();
    let state = Rc::new(RefCell::new(State {
        displays: display::Displays::new(),
        tt: TrueTone::new(Mode::from_index(cfg.tt_mode), cfg.tt_strength, cfg.tt_warmth),
        settings: cfg,
        sensor: None,
        sensor_probed: false,
        ambient: None,
        tint_ok: true,
        note: "",
    }));

    {
        let s = state.borrow();
        sync_brightness(&ui, &s);
        push_ui(&ui, &s);
    }

    // ---- UI callbacks ----
    ui.on_set_brightness({
        let state = state.clone();
        move |v| state.borrow_mut().displays.set(v)
    });
    ui.on_refresh({
        let (state, weak) = (state.clone(), ui.as_weak());
        move || {
            let mut s = state.borrow_mut();
            s.displays.refresh();
            if let Some(ui) = weak.upgrade() {
                sync_brightness(&ui, &s);
                push_ui(&ui, &s);
            }
        }
    });
    ui.on_tt_mode_changed({
        let (state, weak) = (state.clone(), ui.as_weak());
        move |i| {
            let mut s = state.borrow_mut();
            s.tt.mode = Mode::from_index(i);
            apply_tint(&mut s);
            save(&mut s);
            if let Some(ui) = weak.upgrade() {
                push_ui(&ui, &s);
            }
        }
    });
    ui.on_tt_strength_changed({
        let (state, weak) = (state.clone(), ui.as_weak());
        move |v| {
            let mut s = state.borrow_mut();
            s.tt.strength = v;
            apply_tint(&mut s);
            save(&mut s);
            if let Some(ui) = weak.upgrade() {
                push_ui(&ui, &s);
            }
        }
    });
    ui.on_tt_warmth_changed({
        let (state, weak) = (state.clone(), ui.as_weak());
        move |v| {
            let mut s = state.borrow_mut();
            s.tt.manual_k = v;
            apply_tint(&mut s);
            save(&mut s);
            if let Some(ui) = weak.upgrade() {
                push_ui(&ui, &s);
            }
        }
    });
    ui.on_autostart_changed({
        let state = state.clone();
        move |on| {
            let mut s = state.borrow_mut();
            s.settings.autostart = on;
            settings::set_autostart(on);
            save(&mut s);
        }
    });
    ui.on_diagnostics({
        let state = state.clone();
        move || {
            let path = std::env::temp_dir().join("studio-brightness-diag.txt");
            if std::fs::write(&path, diagnostics_text(&state.borrow())).is_ok() {
                let _ = std::process::Command::new("notepad").arg(&path).spawn();
            }
        }
    });

    // ---- tray + hotkeys ----
    let open = MenuItem::new("Open Studio Brightness", true, None);
    let brighter = MenuItem::new("Brighter\tCtrl+Alt+Up", true, None);
    let dimmer = MenuItem::new("Dimmer\tCtrl+Alt+Down", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let preset_items: Vec<(MenuItem, i32)> = PRESETS
        .iter()
        .map(|&p| (MenuItem::new(format!("{p}%"), true, None), p))
        .collect();
    let menu = Menu::new();
    menu.append(&open).unwrap();
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    menu.append(&brighter).unwrap();
    menu.append(&dimmer).unwrap();
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    for (item, _) in &preset_items {
        menu.append(item).unwrap();
    }
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    menu.append(&quit).unwrap();

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("Studio Brightness")
        .with_icon(sun_icon())
        .build()
        .expect("tray icon");

    let mods = Modifiers::CONTROL | Modifiers::ALT;
    let up = HotKey::new(Some(mods), Code::ArrowUp);
    let down = HotKey::new(Some(mods), Code::ArrowDown);
    let hotkeys = GlobalHotKeyManager::new().ok();
    if let Some(m) = &hotkeys {
        let _ = m.register(up);
        let _ = m.register(down);
    }

    // Poll tray / hotkey events.
    let event_timer = Timer::default();
    event_timer.start(TimerMode::Repeated, Duration::from_millis(60), {
        let (state, weak) = (state.clone(), ui.as_weak());
        let menu_rx = MenuEvent::receiver();
        let tray_rx = TrayIconEvent::receiver();
        let hotkey_rx = GlobalHotKeyEvent::receiver();
        move || {
            let show = |weak: &slint::Weak<AppWindow>, state: &Rc<RefCell<State>>| {
                if let Some(ui) = weak.upgrade() {
                    let mut s = state.borrow_mut();
                    s.displays.refresh();
                    sync_brightness(&ui, &s);
                    push_ui(&ui, &s);
                    let _ = ui.show();
                    ui.window().set_minimized(false);
                }
            };
            let nudge = |weak: &slint::Weak<AppWindow>, state: &Rc<RefCell<State>>, f: &dyn Fn(&mut display::Displays)| {
                let mut s = state.borrow_mut();
                f(&mut s.displays);
                if let Some(ui) = weak.upgrade() {
                    sync_brightness(&ui, &s);
                }
            };
            while let Ok(e) = menu_rx.try_recv() {
                if e.id == quit.id() {
                    color::reset();
                    let _ = slint::quit_event_loop();
                } else if e.id == open.id() {
                    show(&weak, &state);
                } else if e.id == brighter.id() {
                    nudge(&weak, &state, &|d| d.adjust(STEP));
                } else if e.id == dimmer.id() {
                    nudge(&weak, &state, &|d| d.adjust(-STEP));
                } else if let Some((_, p)) = preset_items.iter().find(|(i, _)| e.id == i.id()) {
                    let p = *p;
                    nudge(&weak, &state, &|d| d.set(p));
                }
            }
            while let Ok(e) = tray_rx.try_recv() {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                    show(&weak, &state);
                }
            }
            while let Ok(e) = hotkey_rx.try_recv() {
                if e.state == HotKeyState::Pressed {
                    if e.id == up.id() {
                        nudge(&weak, &state, &|d| d.adjust(STEP));
                    } else if e.id == down.id() {
                        nudge(&weak, &state, &|d| d.adjust(-STEP));
                    }
                }
            }
        }
    });

    // Sensor + True Tone loop (1 Hz). The sensor is opened lazily, once the event loop
    // (and COM) is up.
    let tick_timer = Timer::default();
    tick_timer.start(TimerMode::Repeated, Duration::from_secs(1), {
        let (state, weak) = (state.clone(), ui.as_weak());
        move || {
            let mut s = state.borrow_mut();
            if !s.sensor_probed {
                s.sensor = Sensor::new();
                s.sensor_probed = true;
            }
            s.ambient = s.sensor.as_ref().and_then(|x| x.read());
            if s.tt.mode != Mode::Off {
                apply_tint(&mut s); // also re-asserts the ramp if Windows reset it
            }
            if let Some(ui) = weak.upgrade() {
                if ui.window().is_visible() {
                    ui.set_ambient_text(SharedString::from(ambient_text(&s)));
                    ui.set_whitepoint_text(SharedString::from(if s.tt.mode == Mode::Off {
                        "Neutral (6500 K)".to_string()
                    } else {
                        format!("{:.0} K", s.tt.current_k())
                    }));
                    ui.set_tt_note(SharedString::from(tt_note(&s)));
                }
            }
        }
    });

    if !start_hidden {
        ui.show().expect("show window");
    }
    {
        let mut s = state.borrow_mut();
        apply_tint(&mut s);
    }
    slint::run_event_loop_until_quit().expect("event loop");
    color::reset();
}
