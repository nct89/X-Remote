//! uvclient — native UltraViewer console for macOS and Linux.
//!
//! The visible surface is deliberately small: four fields and one button, in the
//! same shape as UltraViewer's own window.
//!
//! Two ways to run:
//! * `--agent <host:port>` — talk to `uvagent` on the Windows machine;
//! * `--uv --id … --pw …` — talk to the UltraViewer servers directly,
//!   nothing to install on the remote machine.

#[cfg(target_os = "linux")]
mod shadow;
mod attach;
mod hostmode;
mod history;
mod net;
mod theme;
mod ui;
mod uv;

use eframe::egui;

const HELP: &str = "\
uvclient [--agent <host:port>] [--connect] [--screenshot <file.jpg>]

Real UltraViewer mode (no agent on the remote machine):
  uvclient --uv --id <remote ID> --pw <password> [--connect]
           [--own-id <my ID>] [--own-pw <my password>]
           [--shot-delay <seconds>] [--screenshot <file.jpg>]

  --own-id/--own-pw  also work when you open the app and type the partner ID
  --connect          join the session immediately, no button press
  --shot-delay N     wait N seconds (and for a picture) before the shot
  --screenshot       save the window as JPEG and exit
";

/// Enable the IME on the X display being controlled.
///
/// UltraViewer forwards real keys only. A controller IME swallows the key and
/// inserts a precomposed character as a fake Unicode key, which UltraViewer
/// drops, so this side sees "t o <Backspace>" and loses the letter (seen
/// 2026-11-05 21:20). Turn the controller IME off for the UltraViewer window
/// so THIS machine composes from raw keys. An X session (e.g. XFCE `:2`) next
/// to Wayland shares one fcitx5 on the same D-Bus, but fcitx5 only attaches to
/// the display it opened on. Ask it to add an XIM connection to `$DISPLAY`
/// (repeat calls return "already exists"), then set `XMODIFIERS` so X-Remote's
/// text fields can use the IME.
#[cfg(target_os = "linux")]
fn bind_ime_x11() {
    let Some(display) = std::env::var("DISPLAY").ok().filter(|s| !s.is_empty()) else { return };
    if std::env::var_os("XREMOTE_NO_IME").is_some() {
        return;
    }
    let out = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--dest=org.fcitx.Fcitx5",
            "/controller",
            "org.fcitx.Fcitx.Controller1.OpenX11Connection",
            &format!("string:{display}"),
        ])
        .output();
    let attached = match &out {
        Ok(o) => o.status.success() || String::from_utf8_lossy(&o.stderr).contains("already exists"),
        Err(_) => false,
    };
    if attached && std::env::var_os("XMODIFIERS").map_or(true, |v| v.is_empty()) {
        std::env::set_var("XMODIFIERS", "@im=fcitx");
    }
    eprintln!("[x-remote] fcitx5 IME for {display}: {}", if attached { "connected" } else { "unavailable" });
}

/// Window icon. Straight RGBA, 128×128, scaled from `x-remote-logo.png`.
fn window_icon() -> egui::IconData {
    const N: u32 = 128;
    let rgba = include_bytes!("../assets/icon-128.rgba");
    debug_assert_eq!(rgba.len(), (N * N * 4) as usize);
    egui::IconData {
        rgba: rgba.to_vec(),
        width: N,
        height: N,
    }
}

fn main() -> Result<(), eframe::Error> {
    let mut engine: Option<String> = None;
    let mut autoconnect = false;
    let mut screenshot: Option<String> = None;
    let mut uv_mode = false;
    let mut peer_id = String::new();
    let mut peer_pw = String::new();
    let mut own_id = String::new();
    let mut own_pw = String::new();
    let mut shot_delay: u64 = 0;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--agent" => engine = it.next(),
            "--uv" => uv_mode = true,
            "--id" => peer_id = it.next().unwrap_or_default(),
            "--pw" => peer_pw = it.next().unwrap_or_default(),
            "--own-id" => own_id = it.next().unwrap_or_default(),
            "--own-pw" => own_pw = it.next().unwrap_or_default(),
            "--shot-delay" => shot_delay = it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            "--connect" => autoconnect = true,
            "--screenshot" => screenshot = it.next(),
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            _ => {}
        }
    }

    // No `--own-id`: use this machine's saved ID, stable across launches.
    if own_id.trim().is_empty() {
        if let Some(path) = ui::id_file() {
            own_id = ui::stored_id(&path);
        }
    }

    #[cfg(target_os = "linux")]
    bind_ime_x11();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(ui::WIN_SIZE)
            .with_min_inner_size(ui::WIN_SIZE)
            .with_icon(window_icon())
            .with_app_id("x-remote")
            .with_title("X-Remote"),
        ..Default::default()
    };

    eframe::run_native(
        "X-Remote",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            let mut app = ui::App::new(engine);
            if uv_mode {
                app.set_uv(peer_id, peer_pw, own_id, own_pw);
            } else {
                // `--own-id/--own-pw` also apply when opening the app and typing the ID by hand.
                app.set_own(own_id, own_pw);
                app.set_peer(peer_id, peer_pw);
            }
            app.set_shot_delay(shot_delay);
            if autoconnect {
                app.autoconnect();
            }
            if let Some(path) = screenshot {
                app.screenshot_to(std::path::PathBuf::from(path));
            }
            Ok(Box::new(app))
        }),
    )
}
