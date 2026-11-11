//! Session channel: raw TCP, frame `[u32 length][IV16][AES-256-CBC]`.
//!
//! After the peer accepts, every frame and keyboard/mouse event goes through
//! here. The read buffer keeps the leftover between calls, and frames that
//! arrive together are queued — otherwise the second frame in one read is lost.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::crypto::{aes_decrypt, aes_encrypt};

/// TLS channel to the P2P relay (the relay speaks only HTTPS 443).
pub type RelayTls = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

pub struct Session<S: Read + Write = TcpStream> {
    sock: S,
    password: String,
    /// Raw bytes read but not yet split into frames.
    buf: Vec<u8>,
    /// Decrypted frames waiting for the caller.
    pending: VecDeque<Vec<u8>>,
    /// Status of the latest read, for diagnostics.
    pub last_read: &'static str,
}

impl<S: Read + Write> Session<S> {
    pub fn wrap(sock: S, password: &str) -> Session<S> {
        Session {
            sock,
            password: password.to_string(),
            buf: Vec::new(),
            pending: VecDeque::new(),
            last_read: "unread",
        }
    }

    pub fn send(&mut self, plain: &[u8]) -> std::io::Result<()> {
        let ct = aes_encrypt(&self.password, plain);
        // Header and body in one write, like the Python port: the peer reads
        // frames, so it must not see a header standing alone.
        let mut frame = Vec::with_capacity(4 + ct.len());
        frame.extend_from_slice(&(ct.len() as u32).to_le_bytes());
        frame.extend_from_slice(&ct);
        self.sock.write_all(&frame)?;
        self.sock.flush()
    }

    /// Move every complete frame in `buf` onto the queue.
    fn split_buffer(&mut self) {
        loop {
            if self.buf.len() < 4 {
                break;
            }
            let n = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
            if n == 0 || n > 12_000_000 || self.buf.len() < 4 + n {
                break;
            }
            let payload: Vec<u8> = self.buf[4..4 + n].to_vec();
            self.buf.drain(..4 + n);
            self.pending.push_back(aes_decrypt(&self.password, &payload));
        }
    }

    /// Read once more from the socket into `buf`. `false` when there is no more data.
    fn read_once(&mut self) -> std::io::Result<bool> {
        let mut chunk = vec![0u8; 262_144];
        match self.sock.read(&mut chunk) {
            Ok(0) => {
                // The peer closed. Do not return `Ok(false)` the way a timeout does:
                // the host treats a timeout as "no command yet" and keeps the session,
                // so the chat panel and "Connecting" line never clear when the
                // controller exits without sending `19`.
                self.last_read = "eof";
                Err(std::io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "connection closed",
                ))
            }
            Ok(n) => {
                self.last_read = "data";
                self.buf.extend_from_slice(&chunk[..n]);
                Ok(true)
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                self.last_read = "timeout";
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    /// Wait for one frame. `None` on timeout or a closed connection.
    pub fn recv(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(f) = self.pending.pop_front() {
                return Ok(Some(f));
            }
            if !self.read_once()? {
                return Ok(None);
            }
            self.split_buffer();
        }
    }

    /// Wait for one frame, then take every frame already available — avoids a queue of stale pictures.
    pub fn recv_batch(&mut self) -> std::io::Result<Vec<Vec<u8>>> {
        let first = match self.recv()? {
            Some(f) => f,
            None => return Ok(Vec::new()),
        };
        let mut frames = vec![first];
        // Drain whatever is already in the socket, without blocking.
        while self.read_once()? {}
        self.split_buffer();
        while let Some(f) = self.pending.pop_front() {
            frames.push(f);
        }
        Ok(frames)
    }
}

impl Session<TcpStream> {
    pub fn new(sock: TcpStream, password: &str) -> std::io::Result<Session<TcpStream>> {
        sock.set_nodelay(true)?;
        Ok(Session::wrap(sock, password))
    }

    pub fn set_timeout(&self, t: Duration) -> std::io::Result<()> {
        self.sock.set_read_timeout(Some(t))
    }

    pub fn set_write_timeout(&self, t: Duration) -> std::io::Result<()> {
        self.sock.set_write_timeout(Some(t))
    }
}

impl Session<RelayTls> {
    pub fn set_timeout(&self, t: Duration) -> std::io::Result<()> {
        self.sock.sock.set_read_timeout(Some(t))
    }

