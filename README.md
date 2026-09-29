# Studio Brightness

A tray flyout for Apple Studio Displays on Windows. Talks to the display over USB HID, so it needs no driver and no admin rights.

- **Brightness** with a slider, hotkeys (Ctrl+Alt+Up / Down) and an on-screen level bar.
- **Auto brightness** from the display's ambient light sensor (Windows Sensor API), with a learnable curve.
- **Color mode**: the display's own reference modes, read from the display. A 10-second "keep this mode?" prompt reverts automatically, in case a panel blanks.
- **True Tone**: a software white-point shift (gamma ramp) that follows the room's color temperature, or a time-of-day schedule when the sensor gives no color data.
- **Manual tint**: warmth and green/magenta sliders. It pauses True Tone.
- Launch at sign-in (per-user registry entry, no elevation). On first run the app also adds itself to the Start menu; launching it again just opens the flyout.
- Tint method: gamma ramp first, with a click-through overlay as a fallback for drivers that reject or ignore gamma. Cycle it with the `tint:` button in the flyout footer.

Reference modes other than the "Apple ..." ones lock brightness, True Tone and tint, like macOS.

## Build

    cargo xwin build --release --target aarch64-pc-windows-msvc   # or x86_64-pc-windows-msvc

Preview the UI on any OS with sample data: `cargo run --example preview`.

## Notes

- True Tone here is a software tint. The display's own white point is not writable from Windows, and HDR mode may ignore the tint.
- `assets/display2026.png` is Apple's product image. Keep this repo private, or replace it before publishing.
- Departure Mono is SIL OFL (see `assets/DepartureMono-LICENSE.txt`). UI is built with Slint (GPLv3 / royalty-free / commercial licence).
