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
| Linux | `x-remote_0.1.0_amd64.deb` |
| Linux | `x-remote-0.1.0-1.fc44.x86_64.rpm` |
| Linux | `x-remote-0.1.0-linux-x86_64.tar.gz` |

## Cài đặt

### macOS (macOS 11 Big Sur trở lên)

1. Tải file `X-Remote-0.1.0-macos-universal.dmg`.
2. Mở file và kéo **X-Remote** vào thư mục **Applications**.
3. Lần đầu mở: nhấp chuột phải vào ứng dụng → chọn **Open** → chọn **Open** lần nữa để xác nhận.

### Linux

#### AppImage — mọi bản phân phối, không cần quyền root

```sh
chmod +x X-Remote-x86_64.AppImage
./X-Remote-x86_64.AppImage
```

#### Debian / Ubuntu / Linux Mint / Pop!_OS

```sh
sudo apt install ./x-remote_0.1.0_amd64.deb
```

#### Fedora / RHEL / Rocky Linux / AlmaLinux

```sh
sudo dnf install ./x-remote-0.1.0-1.fc44.x86_64.rpm
```

#### Tarball di động — cài vào `~/.local`, không cần quyền root

```sh
tar xf x-remote-0.1.0-linux-x86_64.tar.gz
./x-remote-0.1.0-linux-x86_64/install.sh
```

Sau đó chạy lệnh `x-remote` (thêm `~/.local/bin` vào `PATH` nếu cần).

## Sử dụng

1. Trên máy Windows, mở UltraViewer và ghi lại **ID của bạn** cùng **mật khẩu**.
2. Mở X-Remote, nhập ID và mật khẩu đó vào khung kết nối.
3. Nhấn **Bắt đầu điều khiển**.

Để cho máy khác điều khiển máy của bạn, chia sẻ ID và mật khẩu hiển thị ở khung **Cho phép điều khiển**.

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

# bản di động (portable)
rm -rf ~/.local/share/x-remote ~/.local/bin/x-remote \
       ~/.local/share/applications/x-remote.desktop
```

## Giấy phép

MIT
