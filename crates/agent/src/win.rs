//! Window enumeration, GDI capture, and input injection.

use crate::ffi::*;
use core::ffi::c_void;
use std::ffi::OsString;
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::thread::sleep;
use std::time::Duration;

const MOUSEEVENTF_LEFTDOWN: u32 = 0x0002;
const MOUSEEVENTF_LEFTUP: u32 = 0x0004;
const MOUSEEVENTF_RIGHTDOWN: u32 = 0x0008;
const MOUSEEVENTF_RIGHTUP: u32 = 0x0010;
const MOUSEEVENTF_MIDDLEDOWN: u32 = 0x0020;
const MOUSEEVENTF_MIDDLEUP: u32 = 0x0040;

#[derive(Clone, Debug)]
pub struct WinInfo {
    pub hwnd: HWND,
    pub title: String,
    pub w: i32,
    pub h: i32,
    pub pid: u32,
}

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..end]).to_string_lossy().into_owned()
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let out = &mut *(lparam as *mut Vec<WinInfo>);

    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }

    let mut title_buf = [0u16; 512];
    let n = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
    if n <= 0 {
        return 1;
    }

    let rect = visible_rect(hwnd);
    if rect.width() < 64 || rect.height() < 64 {
        return 1;
    }

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);

    out.push(WinInfo {
        hwnd,
        title: wide_to_string(&title_buf),
        w: rect.width(),
        h: rect.height(),
        pid,
    });

    1
}

pub fn list_windows() -> Vec<WinInfo> {
    let mut out: Vec<WinInfo> = Vec::new();
    unsafe {
        EnumWindows(Some(enum_proc), &mut out as *mut Vec<WinInfo> as LPARAM);
    }
    out.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    out
}

pub fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    wide_to_string(&buf)
}

pub fn is_alive(hwnd: HWND) -> bool {
    unsafe { IsWindow(hwnd) != 0 && IsWindowVisible(hwnd) != 0 }
}

/// Find a window by numeric hwnd handle or by title substring (case-insensitive).
pub fn find_window(hwnd: Option<HWND>, title_substr: Option<&str>) -> Option<WinInfo> {
    if let Some(h) = hwnd {
        if is_alive(h) {
            let rect = visible_rect(h);
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(h, &mut pid) };
            return Some(WinInfo {
                hwnd: h,
                title: window_title(h),
                w: rect.width(),
                h: rect.height(),
                pid,
            });
        }
        return None;
    }

    let needle = title_substr?.to_lowercase();
    list_windows()
        .into_iter()
        .find(|w| w.title.to_lowercase().contains(&needle))
}

/// Screen-space rectangle the user actually sees.
///
/// `GetWindowRect` on Windows 10/11 includes an invisible resize border, which
/// shows up as a black strip along the right and bottom of the captured image
/// (the DWM simply never paints there). `DWMWA_EXTENDED_FRAME_BOUNDS` gives the
/// visible rectangle; we fall back to `GetWindowRect` when DWM is unavailable
/// (for example under Wine).
pub fn visible_rect(hwnd: HWND) -> RECT {
    let mut full = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut full) };

    let mut ext = RECT::default();
    let hr = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut ext as *mut RECT as *mut c_void,
            size_of::<RECT>() as u32,
        )
    };

    let plausible = hr == 0
        && ext.width() > 0
        && ext.height() > 0
        && ext.left >= full.left - 32
        && ext.top >= full.top - 32
        && ext.right <= full.right + 32
        && ext.bottom <= full.bottom + 32;

    if plausible {
        ext
    } else {
        full
    }
}

// ---------------------------------------------------------------------------
// capture
// ---------------------------------------------------------------------------

pub struct Capture {
    pub hwnd: HWND,
    hdc_mem: HDC,
    hbm: HBITMAP,
    old: HGDIOBJ,
    /// bitmap size — equals `GetWindowRect`, the coordinate space PrintWindow
    /// renders into.
    bw: i32,
    bh: i32,
    /// visible size after cropping away the invisible border
    pub w: i32,
    pub h: i32,
    crop_x: i32,
    crop_y: i32,
    bgra: Vec<u8>,
    pub rgb: Vec<u8>,
}

