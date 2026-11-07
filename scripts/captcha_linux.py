#!/usr/bin/env python3
# Small window that opens the official UltraViewer captcha page on Linux (WebKit2GTK).
# Same idea as captcha_mac.swift: the page calls window.external.FromJS_CaptchaCorrect(_, token);
# the token is printed on stdout (first line) and the process exits. The user types the image text.
# Needs: python3-gobject (gi) + webkit2gtk4.1 (typelib WebKit2-4.1). Run: python3 captcha_linux.py <https-url>
import sys

import gi

gi.require_version("Gtk", "3.0")
try:
    gi.require_version("WebKit2", "4.1")
except ValueError:
    gi.require_version("WebKit2", "4.0")
from gi.repository import GLib, Gtk, WebKit2  # noqa: E402

# Injected at document-start: replace window.external with a Proxy that forwards
# every call (e.g. FromJS_CaptchaCorrect) to the 'xremote' message handler.
# Matches captcha_mac.swift.
INJECT = r"""(function () {
  var post = function (parts) {
    try {
      window.webkit.messageHandlers.xremote.postMessage(parts.join("\u001f"));
    } catch (e) {}
  };
  var external = new Proxy({}, {
    get: function (_, name) {
      if (typeof name !== "string") { return undefined; }
      return function () {
        var parts = [name];
        for (var i = 0; i < arguments.length; i++) {
          parts.push(arguments[i] == null ? "" : String(arguments[i]));
        }
        post(parts);
      };
    },
    has: function () { return true; }
  });
  try {
    Object.defineProperty(window, "external", { value: external, configurable: true });
  } catch (e) {
    window.external = external;
  }
})();"""


def valid_token(t):
    """Valid token: printable ASCII, non-empty, no ':' (protocol separator)."""
    s = (t or "").strip()
    if not s:
        return None
    raw = s.encode("utf-8", "ignore")
    if len(raw) > 4096:
        return None
    for b in raw:
        if b < 0x20 or b > 0x7E or b == 0x3A:
            return None
    return s


def main():
    if len(sys.argv) < 2 or not sys.argv[1].startswith("https://"):
        sys.stderr.write("usage: captcha_linux.py <https-url>\n")
        return 2
    url = sys.argv[1]

    ucm = WebKit2.UserContentManager()
    ucm.add_script(
        WebKit2.UserScript.new(
            INJECT,
            WebKit2.UserContentInjectedFrames.ALL_FRAMES,
            WebKit2.UserScriptInjectionTime.START,
            None,
            None,
        )
    )
    ucm.register_script_message_handler("xremote")

    def doc_message(value):
        """String from a JSCValue (4.1) or WebKitJavascriptResult (4.0)."""
        try:
            return value.to_string()
        except Exception:
            pass
        try:
            return value.get_js_value().to_string()
        except Exception:
            return None

    def on_msg(_mgr, value):
        text = doc_message(value)
        if text is None:
            return
        parts = text.split("\u001f")
        if not parts or parts[0] != "FromJS_CaptchaCorrect":
            return
        tok = valid_token(parts[2]) if len(parts) >= 3 else None
        if tok is None and len(parts) >= 2:
            tok = valid_token(parts[1])
        if tok is None:
            sys.stderr.write("captcha returned an unusable token\n")
            return
        sys.stdout.write(tok + "\n")
        sys.stdout.flush()
        Gtk.main_quit()

    ucm.connect("script-message-received::xremote", on_msg)

    web = WebKit2.WebView.new_with_user_content_manager(ucm)
    win = Gtk.Window(title="Confirm UltraViewer")
    win.set_default_size(520, 720)
    win.set_keep_above(True)
    win.connect("destroy", Gtk.main_quit)
    win.add(web)
    web.load_uri(url)
    win.show_all()
    win.present()

    # Close after 10 minutes (exit 0 with no token; the caller waits and retries).
    GLib.timeout_add_seconds(600, Gtk.main_quit)
    Gtk.main()
    return 0


if __name__ == "__main__":
    sys.exit(main())
