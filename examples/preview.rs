//! Runs the flyout UI on any OS with sample data, for layout checks.
slint::include_modules!();

use slint::{ModelRc, SharedString, VecModel};

fn main() {
    let ui = Flyout::new().unwrap();
    let img = image::load_from_memory(include_bytes!("../assets/display2026.png")).unwrap().to_rgba8();
    let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(img.as_raw(), img.width(), img.height());
    ui.set_preview(slint::Image::from_rgba8(buf));
    ui.set_display_name("Studio Display (2026)".into());
    ui.set_display_info("usb-c · pid 0x1118".into());
    ui.set_display_found(true);
    ui.set_brightness(58.0);
    ui.set_auto_available(true);
    ui.set_auto_on(true);
    ui.set_auto_status("120 lux".into());
    let open = std::env::args().any(|a| a == "--curve");
    ui.set_curve_open(open);
    ui.set_curve_line("M0 80 L60 60 L120 45 L180 30 L240 15 L300 8".into());
    ui.set_curve_base("M0 85 L60 66 L120 50 L180 34 L240 18 L300 10".into());
    ui.set_curve_dot_x(0.45);
    ui.set_curve_dot_y(0.4);
    ui.set_bias_text("Shifted +6% by your adjustments".into());
    ui.set_bias_active(true);
    let modes: Vec<SharedString> = ["Apple Display (P3-600 nits)", "HDR Video (P3-ST 2084)", "Photography (P3-D50)"].iter().map(|s| (*s).into()).collect();
    ui.set_ref_modes(ModelRc::new(VecModel::from(modes)));
    ui.set_ref_available(true);
    ui.set_ref_status("standard".into());
    ui.set_tt_on(true);
    ui.set_tt_room("room 4200 K · 120 lux".into());
    ui.set_tt_screen("screen 4202 K".into());
    ui.set_tt_room_pos(0.34);
    ui.set_tt_screen_pos(0.34);
    ui.set_manual_on(std::env::args().any(|a| a == "--manual"));
    ui.set_confirm_visible(std::env::args().any(|a| a == "--confirm"));
    ui.set_confirm_text("Keep this mode? Reverting in 10s".into());
    ui.set_autostart(true);
    ui.run().unwrap();
}