    pub fn set_write_timeout(&self, t: Duration) -> std::io::Result<()> {
        self.sock.sock.set_write_timeout(Some(t))
    }
}

/// Host session channel: raw TCP (an X-Remote controller) or TLS via the relay
/// (a real UltraViewer). Both speak the same AES frame protocol.
pub enum Chan {
    Tcp(Session<TcpStream>),
    Tls(Session<RelayTls>),
}

impl Chan {
    pub fn send(&mut self, plain: &[u8]) -> std::io::Result<()> {
        match self {
            Chan::Tcp(s) => s.send(plain),
            Chan::Tls(s) => s.send(plain),
        }
    }

    pub fn recv(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        match self {
            Chan::Tcp(s) => s.recv(),
            Chan::Tls(s) => s.recv(),
        }
    }

    pub fn recv_batch(&mut self) -> std::io::Result<Vec<Vec<u8>>> {
        match self {
            Chan::Tcp(s) => s.recv_batch(),
            Chan::Tls(s) => s.recv_batch(),
        }
    }

    pub fn set_timeout(&self, t: Duration) -> std::io::Result<()> {
        match self {
            Chan::Tcp(s) => s.set_timeout(t),
            Chan::Tls(s) => s.set_timeout(t),
        }
    }

    pub fn set_write_timeout(&self, t: Duration) -> std::io::Result<()> {
        match self {
            Chan::Tcp(s) => s.set_write_timeout(t),
            Chan::Tls(s) => s.set_write_timeout(t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use std::net::TcpListener;

    fn cap() -> (Session, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = l.accept().unwrap();
        (Session::new(client, "abc123").unwrap(), server)
    }

    /// Read one raw frame on the receiving side and check its contents.
    fn read_raw(peer: &mut TcpStream) -> Vec<u8> {
        let mut hdr = [0u8; 4];
        peer.read_exact(&mut hdr).unwrap();
        let n = u32::from_le_bytes(hdr) as usize;
        let mut body = vec![0u8; n];
        peer.read_exact(&mut body).unwrap();
        aes_decrypt("abc123", &body)
    }

    #[test]
    fn send_and_read_back() {
        let (mut tx, mut rx) = cap();
        tx.send(b"xin chao").unwrap();
        assert_eq!(read_raw(&mut rx), b"xin chao");
    }

    #[test]
    fn consecutive_frames() {
        let (mut tx, mut rx) = cap();
        for i in 0..5u8 {
            tx.send(&[i; 40]).unwrap();
        }
        for i in 0..5u8 {
            assert_eq!(read_raw(&mut rx), vec![i; 40]);
        }
    }

    #[test]
    fn no_loss_when_frames_arrive_together() {
        // Three frames sent back-to-back, then one read: the old code dropped the second and third.
        let (mut tx, mut rx) = cap();
        tx.send(b"mot").unwrap();
        tx.send(b"hai").unwrap();
        tx.send(b"ba").unwrap();
        std::thread::sleep(Duration::from_millis(150));

        let mut s = Session::new(rx, "abc123").unwrap();
        s.set_timeout(Duration::from_millis(300)).unwrap();
        let batch = s.recv_batch().unwrap();
        assert_eq!(batch.len(), 3, "expected 3 frames, got {}", batch.len());
        assert_eq!(batch[0], b"mot");
        assert_eq!(batch[1], b"hai");
        assert_eq!(batch[2], b"ba");

        // The next read is empty, and no frame was lost.
        assert!(s.recv_batch().unwrap().is_empty());
    }

    #[test]
    fn later_frames_still_arrive() {
        let (mut tx, mut rx) = cap();
        tx.send(b"a").unwrap();
        std::thread::sleep(Duration::from_millis(80));
        tx.send(b"b").unwrap();
        std::thread::sleep(Duration::from_millis(80));
        let mut s = Session::new(rx, "abc123").unwrap();
        s.set_timeout(Duration::from_millis(300)).unwrap();
        assert_eq!(s.recv().unwrap().unwrap(), b"a");
        assert_eq!(s.recv().unwrap().unwrap(), b"b");
    }

    #[test]
    fn peer_close_is_eof_not_timeout() {
        let (mut s, peer) = cap();
        s.set_timeout(Duration::from_millis(300)).unwrap();
        drop(peer);
        let e = s.recv().unwrap_err();
        assert_eq!(e.kind(), ErrorKind::UnexpectedEof);
        assert_eq!(s.last_read, "eof");
    }
}
