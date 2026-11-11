# X-Remote

X-Remote là ứng dụng giúp bạn **kết nối và điều khiển máy tính Windows đang chạy UltraViewer** một cách dễ dàng ngay từ **macOS** và **Linux**.

Chỉ cần nhập **ID** và **mật khẩu** hiển thị trên máy Windows là bạn có thể xem màn hình và thao tác từ xa ngay lập tức.

## Tính năng

- Kết nối nhanh bằng ID và mật khẩu UltraViewer
- Xem màn hình từ xa mượt mà, hỗ trợ nhiều màn hình
- Điều khiển chuột và bàn phím từ xa
- Trò chuyện (chat) trong phiên làm việc
- Tùy chỉnh chất lượng hình ảnh: tốc độ / tự động / nét
- Cho phép máy khác điều khiển ngược lại máy của bạn (chế độ bị điều khiển)

## Tải xuống

Tải bản mới nhất tại [trang Releases](https://github.com/nct89/X-Remote/releases/latest).

| Nền tảng | Tệp |
|---|---|
| macOS | `X-Remote-0.1.0-macos-universal.dmg` |
| Linux | `X-Remote-x86_64.AppImage` |
| Linux | `x-remote_0.1.4_amd64.deb` |
| Linux | `x-remote-0.1.4-1.x86_64.rpm` |
| Linux | `x-remote-0.1.4-linux-x86_64.tar.gz` |
| Linux | `X-Remote.flatpak` |

Gói Linux chạy trên glibc 2.27 trở lên: Ubuntu 22.04+, Debian 11+, Linux Mint, Fedora, RHEL/Rocky/AlmaLinux 9, openSUSE Leap 15.6 và các bản tương đương.

## Cài đặt

### macOS (macOS 11 Big Sur trở lên)

1. Tải file `X-Remote-0.1.0-macos-universal.dmg`.
2. Mở file và kéo **X-Remote** vào thư mục **Applications**.
3. Lần đầu mở: nhấp chuột phải vào ứng dụng → chọn **Open** → chọn **Open** lần nữa để xác nhận.

### Linux

#### AppImage — mọi bản phân phối có glibc 2.27 trở lên, không cần quyền root

```sh
chmod +x X-Remote-x86_64.AppImage
./X-Remote-x86_64.AppImage
```

#### Debian / Ubuntu 22.04+ / Linux Mint / Pop!_OS

```sh
sudo apt install ./x-remote_0.1.4_amd64.deb
```

#### Fedora / RHEL / Rocky Linux / AlmaLinux

```sh
sudo dnf install ./x-remote-0.1.4-1.x86_64.rpm
```

#### openSUSE

```sh
sudo zypper install --allow-unsigned-rpm ./x-remote-0.1.4-1.x86_64.rpm
```

#### Flatpak — mọi bản phân phối có Flatpak

```sh
flatpak install --user ./X-Remote.flatpak
flatpak run net.xremote.app
```

#### Tarball di động — cài vào `~/.local`, không cần quyền root

```sh
tar xf x-remote-0.1.4-linux-x86_64.tar.gz
./x-remote-0.1.4-linux-x86_64/install.sh
```

Sau đó chạy lệnh `x-remote` (thêm `~/.local/bin` vào `PATH` nếu cần).

## Sử dụng

1. Trên máy Windows, mở UltraViewer và ghi lại **ID của bạn** cùng **mật khẩu**.
2. Mở X-Remote, nhập ID và mật khẩu đó vào khung kết nối.
3. Nhấn **Bắt đầu điều khiển**.

Để cho máy khác điều khiển máy của bạn, chia sẻ ID và mật khẩu hiển thị ở khung **Cho phép điều khiển**.

> **Wayland:** chế độ bị điều khiển trên Linux cần phiên **X11 (Xorg)**. Trên phiên Wayland, máy điều khiển sẽ chỉ thấy màn hình đen và X-Remote sẽ báo điều này. Điều khiển máy khác vẫn dùng được trên Wayland.

## Gỡ cài đặt

### macOS

```sh
rm -rf /Applications/X-Remote.app
```

### Linux

```sh
# .deb
sudo apt remove x-remote

# .rpm
sudo dnf remove x-remote

# Flatpak
flatpak uninstall --user net.xremote.app

# bản di động (portable)
rm -rf ~/.local/share/x-remote ~/.local/bin/x-remote \
       ~/.local/share/applications/x-remote.desktop
```

## Build từ mã nguồn (for developers)

A small cross-platform remote-desktop client written in Rust (egui UI). It connects to a
peer over a rendezvous service, streams the remote screen, and forwards mouse/keyboard input.
Supported hosts: Linux (X11), macOS, Windows.

### Build

```
cargo build --release -p uvclient
```

Run:

```
cargo run -p uvclient
```

### Linux packages (all distros)

From a Linux x86_64 machine:

```
sh scripts/build-linux.sh
```

Output in `dist/`:

| File | Distros |
|---|---|
| `X-Remote-x86_64.AppImage` | Any (chmod +x, then run) |
| `x-remote_*_amd64.deb` | Debian, Ubuntu, Mint, Pop!_OS |
| `x-remote-*.x86_64.rpm` | Fedora, RHEL, Rocky, openSUSE |
| `x-remote-*-linux-x86_64.tar.gz` | Any (unpack, `./install.sh` into `~/.local`) |

```
chmod +x dist/X-Remote-x86_64.AppImage && ./dist/X-Remote-x86_64.AppImage
sudo dpkg -i dist/x-remote_*_amd64.deb
sudo dnf install dist/x-remote-*.x86_64.rpm
tar xf dist/x-remote-*-linux-x86_64.tar.gz && ./x-remote-*/install.sh
```

From a source tree without packaging:

```
cargo build --release -p uvclient
sh scripts/install-linux.sh
```

`build-linux.sh` compiles inside `rust:1.88-bullseye`; the binary needs only glibc 2.27 with podman or docker, so
the same binary runs on Ubuntu 22.04+, Debian 11+, Mint, Fedora, RHEL/Rocky 9 and openSUSE.
`LOCAL_BUILD=1` uses the host toolchain instead (the result then needs the host's glibc or newer).
`BUNDLE_DICT=1` adds `packaging/8bit.dat` to the packages — only if you may redistribute it.

#### Flatpak

```
sh scripts/build-flatpak.sh
flatpak install --user dist/X-Remote.flatpak && flatpak run net.xremote.app
```

Needs `flatpak-builder` (or `flatpak install --user flathub org.flatpak.Builder`). Uses the
GNOME 51 runtime from Flathub; crates are vendored so the build runs offline. Screenshots go to
`~/Pictures/X-Remote`. For a Flathub submission, swap the local source archive for
`flatpak-cargo-generator` output and a git source.

### Notes

- **Wayland** — the UI runs through XWayland when `DISPLAY` is set (set `XREMOTE_WAYLAND=1` to
  force native Wayland). Being controlled needs an **X11 (Xorg) session**; under Wayland the
  controller would only see a black screen, and the app says so.
- **Screen-frame dictionary** — some servers compress screen frames with an external zstd
  dictionary. It is **not bundled**. To enable it, point `XREMOTE_SCREEN_DICT` at a dictionary
  file. Without it, peer-to-peer sessions work using plain zstd.
- **uvagent** is a Windows-only helper (Win32 FFI) and is excluded from the default build.
  On Windows, build it with `cargo build -p uvagent`.
- **Linux "be-controlled" mode** opens a small verification page in an embedded WebKit2GTK
  window. Install the runtime first, e.g. on Fedora: `sudo dnf install webkit2gtk4.1`
  (Debian/Ubuntu: `gir1.2-webkit2-4.1 python3-gi`).

## Giấy phép

MIT — xem [LICENSE](LICENSE).
