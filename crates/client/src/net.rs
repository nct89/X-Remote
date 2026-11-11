//! Transport: connects to an engine endpoint and feeds decoded frames to the UI.

use eframe::egui;
use std::io::BufReader;
use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use uvproto::{Ctl, Event};

/// One frame, already decoded to 8-bit RGB.
///
/// `Arc` so the network thread and the UI share one buffer: a 1080p frame is
/// almost 6 MB, so copying every frame would be waste.
pub struct Pic {
    pub w: u16,
    pub h: u16,
    pub rgb: Arc<Vec<u8>>,
    /// Bytes actually received from the remote for this frame (for the speed readout).
    pub byte: usize,
}

pub enum Inbound {
    Frame(Pic),
    Event(Event),
    /// Remote monitor layout (UltraViewer sessions only, when there are two or more).
    Monitors(Vec<uvsession::Monitor>),
    /// Remote command 49: `true` = control allowed, `false` = view only.
    Control(bool),
    /// Chat from the remote (UltraViewer session, this machine is the controller).
    Chat(uvsession::chat::HostEvent),
    /// Remote machine name (part 15 of the init message).
    HostName(String),
    /// Remote OS: `windows` / `linux` / `macos` (`XR-OS:` tag in part 16).
    Os(String),
    Closed(String),
}

pub struct Conn {
    tx: TcpStream,
    rx: Receiver<Inbound>,
}

impl Conn {
    pub fn connect(addr: &str, ctx: &egui::Context) -> std::io::Result<Conn> {
        let stream = TcpStream::connect(addr)?;
        let _ = stream.set_nodelay(true);
        let read_half = stream.try_clone()?;
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        thread::spawn(move || reader(read_half, tx, ctx));
        Ok(Conn { tx: stream, rx })
    }

    pub fn send(&mut self, ctl: &Ctl) {
        if let Ok(payload) = serde_json::to_vec(ctl) {
            let _ = uvproto::write_msg(&mut self.tx, uvproto::KIND_CTL, &payload);
        }
    }

    /// Drain everything the reader thread produced since the last call.
    pub fn poll(&self) -> (Vec<Inbound>, Option<String>) {
        let mut items = Vec::new();
        let mut closed = None;
        loop {
            match self.rx.try_recv() {
                Ok(item) => {
                    if let Inbound::Closed(why) = &item {
                        closed = Some(why.clone());
                        break;
                    }
                    items.push(item);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    closed = Some("receive channel closed".to_string());
                    break;
                }
            }
        }
        (items, closed)
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        let _ = self.tx.shutdown(std::net::Shutdown::Both);
    }
}

fn reader(stream: TcpStream, tx: Sender<Inbound>, ctx: egui::Context) {
    let mut r = BufReader::new(stream);
    loop {
        match uvproto::read_msg(&mut r) {
            Ok((kind, payload)) => {
                if kind == uvproto::KIND_FRAME {
                    if let Some((w, h, _seq, jpeg)) = uvproto::decode_frame(&payload) {
                        if let Some(rgb) = decode_jpeg(jpeg) {
                            let byte = rgb.len();
                            if tx
                                .send(Inbound::Frame(Pic { w, h, rgb: Arc::new(rgb), byte }))
                                .is_err()
                            {
                                return;
                            }
                            ctx.request_repaint();
                        }
                    }
                } else if kind == uvproto::KIND_CTL {
                    if let Ok(ev) = serde_json::from_slice::<Event>(&payload) {
                        if tx.send(Inbound::Event(ev)).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(Inbound::Closed(e.to_string()));
                ctx.request_repaint();
                return;
            }
        }
    }
}

fn decode_jpeg(jpeg: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(jpeg));
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => Some(pixels),
        jpeg_decoder::PixelFormat::L8 => {
            let mut rgb = Vec::with_capacity(pixels.len() * 3);
            for v in pixels {
                rgb.extend_from_slice(&[v, v, v]);
            }
            Some(rgb)
        }
        _ => None,
    }
}