fn crop_of(full: &RECT, vis: &RECT) -> (i32, i32, i32, i32) {
    let fw = full.width().max(1);
    let fh = full.height().max(1);
    let x = (vis.left - full.left).clamp(0, fw - 1);
    let y = (vis.top - full.top).clamp(0, fh - 1);
    let w = vis.width().clamp(1, fw - x);
    let h = vis.height().clamp(1, fh - y);
    (x, y, w, h)
}

impl Capture {
    pub fn new(hwnd: HWND) -> Option<Capture> {
        if !is_alive(hwnd) {
            return None;
        }

        let mut full = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut full) };
        if full.width() <= 0 || full.height() <= 0 {
            return None;
        }

        let vis = visible_rect(hwnd);
        let (crop_x, crop_y, w, h) = crop_of(&full, &vis);

        unsafe {
            let hdc_screen = GetDC(hwnd);
            if hdc_screen == 0 {
                return None;
            }
            let hdc_mem = CreateCompatibleDC(hdc_screen);
            let hbm = CreateCompatibleBitmap(hdc_screen, full.width(), full.height());
            ReleaseDC(hwnd, hdc_screen);

            if hdc_mem == 0 || hbm == 0 {
                return None;
            }
            let old = SelectObject(hdc_mem, hbm);

            Some(Capture {
                hwnd,
                hdc_mem,
                hbm,
                old,
                bw: full.width(),
                bh: full.height(),
                w,
                h,
                crop_x,
                crop_y,
                bgra: vec![0u8; (full.width() as usize) * (full.height() as usize) * 4],
                rgb: vec![0u8; (w as usize) * (h as usize) * 3],
            })
        }
    }

    fn resize_bitmap(&mut self, w: i32, h: i32) -> bool {
        if w <= 0 || h <= 0 {
            return false;
        }
        unsafe {
            SelectObject(self.hdc_mem, self.old);
            DeleteObject(self.hbm);

            let hdc_screen = GetDC(self.hwnd);
            if hdc_screen == 0 {
                return false;
            }
            let hbm = CreateCompatibleBitmap(hdc_screen, w, h);
            ReleaseDC(self.hwnd, hdc_screen);
            if hbm == 0 {
                return false;
            }
            self.old = SelectObject(self.hdc_mem, hbm);
            self.hbm = hbm;
        }
        self.bw = w;
        self.bh = h;
        self.bgra.resize((w as usize) * (h as usize) * 4, 0);
        true
    }

    /// Copy the bitmap into `self.bgra`. The bitmap must not stay selected into
    /// the DC while `GetDIBits` runs, so we detach and reattach it.
    fn read_bits(&mut self) -> bool {
        let mut bi = BITMAPINFO::default();
        bi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = self.bw;
        bi.bmiHeader.biHeight = -self.bh; // top-down
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;

        unsafe {
            SelectObject(self.hdc_mem, self.old);
            let lines = GetDIBits(
                self.hdc_mem,
                self.hbm,
                0,
                self.bh as u32,
                self.bgra.as_mut_ptr() as *mut c_void,
                &mut bi,
                DIB_RGB_COLORS,
            );
            SelectObject(self.hdc_mem, self.hbm);
            lines == self.bh
        }
    }

    /// BGRA (whole window) -> RGB (visible area only).
    fn to_rgb(&mut self) {
        let stride = self.bw as usize;
        let (cx, cy) = (self.crop_x as usize, self.crop_y as usize);
        let (w, h) = (self.w as usize, self.h as usize);

        for row in 0..h {
            let src_row = (cy + row) * stride;
            for col in 0..w {
                let s = (src_row + cx + col) * 4;
                let d = (row * w + col) * 3;
                self.rgb[d] = self.bgra[s + 2];
                self.rgb[d + 1] = self.bgra[s + 1];
                self.rgb[d + 2] = self.bgra[s];
            }
        }
    }

    fn looks_blank(&self) -> bool {
        let px = (self.w as usize) * (self.h as usize);
        if px == 0 || self.rgb.len() < 3 {
            return true;
        }
        let first = [self.rgb[0], self.rgb[1], self.rgb[2]];
        let step = (px / 512).max(1);
        let mut i = 0usize;
        while i < px {
            let o = i * 3;
            if self.rgb[o] != first[0] || self.rgb[o + 1] != first[1] || self.rgb[o + 2] != first[2]
            {
                return false;
            }
            i += step;
        }
        true
    }

    /// Grab one frame into `self.rgb`. Returns false when the window vanished.
    pub fn grab(&mut self) -> bool {
        if !is_alive(self.hwnd) {
            return false;
        }

        let mut full = RECT::default();
        unsafe { GetWindowRect(self.hwnd, &mut full) };
        if full.width() <= 0 || full.height() <= 0 {
            return false;
        }

        if full.width() != self.bw || full.height() != self.bh {
            if !self.resize_bitmap(full.width(), full.height()) {
                return false;
            }
        }

        let vis = visible_rect(self.hwnd);
        let (cx, cy, cw, ch) = crop_of(&full, &vis);
        self.crop_x = cx;
        self.crop_y = cy;
        if cw != self.w || ch != self.h {
            self.w = cw;
            self.h = ch;
            self.rgb.resize((cw as usize) * (ch as usize) * 3, 0);
        }

        // PrintWindow renders even when the window is occluded, which matters
        // inside a VM where the UltraViewer window is never in front.
        let mut ok = unsafe { PrintWindow(self.hwnd, self.hdc_mem, PW_RENDERFULLCONTENT) } != 0
            && self.read_bits();

        if ok {
            self.to_rgb();
            if self.looks_blank() {
                ok = false;
            }
        }

        if !ok {
            // Fallback: copy straight from the screen. Only correct when the
            // window is actually visible, but better than a black frame.
            unsafe {
                let hdc_screen = GetDC(0);
                if hdc_screen != 0 {
                    let blitted = BitBlt(
                        self.hdc_mem,
                        0,
                        0,
                        self.bw,
                        self.bh,
                        hdc_screen,
                        full.left,
                        full.top,
                        SRCCOPY,
                    );
                    ReleaseDC(0, hdc_screen);
                    ok = blitted != 0 && self.read_bits();
                }
            }
            if ok {
                self.to_rgb();
            }
        }

        ok
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.hdc_mem, self.old);
            DeleteObject(self.hbm);
            DeleteDC(self.hdc_mem);
        }
    }
}

