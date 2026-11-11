// Small window that opens the official UltraViewer captcha page.
// The page calls window.external.FromJS_CaptchaCorrect(_, token); the second
// argument is printed to stdout and the process exits. The user types the image text.
import Cocoa
import WebKit

final class CaptchaApp: NSObject, NSApplicationDelegate, WKScriptMessageHandler, WKNavigationDelegate, WKUIDelegate {
    var window: NSWindow!
    var web: WKWebView!

    func applicationDidFinishLaunching(_ notification: Notification) {
        let args = CommandLine.arguments
        guard args.count >= 2, let url = URL(string: args[1]), url.scheme == "https" else {
            fputs("usage: captcha_mac <https-url>\n", stderr)
            NSApp.terminate(nil)
            return
        }

        let uc = WKUserContentController()
        let js = """
        (function () {
          var post = function (parts) {
            try {
              window.webkit.messageHandlers.xremote.postMessage(parts.join("\\u001f"));
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
        })();
        """
        uc.addUserScript(WKUserScript(source: js, injectionTime: .atDocumentStart, forMainFrameOnly: false))
        uc.add(self, name: "xremote")

        let cfg = WKWebViewConfiguration()
        cfg.userContentController = uc

        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 520, height: 720),
            styleMask: [.titled, .closable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        window.title = "Confirm UltraViewer"
        window.level = .floating
        window.center()

        web = WKWebView(frame: window.contentView!.bounds, configuration: cfg)
        web.autoresizingMask = [.width, .height]
        web.navigationDelegate = self
        web.uiDelegate = self
        window.contentView?.addSubview(web)
        window.makeKeyAndOrderFront(nil)
        if #available(macOS 14.0, *) {
            NSApp.activate()
        } else {
            NSApp.activate(ignoringOtherApps: true)
        }
        web.load(URLRequest(url: url))
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    func webView(
        _ webView: WKWebView,
        createWebViewWith configuration: WKWebViewConfiguration,
        for navigationAction: WKNavigationAction,
        windowFeatures: WKWindowFeatures
    ) -> WKWebView? {
        if let url = navigationAction.request.url {
            webView.load(URLRequest(url: url))
        }
        return nil
    }

    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let text = message.body as? String else { return }
        let parts = text.split(separator: "\u{1f}", omittingEmptySubsequences: false).map(String.init)
        guard parts.first == "FromJS_CaptchaCorrect" else {
            if let name = parts.first {
                fputs("captcha called \(name)\n", stderr)
            }
            return
        }
        let tok: String? = {
            if parts.count >= 3, let s = validToken(parts[2]) { return s }
            if parts.count >= 2, let s = validToken(parts[1]) { return s }
            return nil
        }()
        guard let tok else {
            fputs("captcha returned an unusable token\n", stderr)
            return
        }
        print(tok)
        fflush(stdout)
        NSApp.terminate(nil)
    }

    func validToken(_ t: String) -> String? {
        let s = t.trimmingCharacters(in: .whitespacesAndNewlines)
        if s.isEmpty || s.utf8.count > 4096 { return nil }
        for b in s.utf8 {
            if b < 0x20 || b > 0x7e || b == 0x3a { return nil }
        }
        return s
    }
}

let app = NSApplication.shared
let delegate = CaptchaApp()
app.setActivationPolicy(.regular)
app.delegate = delegate
app.run()
