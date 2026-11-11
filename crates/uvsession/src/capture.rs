//! Host-side X11 screen capture (Linux only).
//!
//! `GetImage` on the root window returns the full screen as RGB; [`crate::frame`]
//! encodes it into an UltraViewer frame.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};
use x11rb::rust_connection::RustConnection;

/// SysV shared-memory segment attached to the X server (MIT-SHM).
struct ShmSeg {
    seg: u32,
    ptr: *mut u8,
    len: usize,
}

/// Screen capturer bound to one X11 screen.
pub struct Capturer {
    conn: RustConnection,
    root: u32,
    pub width: u16,
    pub height: u16,
    /// Bytes per pixel in the server image (usually 4: BGRX).
    bpp: usize,
    /// MIT-SHM: the server writes the image into shared memory instead of
    /// sending ~8 MB over the socket (measured 19:59: 1080p `GetImage` takes
    /// 10.5–16 ms/frame — the slowest step).
    shm: Option<ShmSeg>,
}

// The shm pointer is only read by the thread that owns `Capturer`.
unsafe impl Send for Capturer {}

impl Capturer {
    /// Connect to the X server (`$DISPLAY`, falling back to `:0` / `:1`) and read the root screen size.
    pub fn new() -> Result<Capturer, String> {
        let (conn, screen_num) = x11rb::connect(None)
            .or_else(|_| x11rb::connect(Some(":0")))
            .or_else(|_| x11rb::connect(Some(":1")))
            .map_err(|e| format!("X11 connect failed: {e}"))?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;
        let width = screen.width_in_pixels;
        let height = screen.height_in_pixels;
        // Bytes/pixel from root depth (24/32 → 4 bytes).
        let bpp = if screen.root_depth > 16 { 4 } else { 2 };
        let shm = if std::env::var_os("XREMOTE_NO_SHM").is_some() {
            None
        } else {
            match open_shm(&conn, width as usize * height as usize * bpp) {
                Ok(s) => Some(s),
                Err(e) => {
                    eprintln!("[uv-host] MIT-SHM unavailable ({e}) — falling back to GetImage");
                    None
                }
            }
        };
        Ok(Capturer { conn, root, width, height, bpp, shm })
    }

    /// Capturing via MIT-SHM.
    pub fn uses_shm(&self) -> bool {
        self.shm.is_some()
    }

    /// Capture the full screen as RGB (3 bytes/pixel, top to bottom).
    pub fn grab_rgb(&self) -> Result<Vec<u8>, String> {
        if let Some(s) = &self.shm {
            use x11rb::protocol::shm::ConnectionExt as _;
            let r = self
                .conn
                .shm_get_image(
                    self.root,
                    0,
                    0,
                    self.width,
                    self.height,
                    !0,
                    ImageFormat::Z_PIXMAP.into(),
                    s.seg,
                    0,
                )
                .map_err(|e| format!("ShmGetImage send failed: {e}"))?
                .reply();
            if r.is_ok() {
                // SAFETY: the shm segment lives until `Capturer` drops; the server
                // finished writing when the reply arrives.
                let data = unsafe { std::slice::from_raw_parts(s.ptr, s.len) };
                return Ok(bgrx_to_rgb(data, self.bpp));
            }
        }
        let img = self
            .conn
            .get_image(ImageFormat::Z_PIXMAP, self.root, 0, 0, self.width, self.height, !0)
            .map_err(|e| format!("GetImage send failed: {e}"))?
            .reply()
            .map_err(|e| format!("GetImage reply failed: {e}"))?;
        Ok(bgrx_to_rgb(&img.data, self.bpp))
    }
}

impl Drop for Capturer {
    fn drop(&mut self) {
        if let Some(s) = self.shm.take() {
            use x11rb::protocol::shm::ConnectionExt as _;
            let _ = self.conn.shm_detach(s.seg);
            let _ = self.conn.flush();
            unsafe {
                libc::shmdt(s.ptr as *const libc::c_void);
            }
        }
    }
}

/// Create a SysV shm segment of `len` bytes and attach it to the X server.
fn open_shm(conn: &RustConnection, len: usize) -> Result<ShmSeg, String> {
    use x11rb::protocol::shm::ConnectionExt as _;
    conn.shm_query_version()
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| format!("server has no MIT-SHM: {e}"))?;
    let id = unsafe { libc::shmget(libc::IPC_PRIVATE, len, libc::IPC_CREAT | 0o600) };
    if id < 0 {
        return Err(format!("shmget failed: {}", std::io::Error::last_os_error()));
    }
    let ptr = unsafe { libc::shmat(id, std::ptr::null(), 0) };
    if ptr as isize == -1 {
        unsafe { libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut()) };
        return Err(format!("shmat failed: {}", std::io::Error::last_os_error()));
    }
    let seg = conn.generate_id().map_err(|e| e.to_string())?;
    let attached = conn
        .shm_attach(seg, id as u32, false)
        .map_err(|e| e.to_string())
        .and_then(|c| c.check().map_err(|e| format!("ShmAttach failed: {e}")));
    // Mark for deletion now: the segment is freed when both sides detach (even on crash).
    unsafe { libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut()) };
    if let Err(e) = attached {
        unsafe { libc::shmdt(ptr) };
        return Err(e);
    }
    Ok(ShmSeg { seg, ptr: ptr as *mut u8, len })
}

/// Convert a server pixel buffer (little-endian BGRX, `bpp` bytes/pixel) to 3-byte RGB.
fn bgrx_to_rgb(data: &[u8], bpp: usize) -> Vec<u8> {
    if bpp < 3 {
        // 16-bit (RGB565/555) — rare; approximate.
        let mut out = Vec::with_capacity(data.len() / 2 * 3);
        for px in data.chunks_exact(2) {
            let v = u16::from_le_bytes([px[0], px[1]]);
            // Assume RGB565.
            let r = ((v >> 11) & 0x1f) as u8;
            let g = ((v >> 5) & 0x3f) as u8;
            let b = (v & 0x1f) as u8;
            out.extend_from_slice(&[r << 3, g << 2, b << 3]);
        }
        return out;
    }
    // Preallocated buffer written by index (the compiler can vectorize), instead of
    // `extend_from_slice` per pixel.
    let mut out = vec![0u8; data.len() / bpp * 3];
    for (o, px) in out.chunks_exact_mut(3).zip(data.chunks_exact(bpp)) {
        // X11 Z_PIXMAP TrueColor little-endian: byte 0=B, 1=G, 2=R.
        o[0] = px[2];
        o[1] = px[1];
        o[2] = px[0];
    }
    out
}