/// Cheap change detector: FNV-1a over a sampled subset of the RGB buffer.
pub fn hash(rgb: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let step = (rgb.len() / 4096).max(1);
    let mut i = 0usize;
    while i < rgb.len() {
        h ^= rgb[i] as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
        i += step;
    }
    h
}

// ---------------------------------------------------------------------------
// input injection
// ---------------------------------------------------------------------------

fn send(input: INPUT) -> u32 {
    unsafe { SendInput(1, &input, size_of::<INPUT>() as i32) }
}

pub fn focus(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }

        let foreground = GetForegroundWindow();
        if foreground == hwnd {
            return;
        }

        // A background process normally cannot steal the foreground; attaching
        // to the current foreground thread's input queue lifts that restriction
        // long enough for SetForegroundWindow to take effect.
        let foreground_thread = GetWindowThreadProcessId(foreground, std::ptr::null_mut());
        let self_thread = GetCurrentThreadId();

        if foreground_thread != 0 && foreground_thread != self_thread {
            AttachThreadInput(self_thread, foreground_thread, 1);
            BringWindowToTop(hwnd);
            SetForegroundWindow(hwnd);
            AttachThreadInput(self_thread, foreground_thread, 0);
        } else {
            BringWindowToTop(hwnd);
            SetForegroundWindow(hwnd);
        }
    }
    sleep(Duration::from_millis(120));
}

/// Make sure keystrokes and clicks land on the captured window. Without this,
/// input goes to whatever happens to have focus in the guest.
pub fn ensure_foreground(hwnd: HWND) {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground != hwnd {
        focus(hwnd);
    }
}

