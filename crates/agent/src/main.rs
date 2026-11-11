//! uvagent — runs inside the Windows guest and streams one window to the native
//! client over TCP, while replaying the client's mouse/keyboard events.
//!
//! Usage:
//!   uvagent --list
//!   uvagent --title UltraViewer --port 7801
//!   uvagent --hwnd 0x000A1234 --port 7801 --fps 20 --quality 70

mod ffi;
mod win;

use ffi::{HWND, VK_A, VK_CONTROL, VK_RETURN, VK_TAB};
use std::env;
use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use uvproto::{Ctl, Event};

fn usage() -> ! {
    eprintln!(
        "uvagent {ver}\n\
         \n\
         Runs in the Windows guest and streams one window over TCP to the native client.\n\
         \n\
         USAGE:\n\
         \x20 uvagent --list\n\
         \x20 uvagent --title <text> [--port 7801] [--fps 20] [--quality 70]\n\
         \x20 uvagent --hwnd <decimal> [...]\n\
         \n\
         OPTIONS:\n\
         \x20 --list            list open windows, then exit\n\
         \x20 --title <s>       case-insensitive title substring\n\
         \x20 --hwnd <n>        window handle (decimal or 0x hex)\n\
         \x20 --bind <ip>       listen address (default 127.0.0.1)\n\
         \x20 --port <n>        port (default 7801)\n\
         \x20 --fps <n>         max frames per second (default 20)\n\
         \x20 --quality <n>     JPEG quality 1..95 (default 70)",
        ver = env!("CARGO_PKG_VERSION")
    );
    std::process::exit(2);
}

struct Args {
    list: bool,
    title: Option<String>,
    hwnd: Option<HWND>,
    bind: String,
    port: u16,
    fps: u32,
    quality: u8,
}

fn parse_args() -> Args {
    let mut a = Args {
        list: false,
        title: None,
        hwnd: None,
        bind: "127.0.0.1".to_string(),
        port: 7801,
        fps: 20,
        quality: 70,
    };

    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--list" => a.list = true,
            "--title" => a.title = Some(it.next().unwrap_or_else(|| usage())),
            "--hwnd" => {
                let v = it.next().unwrap_or_else(|| usage());
                let parsed = if let Some(hex) = v.strip_prefix("0x") {
                    isize::from_str_radix(hex, 16)
                } else {
                    v.parse::<isize>()
                };
                a.hwnd = Some(parsed.unwrap_or_else(|_| usage()));
            }
            "--bind" => a.bind = it.next().unwrap_or_else(|| usage()),
            "--port" => a.port = it.next().unwrap_or_else(|| usage()).parse().unwrap_or(7801),
            "--fps" => a.fps = it.next().unwrap_or_else(|| usage()).parse().unwrap_or(20),
            "--quality" => {
                a.quality = it.next().unwrap_or_else(|| usage()).parse().unwrap_or(70)
            }
            "-h" | "--help" => usage(),
            other => {
                eprintln!("unknown argument: {other}");
                usage();
            }
        }
    }
    a
}

fn main() {
    unsafe {
        ffi::SetProcessDPIAware();
    }

    let args = parse_args();

    if args.list {
        println!("{:<12} {:<8} {:<8} {}", "HWND", "PID", "SIZE", "TITLE");
        for w in win::list_windows() {
            println!(
                "{:<12} {:<8} {:<8} {}",
                format!("{:#x}", w.hwnd),
                w.pid,
                format!("{}x{}", w.w, w.h),
                w.title
            );
        }
        return;
    }

    let info = match win::find_window(args.hwnd, args.title.as_deref()) {
        Some(w) => w,
        None => {
            eprintln!("Window not found. Run `uvagent --list`.");
            std::process::exit(1);
        }
    };

    eprintln!(
        "Following window {:#x} \"{}\" ({}x{})",
        info.hwnd, info.title, info.w, info.h
    );

    let addr = format!("{}:{}", args.bind, args.port);
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Cannot bind {addr}: {e}");
            std::process::exit(1);
        }
    };

    eprintln!("Agent ready at {addr} (fps={}, quality={})", args.fps, args.quality);

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let hwnd = info.hwnd;
                let fps = args.fps;
                let quality = args.quality;
                thread::spawn(move || {
                    let peer = s.peer_addr().map(|a| a.to_string()).unwrap_or_default();
                    eprintln!("client connected: {peer}");
                    serve(s, hwnd, fps, quality);
                    eprintln!("client disconnected: {peer}");
                });
            }
            Err(e) => eprintln!("accept failed: {e}"),
        }
    }
}

