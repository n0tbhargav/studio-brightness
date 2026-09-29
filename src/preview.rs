//! Draws the Apple display image with the live brightness dimming and tint on its screen area.

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

// Screen rectangle as fractions of the (cropped) product image.
const SX: f32 = 0.02094;
const SY: f32 = 0.02857;
const SW: f32 = 0.95707;
const SH: f32 = 0.70068;

pub struct Preview {
    base: image::RgbaImage,
}

impl Preview {
    pub fn new() -> Self {
        let base = image::load_from_memory(include_bytes!("../assets/display2026.png")).expect("asset").to_rgba8();
        Self { base }
    }

    /// `mult` is the per-channel tint (0..1), `level` the brightness factor (0..1).
    pub fn render(&self, mult: [f64; 3], level: f32) -> Image {
        let (w, h) = self.base.dimensions();
        let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
        buf.make_mut_bytes().copy_from_slice(self.base.as_raw());
        let (x0, x1) = ((SX * w as f32) as u32, ((SX + SW) * w as f32) as u32);
        let (y0, y1) = ((SY * h as f32) as u32, ((SY + SH) * h as f32) as u32);
        let f = [mult[0] as f32 * level, mult[1] as f32 * level, mult[2] as f32 * level];
        let px = buf.make_mut_slice();
        for y in y0..y1.min(h) {
            for x in x0..x1.min(w) {
                let p = &mut px[(y * w + x) as usize];
                p.r = (p.r as f32 * f[0]) as u8;
                p.g = (p.g as f32 * f[1]) as u8;
                p.b = (p.b as f32 * f[2]) as u8;
            }
        }
        Image::from_rgba8(buf)
    }
}