fn screen_point(hwnd: HWND, nx: f32, ny: f32) -> (i32, i32) {
    let rect = visible_rect(hwnd);
    let x = rect.left + (nx.clamp(0.0, 1.0) * rect.width() as f32) as i32;
    let y = rect.top + (ny.clamp(0.0, 1.0) * rect.height() as f32) as i32;
    (x, y)
}

pub fn mouse_move(hwnd: HWND, nx: f32, ny: f32) {
    let (x, y) = screen_point(hwnd, nx, ny);
    unsafe {
        SetCursorPos(x, y);
    }
}

pub fn mouse_button(hwnd: HWND, nx: f32, ny: f32, button: u8, down: bool) {
    let (x, y) = screen_point(hwnd, nx, ny);
    unsafe {
        SetCursorPos(x, y);
    }

    let flags = match (button, down) {
        (0, true) => MOUSEEVENTF_LEFTDOWN,
        (0, false) => MOUSEEVENTF_LEFTUP,
        (1, true) => MOUSEEVENTF_MIDDLEDOWN,
        (1, false) => MOUSEEVENTF_MIDDLEUP,
        (2, true) => MOUSEEVENTF_RIGHTDOWN,
        (2, false) => MOUSEEVENTF_RIGHTUP,
        _ => return,
    };

    send(INPUT {
        r#type: INPUT_MOUSE,
        u: INPUT_UNION {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
}

pub fn mouse_wheel(delta: i32) {
    send(INPUT {
        r#type: INPUT_MOUSE,
        u: INPUT_UNION {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: delta as u32,
                dwFlags: MOUSEEVENTF_WHEEL,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
}

pub fn key_vk(vk: u16, down: bool) {
    send(INPUT {
        r#type: INPUT_KEYBOARD,
        u: INPUT_UNION {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if down { 0 } else { KEYEVENTF_KEYUP },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
}

/// Type one character as a Unicode keyboard event. Works regardless of the
/// guest keyboard layout, which is what we want for IDs and passwords.
pub fn key_char(ch: char) {
    let mut units = [0u16; 2];
    for unit in ch.encode_utf16(&mut units) {
        for up in [false, true] {
            send(INPUT {
                r#type: INPUT_KEYBOARD,
                u: INPUT_UNION {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: *unit,
                        dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { 0 },
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }
    }
}

pub fn type_text(s: &str) {
    for ch in s.chars() {
        key_char(ch);
        sleep(Duration::from_millis(6));
    }
}

pub fn press_combo(modifier: u16, key: u16) {
    key_vk(modifier, true);
    sleep(Duration::from_millis(20));
    key_vk(key, true);
    sleep(Duration::from_millis(20));
    key_vk(key, false);
    sleep(Duration::from_millis(20));
    key_vk(modifier, false);
}

// ---- clipboard -----------------------------------------------------------

/// Reads the guest clipboard as text (CF_UNICODETEXT), or `None` when the
/// clipboard holds no text or cannot be opened (another app may hold it).
pub fn get_clipboard_text() -> Option<String> {
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return None;
        }
        if OpenClipboard(0) == 0 {
            return None;
        }
        let h = GetClipboardData(CF_UNICODETEXT);
        if h == 0 {
            CloseClipboard();
            return None;
        }
        let ptr = GlobalLock(h) as *const u16;
        if ptr.is_null() {
            CloseClipboard();
            return None;
        }
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
        GlobalUnlock(h);
        CloseClipboard();
        Some(text)
    }
}

/// Replaces the guest clipboard with `s` (CF_UNICODETEXT).
pub fn set_clipboard_text(s: &str) {
    unsafe {
        // UTF-16, NUL-terminated, in a moveable global the clipboard can own.
        let mut wide: Vec<u16> = s.encode_utf16().collect();
        wide.push(0);
        let h = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
        if h == 0 {
            return;
        }
        let dst = GlobalLock(h) as *mut u16;
        if dst.is_null() {
            GlobalFree(h);
            return;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
        GlobalUnlock(h);

        if OpenClipboard(0) == 0 {
            GlobalFree(h);
            return;
        }
        EmptyClipboard();
        if SetClipboardData(CF_UNICODETEXT, h) == 0 {
            // Ownership was not transferred to the clipboard; release it.
            GlobalFree(h);
        }
        CloseClipboard();
    }
}