fn serve(stream: TcpStream, hwnd: HWND, fps: u32, quality: u8) {
    let _ = stream.set_nodelay(true);

    let read_half = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };

    let alive = Arc::new(AtomicBool::new(true));
    // Live-tunable stream settings, shared with the reader thread.
    let quality = Arc::new(AtomicU8::new(quality.clamp(1, 95)));
    let fps = Arc::new(AtomicU32::new(fps.clamp(1, 60)));
    // Every write goes through one lock so frames and control events, written
    // from two threads, never interleave on the wire.
    let out = Arc::new(Mutex::new(stream));
    // Last clipboard text we have seen, to break the copy->send->set->send loop.
    let last_clip = Arc::new(Mutex::new(String::new()));

    // Reader thread: dispatches every inbound control message.
    {
        let alive = alive.clone();
        let quality = quality.clone();
        let fps = fps.clone();
        let out = out.clone();
        let last_clip = last_clip.clone();
        thread::spawn(move || {
            let mut r = BufReader::new(read_half);
            loop {
                match uvproto::read_msg(&mut r) {
                    Ok((kind, payload)) if kind == uvproto::KIND_CTL => {
                        if let Ok(ctl) = serde_json::from_slice::<Ctl>(&payload) {
                            match ctl.t.as_str() {
                                "quality" => {
                                    quality.store((ctl.v as u8).clamp(1, 95), Ordering::Relaxed)
                                }
                                "fps" => {
                                    fps.store((ctl.v as u32).clamp(1, 60), Ordering::Relaxed)
                                }
                                "ping" => {
                                    let pong = Event {
                                        t: "pong".into(),
                                        w: ctl.v as u32,
                                        ..Default::default()
                                    };
                                    let _ = send_event(&out, &pong);
                                }
                                "clip" => {
                                    if !ctl.s.is_empty() {
                                        if let Ok(mut g) = last_clip.lock() {
                                            *g = ctl.s.clone();
                                        }
                                        win::set_clipboard_text(&ctl.s);
                                    }
                                }
                                _ => apply_input(hwnd, &ctl),
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {
                        alive.store(false, Ordering::SeqCst);
                        break;
                    }
                }
            }
        });
    }

    let mut capture = match win::Capture::new(hwnd) {
        Some(c) => c,
        None => {
            eprintln!("cannot capture window {hwnd:#x} (closed?)");
            return;
        }
    };

    let hello = Event {
        t: "hello".into(),
        w: capture.w as u32,
        h: capture.h as u32,
        title: win::window_title(hwnd),
        ..Default::default()
    };
    if send_event(&out, &hello).is_err() {
        return;
    }

    let mut seq: u32 = 0;
    let mut last_hash: u64 = 0;
    let mut idle_rounds: u32 = 0;
    let mut last_sent = Instant::now();
    let mut last_clip_poll = Instant::now();

    while alive.load(Ordering::SeqCst) {
        let started = Instant::now();
        let fps_now = fps.load(Ordering::Relaxed).clamp(1, 60);
        let frame_interval = Duration::from_millis(1000 / fps_now as u64);

        if win::is_alive(hwnd) && capture.grab() {
            let h = win::hash(&capture.rgb);

            // Send on change, plus a heartbeat frame once a second so a client
            // that missed one still converges on the current screen.
            let heartbeat = last_sent.elapsed() >= Duration::from_millis(1000);
            if h != last_hash || heartbeat {
                last_hash = h;
                last_sent = Instant::now();
                idle_rounds = 0;

                let q = quality.load(Ordering::Relaxed);
                if let Some(jpeg) = encode_jpeg(&capture.rgb, capture.w, capture.h, q) {
                    seq = seq.wrapping_add(1);
                    let payload =
                        uvproto::encode_frame(capture.w as u16, capture.h as u16, seq, &jpeg);
                    let mut g = match out.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    if uvproto::write_msg(&mut *g, uvproto::KIND_FRAME, &payload).is_err() {
                        break;
                    }
                    drop(g);
                }
            } else {
                idle_rounds = idle_rounds.saturating_add(1);
            }
        }

        // Mirror the guest clipboard to the client when it changes.
        if last_clip_poll.elapsed() >= Duration::from_millis(500) {
            last_clip_poll = Instant::now();
            if let Some(text) = win::get_clipboard_text() {
                let changed = match last_clip.lock() {
                    Ok(g) => !text.is_empty() && *g != text,
                    Err(_) => false,
                };
                if changed {
                    if let Ok(mut g) = last_clip.lock() {
                        *g = text.clone();
                    }
                    let ev = Event {
                        t: "clip".into(),
                        s: text,
                        ..Default::default()
                    };
                    let _ = send_event(&out, &ev);
                }
            }
        }

        let elapsed = started.elapsed();
        let mut wait = frame_interval.saturating_sub(elapsed);

        // Nothing moved for a while: back off so an idle session does not burn
        // a full CPU core re-capturing the same pixels.
        if idle_rounds > 10 {
            wait = wait.max(Duration::from_millis(120));
        }

        if !wait.is_zero() {
            thread::sleep(wait);
        }
    }
}

fn send_event(out: &Arc<Mutex<TcpStream>>, event: &Event) -> std::io::Result<()> {
    let payload = serde_json::to_vec(event).map_err(std::io::Error::other)?;
    let mut g = out
        .lock()
        .map_err(|_| std::io::Error::other("write lock poisoned"))?;
    uvproto::write_msg(&mut *g, uvproto::KIND_CTL, &payload)
}

fn encode_jpeg(rgb: &[u8], w: i32, h: i32, quality: u8) -> Option<Vec<u8>> {
    if w <= 0 || h <= 0 || w > u16::MAX as i32 || h > u16::MAX as i32 {
        return None;
    }
    let mut out = Vec::with_capacity((w as usize) * (h as usize) / 8);
    let encoder = jpeg_encoder::Encoder::new(&mut out, quality.clamp(1, 95));
    encoder
        .encode(
            rgb,
            w as u16,
            h as u16,
            jpeg_encoder::ColorType::Rgb,
        )
        .ok()?;
    Some(out)
}

/// Replays one input control message (mouse / key / the login macro) onto the
/// target window. Non-input messages are handled by the reader loop directly.
fn apply_input(hwnd: HWND, ctl: &Ctl) {
    match ctl.t.as_str() {
        "mouse" => {
            win::ensure_foreground(hwnd);
            match ctl.k.as_str() {
                "move" => win::mouse_move(hwnd, ctl.x, ctl.y),
                "down" => win::mouse_button(hwnd, ctl.x, ctl.y, ctl.b, true),
                "up" => win::mouse_button(hwnd, ctl.x, ctl.y, ctl.b, false),
                "wheel" => win::mouse_wheel(ctl.d as i32),
                _ => {}
            }
        }
        "key" => {
            win::ensure_foreground(hwnd);
            match ctl.k.as_str() {
                "char" => win::type_text(&ctl.c),
                "vk" => win::key_vk(ctl.v, ctl.down),
                _ => {}
            }
        }
        "login" => {
            // Macro: focus UltraViewer, overwrite the ID field, tab to the
            // password field, type it, press Enter.
            win::focus(hwnd);
            thread::sleep(Duration::from_millis(250));
            win::press_combo(VK_CONTROL, VK_A);
            win::type_text(&ctl.id);
            thread::sleep(Duration::from_millis(100));
            win::key_vk(VK_TAB, true);
            win::key_vk(VK_TAB, false);
            thread::sleep(Duration::from_millis(150));
            win::press_combo(VK_CONTROL, VK_A);
            win::type_text(&ctl.pw);
            thread::sleep(Duration::from_millis(100));
            win::key_vk(VK_RETURN, true);
            win::key_vk(VK_RETURN, false);
        }
        _ => {}
    }
}
