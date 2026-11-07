//! Host-side screen capture on macOS — same interface as the X11 build (`capture.rs`):
//! `Capturer::new()`, `width`/`height`, `grab_rgb()`.
//!
//! Uses CoreGraphics `CGDisplayCreateImage` (measured 06/11: still works on macOS 15.8;
//! a 1920×1080 display takes ~60 ms, 32-bit BGRA `bitmapInfo = 0x2002`). Retina frames
//! (pixel size is 2× point size) are scaled down to points — less bandwidth, and controller
//! mouse coordinates match CGEvent points directly. Without Screen Recording permission
//! macOS returns only the wallpaper and menu bar (no windows) — see [`request_screen_record`].

use std::ffi::c_void;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct CGPoint {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct CGSize {
    pub w: f64,
    pub h: f64,
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct CGRect {
    pub o: CGPoint,
    pub s: CGSize,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub(crate) fn CGMainDisplayID() -> u32;
    pub(crate) fn CGDisplayBounds(d: u32) -> CGRect;
    fn CGDisplayCreateImage(d: u32) -> *mut c_void;
    fn CGImageGetWidth(i: *mut c_void) -> usize;
    fn CGImageGetHeight(i: *mut c_void) -> usize;
    fn CGImageGetBytesPerRow(i: *mut c_void) -> usize;
    fn CGImageGetBitsPerPixel(i: *mut c_void) -> usize;
    fn CGImageGetDataProvider(i: *mut c_void) -> *mut c_void;
    fn CGDataProviderCopyData(p: *mut c_void) -> *mut c_void;
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDataGetLength(d: *mut c_void) -> isize;
    fn CFDataGetBytePtr(d: *mut c_void) -> *const u8;
    pub(crate) fn CFRelease(o: *mut c_void);
}

/// True if Screen Recording is granted; otherwise ask macOS to show the prompt (once).
pub fn request_screen_record() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess() }
}

/// Main-display capturer.
pub struct Capturer {
    display: u32,
    /// Outgoing frame size = main display point size.
    pub width: u16,
    pub height: u16,
}

impl Capturer {
    pub fn new() -> Result<Capturer, String> {
        let display = unsafe { CGMainDisplayID() };
        let b = unsafe { CGDisplayBounds(display) };
        if b.s.w < 1.0 || b.s.h < 1.0 {
            return Err("failed to read main display size".into());
        }
        Ok(Capturer { display, width: b.s.w.round() as u16, height: b.s.h.round() as u16 })
    }

    /// Capture the main display as RGB (3 bytes/pixel, `width`×`height`, top to bottom).
    pub fn grab_rgb(&self) -> Result<Vec<u8>, String> {
        unsafe {
            let img = CGDisplayCreateImage(self.display);
            if img.is_null() {
                return Err("CGDisplayCreateImage returned NULL".into());
            }
            let (w, h) = (CGImageGetWidth(img), CGImageGetHeight(img));
            let bpr = CGImageGetBytesPerRow(img);
            let bpp = CGImageGetBitsPerPixel(img);
            let data = CGDataProviderCopyData(CGImageGetDataProvider(img));
            if data.is_null() || bpp != 32 {
                if !data.is_null() {
                    CFRelease(data);
                }
                CFRelease(img);
                return Err(format!("unsupported image format ({bpp} bits/pixel)"));
            }
            let n = CFDataGetLength(data) as usize;
            let p = std::slice::from_raw_parts(CFDataGetBytePtr(data), n);
            let rgb = bgra_to_rgb_scaled(p, w, h, bpr, self.width as usize, self.height as usize);
            CFRelease(data);
            CFRelease(img);
            Ok(rgb)
        }
    }
}

/// Convert BGRA (`bpr` bytes/row) to RGB of size `dw`×`dh`. If the image is larger
/// than the target (Retina, integer factor k), average each k×k block; if equal, copy.
pub(crate) fn bgra_to_rgb_scaled(p: &[u8], w: usize, h: usize, bpr: usize, dw: usize, dh: usize) -> Vec<u8> {
    let mut out = vec![0u8; dw * dh * 3];
    let k = (w / dw.max(1)).max(1);
    if k == 1 {
        for y in 0..dh.min(h) {
            let row = &p[y * bpr..];
            let o = &mut out[y * dw * 3..(y + 1) * dw * 3];
            for x in 0..dw.min(w) {
                o[x * 3] = row[x * 4 + 2];
                o[x * 3 + 1] = row[x * 4 + 1];
                o[x * 3 + 2] = row[x * 4];
            }
        }
        return out;
    }
    let div = (k * k) as u32;
    for y in 0..dh {
        for x in 0..dw {
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
            for dy in 0..k {
                let yy = (y * k + dy).min(h - 1);
                let row = &p[yy * bpr..];
                for dx in 0..k {
                    let xx = (x * k + dx).min(w - 1) * 4;
                    b += row[xx] as u32;
                    g += row[xx + 1] as u32;
                    r += row[xx + 2] as u32;
                }
            }
            let i = (y * dw + x) * 3;
            out[i] = (r / div) as u8;
            out[i + 1] = (g / div) as u8;
            out[i + 2] = (b / div) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_channels_and_row_pad() {
        // 2×1, each row padded by 8 bytes (bpr = 16).
        let p = [10u8, 20, 30, 255, 40, 50, 60, 255, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(bgra_to_rgb_scaled(&p, 2, 1, 16, 2, 1), vec![30, 20, 10, 60, 50, 40]);
    }

    #[test]
    fn retina_point_scale_averages() {
        // 2×2 BGRA → 1×1: average of four pixels.
        let p = [0u8, 0, 100, 255, 0, 0, 200, 255, 0, 0, 0, 255, 0, 0, 100, 255];
        assert_eq!(bgra_to_rgb_scaled(&p, 2, 2, 8, 1, 1), vec![100, 0, 0]);
    }
}
