//! Chat attachments: pick a file / image / folder (system dialog on a background
//! thread), keep the files waiting to send, and open a folder from a chat link.
//!
//! Every file is just bytes, so they send the same way. A folder is NOT sent: it
//! becomes a link in the input (the folder name); clicking it opens that folder.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// State of one attachment.
#[derive(Clone, Debug, PartialEq)]
pub enum FileState {
    /// Ready (the file is there to send).
    Ready,
    /// Pick or read failed.
    Bad(String),
}

/// What the user picked from the "+" menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    File,
    Image,
    Dir,
}

/// One file the user picked.
#[derive(Clone, Debug)]
pub struct AttFile {
    pub name: String,
    /// Path that will be sent (used when the UltraViewer file transfer is wired up).
    #[allow(dead_code)]
    pub path: PathBuf,
    pub size: u64,
    /// Image (draw a picture icon instead of a page).
    pub is_image: bool,
    pub state: FileState,
    pub clock: String,
}

pub type AttList = Arc<Mutex<Vec<AttFile>>>;
/// Folder just picked, waiting for the UI to insert it into the input.
pub type DirQueue = Arc<Mutex<Vec<PathBuf>>>;

/// Common image extensions.
const IMG_EXT: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "tif", "tiff", "svg", "heic", "ico"];

/// True when the path looks like an image (by extension).
pub fn is_image(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMG_EXT.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Open the picker on a background thread. Files/images go into `files`; folders into `dirs`.
pub fn pick(kind: Pick, files: AttList, dirs: DirQueue, clock: String, wake: impl Fn() + Send + 'static) {
    std::thread::Builder::new()
        .name("pick-file".into())
        .spawn(move || {
            match pick_dialog(kind) {
                Ok(v) => {
                    for p in v {
                        if kind == Pick::Dir || p.is_dir() {
                            dirs.lock().unwrap().push(p);
                        } else {
                            add_file(&files, &p, &clock);
                        }
                    }
                }
                Err(e) => files.lock().unwrap().push(AttFile {
                    name: "Không mở được hộp chọn".into(),
                    path: PathBuf::new(),
                    size: 0,
                    is_image: kind == Pick::Image,
                    state: FileState::Bad(e),
                    clock,
                }),
            }
            wake();
        })
        .ok();
}

/// Add one file (not a folder) to the send queue.
pub fn add_file(files: &AttList, p: &Path, clock: &str) {
    let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
    let n = std::fs::metadata(p).map(|m| m.len());
    files.lock().unwrap().push(AttFile {
        name,
        path: p.to_path_buf(),
        size: n.as_ref().copied().unwrap_or(0),
        is_image: is_image(p),
        state: match n {
            Ok(_) => FileState::Ready,
            Err(e) => FileState::Bad(format!("không đọc được tệp: {e}")),
        },
        clock: clock.into(),
    });
}

/// Open a folder with the file manager of the current session (link inside a chat message).
pub fn open_dir(p: &Path) {
    #[cfg(target_os = "linux")]
    {
        // Try the XFCE file manager first (the right session, e.g. :2), then xdg-open.
        for cmd in ["thunar", "xdg-open"] {
            if std::process::Command::new(cmd).arg(p).env("GDK_BACKEND", "x11").spawn().is_ok() {
                return;
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(p).spawn();
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(p).spawn();
    }
}

/// Zip the tree `root` into `dest` (keep subfolders; names in the zip start with
/// the root folder name). Returns the zip path and its size.
#[allow(dead_code)] // kept for sending a folder as a file; the UI currently inserts a link
pub fn zip_dir(root: &Path, dest: &Path) -> Result<(PathBuf, u64), String> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let f = std::fs::File::create(dest).map_err(|e| format!("không tạo được {}: {e}", dest.display()))?;
    let mut z = zip::ZipWriter::new(f);
    let opt = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
    let root_name = root.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rel = d.strip_prefix(root).unwrap_or(Path::new(""));
        let dir_name = Path::new(&root_name).join(rel);
        z.add_directory(zip_name(&dir_name) + "/", opt).map_err(|e| e.to_string())?;
        let mut entries: Vec<_> = std::fs::read_dir(&d)
            .map_err(|e| format!("không đọc được {}: {e}", d.display()))?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            // Do not follow symlinks (avoids loops and data outside the folder).
            let kind = match e.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                stack.push(p);
            } else if kind.is_file() {
                let file_name = Path::new(&root_name).join(p.strip_prefix(root).unwrap_or(&p));
                z.start_file(zip_name(&file_name), opt).map_err(|e| e.to_string())?;
                let mut src = std::fs::File::open(&p).map_err(|e| format!("không mở được {}: {e}", p.display()))?;
                std::io::copy(&mut src, &mut z).map_err(|e| format!("nén {} hỏng: {e}", p.display()))?;
            }
        }
    }
    let mut f = z.finish().map_err(|e| format!("đóng zip hỏng: {e}"))?;
    f.flush().ok();
    let n = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    Ok((dest.to_path_buf(), n))
}

