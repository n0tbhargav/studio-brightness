#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("Studio Brightness runs on Windows. To preview the UI here: cargo run --example preview");
}

#[cfg(windows)]
mod autobright;
#[cfg(windows)]
mod color;
#[cfg(windows)]
mod display;
#[cfg(windows)]
mod preview;
#[cfg(windows)]
mod presets;
#[cfg(windows)]
mod sensor;
#[cfg(windows)]
mod settings;
#[cfg(windows)]
mod truetone;
#[cfg(windows)]
mod winutil;

#[cfg(windows)]
fn main() {
    app::run();
}

#[cfg(windows)]
mod app {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use slint::{ComponentHandle, ModelRc, PhysicalPosition, SharedString, Timer, TimerMode, VecModel};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::SystemInformation::GetLocalTime;

    use crate::autobright::AutoBright;
    use crate::display::Displays;
    use crate::preview::Preview;
    use crate::presets::Presets;
    use crate::sensor::{Ambient, Sensor};
    use crate::settings::Settings;
    use crate::truetone::{Source, TrueTone};
    use crate::{color, display, settings, winutil};

    slint::include_modules!();

    const STEP: f32 = 10.0;
    const CONFIRM_SECS: u64 = 10;

    struct Confirm {
        prev: u32,
        deadline: Instant,
    }

