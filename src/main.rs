#![cfg_attr(windows, windows_subsystem = "windows")]

mod display;

use std::time::{Duration, Instant};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

const STEP: i32 = 10;
const PRESETS: [i32; 5] = [0, 25, 50, 75, 100];

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
            rgba.extend_from_slice(if on { &[255, 200, 0, 255] } else { &[0, 0, 0, 0] });
        }
    }
    Icon::from_rgba(rgba, N, N).expect("valid icon")
}

fn main() {
    let event_loop = EventLoopBuilder::new().build();

    let preset_items: Vec<(MenuItem, i32)> = PRESETS
        .iter()
        .map(|&p| (MenuItem::new(format!("{p}%"), true, None), p))
        .collect();
    let brighter = MenuItem::new("Brighter\tCtrl+Alt+Up", true, None);
    let dimmer = MenuItem::new("Dimmer\tCtrl+Alt+Down", true, None);
    let diag = MenuItem::new("Diagnostics...", true, None);
    let quit = MenuItem::new("Quit", true, None);

    let menu = Menu::new();
    menu.append(&brighter).unwrap();
    menu.append(&dimmer).unwrap();
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    for (item, _) in &preset_items {
        menu.append(item).unwrap();
    }
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    menu.append(&diag).unwrap();
    menu.append(&quit).unwrap();

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Studio Display brightness")
        .with_icon(sun_icon())
        .build()
        .expect("tray icon");

    // Hotkey registration can fail if another app owns the combo; the menu still works.
    let hotkeys = GlobalHotKeyManager::new().ok();
    let mods = Modifiers::CONTROL | Modifiers::ALT;
    let up = HotKey::new(Some(mods), Code::ArrowUp);
    let down = HotKey::new(Some(mods), Code::ArrowDown);
    if let Some(m) = &hotkeys {
        let _ = m.register(up);
        let _ = m.register(down);
    }

    let menu_rx = MenuEvent::receiver();
    let hotkey_rx = GlobalHotKeyEvent::receiver();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(50));
        if let Event::NewEvents(StartCause::Init) = event {
            return;
        }

        while let Ok(e) = menu_rx.try_recv() {
            if e.id == quit.id() {
                *control_flow = ControlFlow::Exit;
            } else if e.id == diag.id() {
                let path = std::env::temp_dir().join("studio-brightness-diag.txt");
                if std::fs::write(&path, display::diagnostics()).is_ok() {
                    let _ = std::process::Command::new("notepad").arg(&path).spawn();
                }
            } else if e.id == brighter.id() {
                display::adjust(STEP);
            } else if e.id == dimmer.id() {
                display::adjust(-STEP);
            } else if let Some((_, p)) = preset_items.iter().find(|(i, _)| e.id == i.id()) {
                display::set_all(*p);
            }
        }
        while let Ok(e) = hotkey_rx.try_recv() {
            if e.state == HotKeyState::Pressed {
                if e.id == up.id() {
                    display::adjust(STEP);
                } else if e.id == down.id() {
                    display::adjust(-STEP);
                }
            }
        }
    });
}