/// Entry name inside the zip: `/` separators, no `\`.
#[allow(dead_code)]
fn zip_name(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// System file dialog. Linux: `zenity` (GNOME/XFCE), fall back to `kdialog`.
fn pick_dialog(kind: Pick) -> Result<Vec<PathBuf>, String> {
    #[cfg(target_os = "linux")]
    {
        let mut cmd = std::process::Command::new("zenity");
        // zenity 4 (GTK4) opens the picker through xdg-desktop-portal by default. The portal
        // belongs to a different Wayland session, so the dialog shows on a screen the operator
        // cannot see (seen 21:57 on :2). Force drawing straight onto the current X screen.
        cmd.env("GDK_BACKEND", "x11").env("GDK_DEBUG", "no-portals").env("GTK_USE_PORTAL", "0");
        cmd.args(["--file-selection", "--title"]);
        match kind {
            Pick::Dir => {
                cmd.args(["Chọn thư mục (chèn liên kết vào tin)", "--directory"]);
            }
            Pick::Image => {
                let filter = format!(
                    "--file-filter=Hình ảnh | {}",
                    IMG_EXT.iter().flat_map(|e| [format!("*.{e}"), format!("*.{}", e.to_uppercase())]).collect::<Vec<_>>().join(" ")
                );
                cmd.args(["Chọn hình ảnh để gửi", "--multiple", "--separator=\n", &filter]);
            }
            Pick::File => {
                cmd.args(["Chọn tệp để gửi", "--multiple", "--separator=\n"]);
            }
        }
        let out = match cmd.output() {
            Ok(o) => o,
            Err(_) => {
                let mut k = std::process::Command::new("kdialog");
                k.env("QT_QPA_PLATFORM", "xcb");
                if kind == Pick::Dir {
                    k.arg("--getexistingdirectory");
                } else {
                    k.args(["--getopenfilename", ".", "", "--multiple", "--separate-output"]);
                }
                k.output().map_err(|e| format!("không mở được hộp chọn tệp (zenity/kdialog): {e}"))?
            }
        };
        if !out.status.success() {
            return Ok(Vec::new()); // user pressed Cancel
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect())
    }
    #[cfg(target_os = "macos")]
    {
        // Standard macOS picker via AppleScript; one POSIX path per line.
        let script = match kind {
            Pick::Dir => {
                r#"POSIX path of (choose folder with prompt "Chọn thư mục (chèn liên kết vào tin)")"#.to_string()
            }
            Pick::Image | Pick::File => {
                let of_type = if kind == Pick::Image { r#" of type {"public.image"}"# } else { "" };
                let prompt = if kind == Pick::Image { "Chọn hình ảnh để gửi" } else { "Chọn tệp để gửi" };
                format!(
                    "set ds to choose file{of_type} with prompt \"{prompt}\" with multiple selections allowed\n\
                     set kq to \"\"\n\
                     repeat with f in ds\n\
                     set kq to kq & POSIX path of f & linefeed\n\
                     end repeat\n\
                     return kq"
                )
            }
        };
        let out = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|e| format!("không mở được hộp chọn tệp (osascript): {e}"))?;
        if !out.status.success() {
            return Ok(Vec::new()); // user pressed Cancel (osascript returns error -128)
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = kind;
        Err("Hộp chọn tệp chưa hỗ trợ trên nền này".into())
    }
}

/// "1,2 MB", Vietnamese decimal comma.
pub fn human_size(n: u64) -> String {
    let f = n as f64;
    let s = if n < 1024 {
        format!("{n} B")
    } else if f < 1024.0 * 1024.0 {
        format!("{:.1} KB", f / 1024.0)
    } else if f < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", f / 1024.0 / 1024.0)
    } else {
        format!("{:.2} GB", f / 1024.0 / 1024.0 / 1024.0)
    };
    s.replace('.', ",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_dir_keeps_tree_and_bytes() {
        let root = std::env::temp_dir().join(format!("xr-nen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("con/chau")).unwrap();
        std::fs::write(root.join("a.txt"), "xin chào").unwrap();
        std::fs::write(root.join("con/b.bin"), [0u8, 1, 2, 255]).unwrap();
        std::fs::write(root.join("con/chau/c.txt"), "sâu").unwrap();
        let dest = std::env::temp_dir().join(format!("xr-nen-{}.zip", std::process::id()));
        let (p, n) = zip_dir(&root, &dest).unwrap();
        assert!(n > 0);
        let mut z = zip::ZipArchive::new(std::fs::File::open(&p).unwrap()).unwrap();
        let root_name = root.file_name().unwrap().to_string_lossy().into_owned();
        let read = |z: &mut zip::ZipArchive<std::fs::File>, t: &str| {
            use std::io::Read;
            let mut v = Vec::new();
            z.by_name(t).unwrap().read_to_end(&mut v).unwrap();
            v
        };
        assert_eq!(read(&mut z, &format!("{root_name}/a.txt")), "xin chào".as_bytes());
        assert_eq!(read(&mut z, &format!("{root_name}/con/b.bin")), vec![0u8, 1, 2, 255]);
        assert_eq!(read(&mut z, &format!("{root_name}/con/chau/c.txt")), "sâu".as_bytes());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&dest);
    }

    #[test]
    fn image_ext_detect() {
        assert!(is_image(Path::new("/a/b/Anh.PNG")));
        assert!(is_image(Path::new("x.jpeg")));
        assert!(!is_image(Path::new("bao-cao.pdf")));
        assert!(!is_image(Path::new("khong-duoi")));
    }

    #[test]
    fn human_size_vi() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1,5 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5,0 MB");
    }
}