    struct App {
        flyout: slint::Weak<Flyout>,
        osd: slint::Weak<Osd>,
        displays: Displays,
        presets: Option<Presets>,
        sensor: Option<Sensor>,
        sensor_probed: bool,
        ambient: Option<Ambient>,
        st: Settings,
        dirty: bool,
        tt: TrueTone,
        ab: AutoBright,
        bright: f32,
        active_mode: Option<u32>,
        confirm: Option<Confirm>,
        preview: Preview,
        preview_key: (i32, i32, i32),
        src: Source,
        tint_ok: bool,
        applied: Option<(f64, f64)>,
        applied_at: Instant,
        shown_at: Option<Instant>,
        focused_once: bool,
        hidden_at: Instant,
        fly_hwnd: Option<HWND>,
        osd_hwnd: Option<HWND>,
        osd_hide_at: Option<Instant>,
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
                let on_ray = (11.0..15.0).contains(&d) && (angle < 0.2 || angle > std::f32::consts::FRAC_PI_4 - 0.2);
                let on = d <= 7.0 || on_ray;
                rgba.extend_from_slice(if on { &[242, 179, 61, 255] } else { &[0, 0, 0, 0] });
            }
        }
        Icon::from_rgba(rgba, N, N).expect("valid icon")
    }

    impl App {
        fn locked(&self) -> bool {
            match (&self.presets, self.active_mode) {
                (Some(p), Some(i)) => p.locks_controls(i),
                _ => false,
            }
        }

        fn lux(&self) -> f64 {
            self.ambient.as_ref().map(|a| a.lux).unwrap_or(0.0)
        }

        fn auto_available(&self) -> bool {
            self.sensor.is_some() && self.ambient.is_some()
        }

        fn visible(&self) -> bool {
            self.shown_at.is_some()
        }

        fn mark_dirty(&mut self) {
            self.dirty = true;
        }

        fn flush(&mut self) {
            if !self.dirty {
                return;
            }
            self.st.tt_on = self.tt.tt_on;
            self.st.manual_on = self.tt.manual_on;
            self.st.warmth = self.tt.warmth;
            self.st.shift = self.tt.shift;
            self.st.auto_on = self.ab.on;
            self.st.bias = self.ab.bias;
            settings::save(&self.st);
            self.dirty = false;
        }

        // ---------- brightness ----------
        fn sync_from_hardware(&mut self) {
            if let Some(p) = self.displays.percent() {
                self.bright = p as f32;
            }
        }

        fn user_set(&mut self, v: f32, osd: bool) {
            if self.locked() {
                return;
            }
            let v = v.clamp(0.0, 100.0);
            self.bright = v;
            self.displays.set(v.round() as i32);
            if self.ab.on {
                self.ab.learn(v, self.lux());
                self.mark_dirty();
            }
            if osd {
                self.show_osd();
            }
            self.push_ui();
        }

        fn nudge(&mut self, delta: f32) {
            self.sync_from_hardware();
            self.user_set(self.bright + delta, true);
        }

        // ---------- reference modes ----------
        fn refresh_modes(&mut self) {
            if let Some(p) = &self.presets {
                self.active_mode = p.active();
            }
        }

        fn select_mode(&mut self, pos: usize) {
            let Some(p) = &self.presets else { return };
            let Some(mode) = p.modes.get(pos) else { return };
            let new = mode.index;
            let prev = self.active_mode.unwrap_or(0);
            if new == prev {
                return;
            }
            if p.set_active(new) {
                self.active_mode = Some(new);
                // Safety net: some panels blank on a mode switch, so revert unless confirmed.
                self.confirm = Some(Confirm { prev, deadline: Instant::now() + Duration::from_secs(CONFIRM_SECS) });
                self.after_mode_change();
            }
        }

        fn revert_mode(&mut self) {
            if let Some(c) = self.confirm.take() {
                if let Some(p) = &self.presets {
                    p.set_active(c.prev);
                }
                self.active_mode = Some(c.prev);
                self.after_mode_change();
            }
        }

        fn after_mode_change(&mut self) {
            self.ab.invalidate();
            self.applied = None; // force the tint to be re-applied (or cleared when locked)
            self.sync_from_hardware();
            self.push_ui();
        }

        // ---------- ticks ----------
        fn color_tick(&mut self) {
            let locked = self.locked();
            let (k, shift, src) = self.tt.tick(self.ambient.as_ref(), local_hour(), locked);
            self.src = src;
            let need = match self.applied {
                None => true,
                Some((ak, ash)) => (ak - k).abs() > 1.0 || (ash - shift).abs() > 0.5 || (src != Source::Off && self.applied_at.elapsed() > Duration::from_secs(3)),
            };
            if need {
                let n = color::apply(k, shift);
                self.tint_ok = n > 0 || src == Source::Off;
                self.applied = Some((k, shift));
                self.applied_at = Instant::now();
            }
        }

        fn auto_tick(&mut self) {
            if self.ab.on && !self.locked() && self.auto_available() {
                let lux = self.lux();
                if self.ab.step(lux, &mut self.bright) {
                    self.displays.set(self.bright.round() as i32);
                }
            }
        }

        fn sensor_tick(&mut self) {
            if !self.sensor_probed {
                self.sensor = Sensor::new();
                self.sensor_probed = true;
            }
            self.ambient = self.sensor.as_ref().and_then(|s| s.read());
            self.flush();
        }

        // ---------- windows ----------
        fn position_flyout(&self, ui: &Flyout) {
            let (l, t, r, b) = winutil::work_area();
            let size = ui.window().size();
            let m = (12.0 * ui.window().scale_factor()) as i32;
            let (x, y) = ((r - size.width as i32 - m).max(l), (b - size.height as i32 - m).max(t));
            let cur = ui.window().position();
            if cur.x != x || cur.y != y {
                ui.window().set_position(PhysicalPosition::new(x, y));
            }
        }

        fn show_flyout(&mut self) {
            let Some(ui) = self.flyout.upgrade() else { return };
            self.displays.refresh();
            self.sync_from_hardware();
            self.refresh_modes();
            self.fill_modes(&ui);
            self.push_ui();
            let _ = ui.show();
            if self.fly_hwnd.is_none() {
                self.fly_hwnd = winutil::hwnd_of(ui.window());
                if let Some(h) = self.fly_hwnd {
                    winutil::style_popup(h, false);
                }
            }
            self.position_flyout(&ui);
            if let Some(h) = self.fly_hwnd {
                winutil::focus(h);
            }
            self.shown_at = Some(Instant::now());
            self.focused_once = false;
        }

        fn hide_flyout(&mut self) {
            if let Some(ui) = self.flyout.upgrade() {
                let _ = ui.hide();
            }
            self.shown_at = None;
            self.hidden_at = Instant::now();
        }

        fn toggle_flyout(&mut self) {
            if self.visible() {
                self.hide_flyout();
            } else if self.hidden_at.elapsed() > Duration::from_millis(300) {
                self.show_flyout();
            }
        }

        fn init_osd(&mut self) {
            let Some(o) = self.osd.upgrade() else { return };
            o.window().set_position(PhysicalPosition::new(-5000, -5000)); // first show activates; keep it off-screen
            let _ = o.show();
            self.osd_hwnd = winutil::hwnd_of(o.window());
            if let Some(h) = self.osd_hwnd {
                winutil::style_popup(h, true);
            }
            let _ = o.hide();
        }

        fn show_osd(&mut self) {
            let Some(o) = self.osd.upgrade() else { return };
            o.set_level(self.bright);
            let (l, _t, r, b) = winutil::work_area();
            let size = o.window().size();
            let scale = o.window().scale_factor();
            let x = (l + r) / 2 - size.width as i32 / 2;
            let y = b - size.height as i32 - (48.0 * scale) as i32;
            o.window().set_position(PhysicalPosition::new(x, y));
            let _ = o.show();
            self.osd_hide_at = Some(Instant::now() + Duration::from_millis(1800));
        }

        // ---------- ui state ----------
        fn fill_modes(&self, ui: &Flyout) {
            match &self.presets {
                Some(p) => {
                    let names: Vec<SharedString> = p.modes.iter().map(|m| SharedString::from(m.name.as_str())).collect();
                    ui.set_ref_modes(ModelRc::new(VecModel::from(names)));
                    ui.set_ref_available(true);
                }
                None => {
                    ui.set_ref_modes(ModelRc::new(VecModel::from(Vec::<SharedString>::new())));
                    ui.set_ref_available(false);
                }
            }
        }

        fn push_ui(&mut self) {
            if !self.visible() {
                return;
            }
            let Some(ui) = self.flyout.upgrade() else { return };
            let locked = self.locked();
            let lux = self.lux();

            ui.set_display_name(self.displays.name().into());
            ui.set_display_info(self.displays.info().into());
            ui.set_display_found(self.displays.found());

            // brightness
            ui.set_brightness(self.bright);
            ui.set_brightness_locked(locked);
            ui.set_auto_available(self.auto_available());
            ui.set_auto_on(self.ab.on && !locked);
            ui.set_auto_status(
                if self.sensor.is_none() {
                    "no light sensor".to_string()
                } else if locked {
                    "locked".to_string()
                } else if self.ab.on {
                    if lux < 1.0 { "dark room".to_string() } else { format!("{lux:.0} lux") }
                } else {
                    "manual".to_string()
                }
                .into(),
            );
            ui.set_bias_active(self.ab.bias != 0.0);
            ui.set_bias_text(
                if self.ab.bias == 0.0 {
                    "Following the default curve".to_string()
                } else {
                    format!("Shifted {:+.0}% by your adjustments", self.ab.bias)
                }
                .into(),
            );
            if ui.get_curve_open() {
                let (line, base) = self.ab.paths();
                let (dx, dy) = AutoBright::dot(lux, self.bright);
                ui.set_curve_line(line.into());
                ui.set_curve_base(base.into());
                ui.set_curve_dot_x(dx);
                ui.set_curve_dot_y(dy);
            }

            // color mode
            if let (Some(p), Some(active)) = (&self.presets, self.active_mode) {
                if let Some(pos) = p.modes.iter().position(|m| m.index == active) {
                    ui.set_ref_index(pos as i32);
                }
            }
            ui.set_ref_status((if locked { "color-critical" } else { "standard" }).into());
            match &self.confirm {
                Some(c) => {
                    let left = c.deadline.saturating_duration_since(Instant::now()).as_secs() + 1;
                    ui.set_confirm_visible(true);
                    ui.set_confirm_text(format!("Keep this mode? Reverting in {left}s").into());
                }
                None => ui.set_confirm_visible(false),
            }

            // true tone + manual tint
            ui.set_tone_locked(locked);
            ui.set_tt_on(self.tt.tt_on && !self.tt.manual_on);
            ui.set_manual_on(self.tt.manual_on);
            ui.set_warmth(self.tt.warmth);
            ui.set_shift(self.tt.shift);
            let pos = |k: f64| (((k - 3000.0) / 3500.0).clamp(0.0, 1.0)) as f32;
            let room_k = self.ambient.as_ref().and_then(|a| if a.lux >= 1.0 { a.cct } else { None });
            ui.set_tt_room(
                match (&self.ambient, room_k) {
                    (Some(a), Some(k)) => format!("room {k:.0} K · {:.0} lux", a.lux),
                    (Some(a), None) => format!("room {:.0} lux · no color data", a.lux),
                    (None, _) => "room: no sensor".to_string(),
                }
                .into(),
            );
            ui.set_tt_screen(format!("screen {:.0} K", self.tt.cur_k()).into());
            ui.set_tt_room_pos(pos(room_k.unwrap_or(self.tt.cur_k())));
            ui.set_tt_screen_pos(pos(self.tt.cur_k()));
            ui.set_tt_status(
                if locked {
                    "locked by color mode".to_string()
                } else if self.tt.manual_on {
                    "paused".to_string()
                } else if self.tt.tt_on && !self.tint_ok {
                    "no Apple display to tint".to_string()
                } else {
                    match self.src {
                        Source::Sensor => "sensor".to_string(),
                        Source::TimeOfDay => "time of day".to_string(),
                        _ => String::new(),
                    }
                }
                .into(),
            );
            ui.set_manual_status(
                if locked {
                    "locked by color mode"
                } else if self.tt.manual_on {
                    "overrides True Tone"
                } else {
                    ""
                }
                .into(),
            );
            ui.set_autostart(self.st.autostart);

            // live preview of the screen
            let m = color::multipliers(self.tt.cur_k(), if self.src == Source::Manual { self.tt.shift as f64 } else { 0.0 });
            let level = 0.3 + 0.7 * self.bright / 100.0;
            let key = ((m[0] * 100.0) as i32 * 10_000 + (m[1] * 100.0) as i32 * 100 + (m[2] * 100.0) as i32, (level * 100.0) as i32, 0);
            if key != self.preview_key {
                self.preview_key = key;
                ui.set_preview(self.preview.render(m, level));
            }
        }

        fn diagnostics_text(&self) -> String {
            let mut out = String::from("== HID brightness ==\n");
            out.push_str(&display::diagnostics());
            out.push_str("\n== Reference modes ==\n");
            match &self.presets {
                None => out.push_str("No 0xFF20 interface found.\n"),
                Some(p) => {
                    for m in &p.modes {
                        out.push_str(&format!("{}{}: {}\n", if Some(m.index) == self.active_mode { "* " } else { "  " }, m.index, m.name));
                    }
                }
            }
            out.push_str("\n== Color ==\n");
            out.push_str(&color::describe());
            out.push_str("\n== Ambient sensor ==\n");
            match &self.sensor {
                None => out.push_str("No Windows light sensor found.\n"),
                Some(s) => out.push_str(&format!(
                    "id={}\nchromaticity_supported={}\nlast reading: {}\n",
                    s.id(),
                    s.chromaticity,
                    match &self.ambient {
                        Some(a) => format!("{:.1} lux, cct {:?}", a.lux, a.cct),
                        None => "none".into(),
                    }
                )),
            }
            out
        }
    }

    pub fn run() {
        let cfg = settings::load();
        let start_hidden = std::env::args().any(|a| a == "--minimized");

        let flyout = Flyout::new().expect("flyout");
        let osd = Osd::new().expect("osd");

        let mut displays = Displays::new();
        displays.refresh();
        let presets = Presets::open();
        let mut app0 = App {
            flyout: flyout.as_weak(),
            osd: osd.as_weak(),
            bright: 50.0,
            displays,
            active_mode: None,
            presets,
            sensor: None,
            sensor_probed: false,
            ambient: None,
            tt: TrueTone::new(cfg.tt_on, cfg.manual_on, cfg.warmth, cfg.shift),
            ab: AutoBright::new(cfg.auto_on, cfg.bias),
            st: cfg,
            dirty: false,
            confirm: None,
            preview: Preview::new(),
            preview_key: (-1, -1, -1),
            src: Source::Off,
            tint_ok: true,
            applied: None,
            applied_at: Instant::now(),
            shown_at: None,
            focused_once: false,
            hidden_at: Instant::now() - Duration::from_secs(5),
            fly_hwnd: None,
            osd_hwnd: None,
            osd_hide_at: None,
        };
        app0.refresh_modes();
        app0.sync_from_hardware();
        let app = Rc::new(RefCell::new(app0));

        flyout.set_preview(app.borrow().preview.render([1.0, 1.0, 1.0], 1.0));

        // ---- flyout callbacks ----
        flyout.on_brightness_changed({
            let a = app.clone();
            move |v| a.borrow_mut().user_set(v, false)
        });
        flyout.on_auto_toggled({
            let a = app.clone();
            move |v| {
                let mut s = a.borrow_mut();
                s.ab.on = v;
                s.ab.invalidate();
                s.mark_dirty();
                s.push_ui();
            }
        });
        flyout.on_curve_reset({
            let a = app.clone();
            move || {
                let mut s = a.borrow_mut();
                s.ab.bias = 0.0;
                s.ab.invalidate();
                s.mark_dirty();
                s.push_ui();
            }
        });
        flyout.on_ref_selected({
            let a = app.clone();
            move |i| a.borrow_mut().select_mode(i.max(0) as usize)
        });
        flyout.on_confirm_keep({
            let a = app.clone();
            move || {
                let mut s = a.borrow_mut();
                s.confirm = None;
                s.push_ui();
            }
        });
        flyout.on_confirm_revert({
            let a = app.clone();
            move || a.borrow_mut().revert_mode()
        });
        flyout.on_tt_toggled({
            let a = app.clone();
            move |v| {
                let mut s = a.borrow_mut();
                s.tt.tt_on = v;
                if v {
                    s.tt.manual_on = false;
                }
                s.mark_dirty();
                s.push_ui();
            }
        });
        flyout.on_manual_toggled({
            let a = app.clone();
            move |v| {
                let mut s = a.borrow_mut();
                s.tt.manual_on = v;
                if v {
                    s.tt.warmth = ((s.tt.cur_k() / 50.0).round() * 50.0).clamp(3000.0, 6500.0) as f32;
                }
                s.mark_dirty();
                s.push_ui();
            }
        });
        flyout.on_warmth_changed({
            let a = app.clone();
            move |v| {
                let mut s = a.borrow_mut();
                s.tt.warmth = v;
                s.mark_dirty();
            }
        });
        flyout.on_shift_changed({
            let a = app.clone();
            move |v| {
                let mut s = a.borrow_mut();
                s.tt.shift = v;
                s.mark_dirty();
            }
        });
        flyout.on_autostart_toggled({
            let a = app.clone();
            move |v| {
                let mut s = a.borrow_mut();
                s.st.autostart = v;
                settings::set_autostart(v);
                s.mark_dirty();
            }
        });
        flyout.on_diagnostics({
            let a = app.clone();
            move || {
                let path = std::env::temp_dir().join("studio-brightness-diag.txt");
                if std::fs::write(&path, a.borrow().diagnostics_text()).is_ok() {
                    let _ = std::process::Command::new("notepad").arg(&path).spawn();
                }
            }
        });
        flyout.on_quit(|| {
            color::reset();
            let _ = slint::quit_event_loop();
        });

        // ---- tray + hotkeys ----
        let open = MenuItem::new("Open Studio Brightness", true, None);
        let brighter = MenuItem::new("Brighter\tCtrl+Alt+Up", true, None);
        let dimmer = MenuItem::new("Dimmer\tCtrl+Alt+Down", true, None);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::new();
        menu.append(&open).unwrap();
        menu.append(&PredefinedMenuItem::separator()).unwrap();
        menu.append(&brighter).unwrap();
        menu.append(&dimmer).unwrap();
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

        // ---- timers ----
        let fast = Timer::default();
        fast.start(TimerMode::Repeated, Duration::from_millis(60), {
            let a = app.clone();
            let menu_rx = MenuEvent::receiver();
            let tray_rx = TrayIconEvent::receiver();
            let hotkey_rx = GlobalHotKeyEvent::receiver();
            move || {
                let mut s = a.borrow_mut();
                while let Ok(e) = menu_rx.try_recv() {
                    if e.id == quit.id() {
                        color::reset();
                        let _ = slint::quit_event_loop();
                    } else if e.id == open.id() {
                        s.toggle_flyout();
                    } else if e.id == brighter.id() {
                        s.nudge(STEP);
                    } else if e.id == dimmer.id() {
                        s.nudge(-STEP);
                    }
                }
                while let Ok(e) = tray_rx.try_recv() {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        s.toggle_flyout();
                    }
                }
                while let Ok(e) = hotkey_rx.try_recv() {
                    if e.state == HotKeyState::Pressed {
                        if e.id == up.id() {
                            s.nudge(STEP);
                        } else if e.id == down.id() {
                            s.nudge(-STEP);
                        }
                    }
                }

                // click-away closes the flyout
                // (only armed once the flyout has actually had focus, so a refused focus grab can't close it)
                if let (Some(_), Some(h)) = (s.shown_at, s.fly_hwnd) {
                    if winutil::foreground_is(h) {
                        s.focused_once = true;
                    } else if s.focused_once {
                        s.hide_flyout();
                    }
                }
                if s.visible() {
                    if let Some(ui) = s.flyout.upgrade() {
                        s.position_flyout(&ui);
                    }
                }
                // mode-switch safety net
                if let Some(c) = &s.confirm {
                    if Instant::now() >= c.deadline {
                        s.revert_mode();
                    }
                }
                if let Some(t) = s.osd_hide_at {
                    if Instant::now() >= t {
                        if let Some(o) = s.osd.upgrade() {
                            let _ = o.hide();
                        }
                        s.osd_hide_at = None;
                    }
                }
            }
        });

        let tick = Timer::default();
        tick.start(TimerMode::Repeated, Duration::from_millis(100), {
            let a = app.clone();
            move || {
                let mut s = a.borrow_mut();
                s.color_tick();
                s.auto_tick();
                s.push_ui();
            }
        });

        let sensor_timer = Timer::default();
        sensor_timer.start(TimerMode::Repeated, Duration::from_secs(1), {
            let a = app.clone();
            move || a.borrow_mut().sensor_tick()
        });

        // Startup work that needs the event loop (window handles exist once it runs).
        Timer::single_shot(Duration::from_millis(50), {
            let a = app.clone();
            move || {
                let mut s = a.borrow_mut();
                s.init_osd();
                s.sensor_tick();
                if !start_hidden {
                    s.show_flyout();
                }
            }
        });

        slint::run_event_loop_until_quit().expect("event loop");
        color::reset();
    }
}
