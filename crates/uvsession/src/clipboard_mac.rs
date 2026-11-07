//! Host-side clipboard on macOS: same interface as the X11 version (`clipboard.rs`), sets
//! the content with `pbcopy` (always present on macOS). `pbcopy` exits only after writing,
//! so once `set` returns the next Cmd+V/Ctrl+V pastes the new text.

use std::io::Write;
use std::process::{Command, Stdio};

/// Clipboard owner (macOS needs no selection-holding thread like X11).
pub struct ClipboardOwner;

impl ClipboardOwner {
    pub fn new() -> Result<ClipboardOwner, String> {
        Ok(ClipboardOwner)
    }

    /// Set the clipboard. Returns `true` once `pbcopy` finishes writing.
    pub fn set(&self, text: &str) -> bool {
        let Ok(mut child) = Command::new("pbcopy")
            // pbcopy honors LANG; force UTF-8 so accented text is not mangled.
            .env("LANG", "en_US.UTF-8")
            .stdin(Stdio::piped())
            .spawn()
        else {
            return false;
        };
        let wrote = child.stdin.take().map(|mut s| s.write_all(text.as_bytes()).is_ok()).unwrap_or(false);
        child.wait().map(|t| t.success()).unwrap_or(false) && wrote
    }
}
