//! uvmock — a fake agent for testing the client without a Windows guest.
//!
//! It serves a synthetic animated 640x400 screen over the real wire protocol and
//! answers every control message:
//!   - ping   -> pong (so the client's latency readout works)
//!   - quality/fps -> adjusts the stream live
//!   - clip   -> logs it, and 2s after connect pushes a clip event back, so the
//!               client's clipboard bridge can be exercised both ways
//!   - mouse/key/login -> logged
//!
//! Usage: uvmock [--bind 127.0.0.1] [--port 7801]

use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use uvproto::{Ctl, Event};

const W: u16 = 640;
const H: u16 = 400;

fn main() {
    let mut bind = "127.0.0.1".to_string();
    let mut port: u16 = 7801;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--bind" => bind = it.next().unwrap_or(bind),
            "--port" => port = it.next().and_then(|v| v.parse().ok()).unwrap_or(port),
            _ => {}
        }
    }

    let addr = format!("{bind}:{port}");
    let listener = TcpListener::bind(&addr).unwrap_or_else(|e| {
        eprintln!("cannot bind {addr}: {e}");
        std::process::exit(1);
    });
    eprintln!("uvmock ready at {addr} (fake screen {W}x{H})");

    for stream in listener.incoming().flatten() {
        thread::spawn(move || serve(stream));
    }
}

fn serve(stream: TcpStream) {
    let _ = stream.set_nodelay(true);
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    eprintln!("[{peer}] client connected");

    let read_half = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let write = Arc::new(Mutex::new(stream));
    let quality = Arc::new(AtomicU8::new(70));
    let fps = Arc::new(AtomicU32::new(10));

    send_event(&write, &Event { t: "hello".into(), w: W as u32, h: H as u32, title: "Mock Desktop".into(), ..Default::default() });

    // Reader: apply control messages and answer them.
    {
        let write = write.clone();
        let quality = quality.clone();
        let fps = fps.clone();
        let peer = peer.clone();
        thread::spawn(move || {
            let mut r = BufReader::new(read_half);
            loop {
                match uvproto::read_msg(&mut r) {
                    Ok((k, payload)) if k == uvproto::KIND_CTL => {
                        if let Ok(c) = serde_json::from_slice::<Ctl>(&payload) {
                            match c.t.as_str() {
                                "ping" => {
                                    eprintln!("[{peer}] ping {} -> pong", c.v);
                                    send_event(&write, &Event { t: "pong".into(), w: c.v as u32, ..Default::default() });
                                }
                                "quality" => {
                                    quality.store((c.v as u8).clamp(1, 95), Ordering::Relaxed);
                                    eprintln!("[{peer}] quality -> {}", c.v);
                                }
                                "fps" => {
                                    fps.store((c.v as u32).clamp(1, 60), Ordering::Relaxed);
                                    eprintln!("[{peer}] fps -> {}", c.v);
                                }
                                "clip" => eprintln!("[{peer}] clip in: {:?}", c.s),
                                "mouse" => eprintln!("[{peer}] mouse {} @ {:.3},{:.3} b{}", c.k, c.x, c.y, c.b),
                                "key" => eprintln!("[{peer}] key {} c={:?} v={} down={}", c.k, c.c, c.v, c.down),
                                "login" => eprintln!("[{peer}] login id={:?}", c.id),
                                other => eprintln!("[{peer}] unknown ctl: {other}"),
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {
                        eprintln!("[{peer}] connection closed");
                        break;
                    }
                }
            }
        });
    }

    // One inbound clipboard push, 2s in, to test the client mirroring it.
    {
        let write = write.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(2));
            eprintln!("[{peer}] sending clip -> client");
            send_event(&write, &Event { t: "clip".into(), s: "hello from the client".into(), ..Default::default() });
        });
    }

    // Capture loop: an animated synthetic frame.
    let start = Instant::now();
    let mut seq: u32 = 0;
    loop {
        let t = start.elapsed().as_millis() as u32 / 33;
        let rgb = render(t);
        let q = quality.load(Ordering::Relaxed);
        if let Some(jpeg) = encode(&rgb, q) {
            seq = seq.wrapping_add(1);
            let payload = uvproto::encode_frame(W, H, seq, &jpeg);
            let mut g = match write.lock() {
                Ok(g) => g,
                Err(_) => break,
            };
            if uvproto::write_msg(&mut *g, uvproto::KIND_FRAME, &payload).is_err() {
                break;
            }
            drop(g);
        }
        let hz = fps.load(Ordering::Relaxed).clamp(1, 60) as u64;
        thread::sleep(Duration::from_millis(1000 / hz));
    }
}

/// A moving gradient with a bright square, so motion and color are both visible.
fn render(t: u32) -> Vec<u8> {
    let (w, h) = (W as usize, H as usize);
    let mut buf = vec![0u8; w * h * 3];
    let bx = ((t * 3) as usize) % (w - 60);
    let by = (h / 2) - 30;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 3;
            let in_box = x >= bx && x < bx + 60 && y >= by && y < by + 60;
            if in_box {
                buf[i] = 0xFF;
                buf[i + 1] = 0xFF;
                buf[i + 2] = 0xFF;
            } else {
                buf[i] = ((x + t as usize) & 0xFF) as u8;
                buf[i + 1] = ((y + t as usize / 2) & 0xFF) as u8;
                buf[i + 2] = 0x50;
            }
        }
    }
    buf
}

fn encode(rgb: &[u8], quality: u8) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let enc = jpeg_encoder::Encoder::new(&mut out, quality);
    enc.encode(rgb, W, H, jpeg_encoder::ColorType::Rgb).ok()?;
    Some(out)
}

fn send_event(write: &Arc<Mutex<TcpStream>>, ev: &Event) {
    if let Ok(payload) = serde_json::to_vec(ev) {
        if let Ok(mut g) = write.lock() {
            let _ = uvproto::write_msg(&mut *g, uvproto::KIND_CTL, &payload);
        }
    }
}
