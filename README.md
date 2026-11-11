# X-Remote

X-Remote giúp bạn **điều khiển máy tính Windows đang chạy UltraViewer** ngay từ **Linux** hoặc **macOS**, và cho phép người khác điều khiển lại máy của bạn.

Chỉ cần nhập **ID** và **mật khẩu** hiển thị trên máy cần điều khiển là bạn xem được màn hình và dùng chuột, bàn phím từ xa.

## Tính năng

- Kết nối bằng ID và mật khẩu UltraViewer, không cần cài gì thêm lên máy Windows
- Xem màn hình từ xa, chọn từng màn hình khi máy kia có nhiều màn hình
- Điều khiển chuột, bàn phím, gửi Ctrl+Alt+Del và các phím tắt hệ thống
- Chat, gửi tệp, ảnh và đường dẫn thư mục trong phiên làm việc
- Chọn chất lượng hình: Thấp (nhẹ mạng), Vừa, Cao (nét)
- Chụp ảnh màn hình máy đang điều khiển
- Cho phép người khác điều khiển máy của bạn, trên cả X11 và Wayland (GNOME, KDE Plasma)

## Chọn gói cài

Tải gói tại [trang Releases](https://github.com/nct89/X-Remote/releases/latest). Chọn **một** gói theo hệ điều hành của bạn:

| Hệ điều hành | Tải tệp |
|---|---|
| Ubuntu 22.04 trở lên, Debian 12 trở lên, Linux Mint 21+, Pop!_OS, Zorin | `x-remote_0.1.5_amd64.deb` |
| Fedora, RHEL / Rocky Linux / AlmaLinux 9 trở lên | `x-remote-0.1.5-1.x86_64.rpm` |
| openSUSE Leap 15.6, Tumbleweed | `x-remote-0.1.5-1.x86_64.rpm` |
| Bản Linux bất kỳ có Flatpak | `X-Remote.flatpak` |
| Bản Linux bất kỳ, không muốn cài vào hệ thống | `X-Remote-x86_64.AppImage` |
| Bản Linux bất kỳ, cài vào thư mục người dùng | `x-remote-0.1.5-linux-x86_64.tar.gz` |
| macOS 11 trở lên (Intel và Apple Silicon) | `X-Remote-0.1.0-macos-universal.dmg` |

**Nên chọn `.deb`, `.rpm` hoặc Flatpak:** các gói này tự cài luôn những thành phần đi kèm (thư viện đồ hoạ, cửa sổ xác nhận captcha, thành phần chia sẻ màn hình trên Wayland). AppImage và tar.gz có thể cần cài thêm, xem [Cửa sổ xác nhận (captcha) không mở](#cửa-sổ-xác-nhận-captcha-không-mở).

Gói Linux đã được cài thử trên Ubuntu 22.04, Ubuntu 24.04, Rocky Linux 9, openSUSE Leap 15.6 và Fedora 44. Máy cần Linux 64-bit (x86_64). Bản Linux cũ hơn danh sách trên thì dùng Flatpak.

## Cài đặt

Mở **Terminal** trong thư mục chứa tệp vừa tải (thường là `~/Downloads`; chuột phải trong thư mục → *Open in Terminal*), rồi chạy lệnh tương ứng.

### Ubuntu / Debian / Linux Mint / Pop!_OS

```sh
sudo apt install ./x-remote_0.1.5_amd64.deb
```

Hoặc nhấp đúp tệp `.deb` để cài bằng trình quản lý phần mềm.

### Fedora / RHEL / Rocky Linux / AlmaLinux

```sh
sudo dnf install ./x-remote-0.1.5-1.x86_64.rpm
```

### openSUSE

```sh
sudo zypper install --allow-unsigned-rpm ./x-remote-0.1.5-1.x86_64.rpm
```

Gói chưa được ký số nên cần thêm `--allow-unsigned-rpm`.

### Flatpak

```sh
flatpak install --user ./X-Remote.flatpak
flatpak run net.xremote.app
```

Máy chưa có Flatpak thì cài theo hướng dẫn tại [flatpak.org/setup](https://flatpak.org/setup/). Lần cài đầu, Flatpak tải thêm bộ thư viện GNOME (vài trăm MB).

### AppImage (không cần quyền root)

```sh
chmod +x X-Remote-x86_64.AppImage
./X-Remote-x86_64.AppImage
```

### Tarball (cài vào `~/.local`, không cần quyền root)

```sh
tar xf x-remote-0.1.5-linux-x86_64.tar.gz
./x-remote-0.1.5-linux-x86_64/install.sh
```

Sau khi cài, mở **X-Remote** từ menu ứng dụng hoặc chạy lệnh `x-remote`. Nếu báo không tìm thấy lệnh, thêm `~/.local/bin` vào `PATH`.

### macOS

1. Mở tệp `X-Remote-0.1.0-macos-universal.dmg`, kéo **X-Remote** vào thư mục **Applications**.
2. Lần đầu mở: chuột phải vào ứng dụng → **Open** → **Open** lần nữa để xác nhận.
3. Để người khác điều khiển được máy Mac, vào **Cài đặt hệ thống → Quyền riêng tư & Bảo mật**, bật **Ghi màn hình** và **Trợ năng** cho X-Remote.

## Sử dụng

### Lần đầu mở ứng dụng

X-Remote tự xin một **ID của bạn** từ máy chủ UltraViewer. Đôi khi máy chủ yêu cầu xác nhận: một cửa sổ nhỏ hiện ảnh chữ, bạn gõ đúng chữ trong ảnh rồi xác nhận. Việc này chỉ cần làm một lần; các lần sau ID được giữ nguyên.

### Điều khiển một máy khác

1. Trên máy cần điều khiển, mở UltraViewer (hoặc X-Remote) và đọc **ID** cùng **mật khẩu**.
2. Trong X-Remote, ở khung **Điều khiển máy tính khác**, nhập **ID đối tác** và **Mật khẩu**.
3. Bấm **Bắt đầu điều khiển**.

Thanh công cụ ở mép trên màn hình phiên:

| Nút | Dùng để |
|---|---|
| **Thao tác** | Gửi Ctrl+Alt+Del, các phím tắt hệ thống, **Ngắt kết nối** |
| **Cấu hình** | Chất lượng hình, chọn màn hình, ẩn hình nền máy đích, **Toàn màn hình** |
| **Chụp hình** | Lưu ảnh màn hình máy đích vào `~/Pictures/X-Remote`, mở thư mục ảnh |
| **Chat** | Mở khung chat: nhắn tin, gửi tệp, ảnh, đường dẫn thư mục |
| **⌃** | Thu gọn thanh công cụ |

### Cho người khác điều khiển máy của bạn

Gửi cho người hỗ trợ **ID của bạn** và **Mật khẩu** trong khung **Cho phép điều khiển**. Xong việc, bấm nút làm mới cạnh mật khẩu để đổi mật khẩu. Muốn ngắt phiên ngay, bấm **×** trên dòng tên máy trong khung chat.

**Trên Linux dùng Wayland** (mặc định trên Fedora, Ubuntu mới, KDE Plasma 6): lần đầu có người kết nối, hệ thống hiện hộp thoại chia sẻ màn hình. Chọn màn hình, bật **cho phép điều khiển** rồi bấm **Chia sẻ**. X-Remote nhớ lựa chọn này nên các lần sau không hỏi lại.

### Gõ tiếng Việt khi điều khiển

X-Remote gửi phím thật sang máy kia. Hãy gõ tiếng Việt bằng bộ gõ (Unikey, EVKey…) **trên máy được điều khiển**. Nếu máy của bạn dùng fcitx5, X-Remote tự kết nối để bạn vẫn gõ tiếng Việt được trong khung chat và ô nhập ID.

## Khắc phục sự cố

### Cửa sổ đứng hình, nền đen, báo "X-Remote is not responding"

Lỗi xảy ra ở bản 0.1.3 trở về trước, khi bạn **điều khiển máy khác** từ Linux chạy Wayland (KDE Plasma, GNOME), nhất là lúc bật toàn màn hình. **Đã sửa từ bản 0.1.4** — hãy cài bản mới nhất. Nếu vẫn gặp, hãy báo lại kèm tên bản Linux và môi trường desktop đang dùng.

### Người điều khiển máy tôi chỉ thấy màn hình đen

Trường hợp **người khác điều khiển máy Linux của bạn**:

- **Đang dùng bản 0.1.4 trở về trước trên Wayland:** các bản này chưa hỗ trợ Wayland. Hãy cài bản 0.1.5 trở lên.
- **Đã bấm Huỷ ở hộp thoại chia sẻ màn hình:** ngắt kết nối, cho người kia kết nối lại rồi bấm **Chia sẻ**.
- **Hộp thoại chia sẻ màn hình không hiện:** máy thiếu thành phần chia sẻ màn hình. Cài thêm rồi đăng xuất, đăng nhập lại:

```sh
# Ubuntu / Debian / Mint
sudo apt install xdg-desktop-portal pipewire
# Fedora / RHEL / Rocky
sudo dnf install xdg-desktop-portal pipewire
# openSUSE
sudo zypper install xdg-desktop-portal pipewire
```

  Trên GNOME cần thêm gói `xdg-desktop-portal-gnome`, trên KDE là `xdg-desktop-portal-kde` (thường đã có sẵn).
- Kiểm tra máy đang chạy phiên nào: mở Terminal, gõ `echo $XDG_SESSION_TYPE` (kết quả là `x11` hoặc `wayland`).

Việc **bạn điều khiển máy khác** không bị ảnh hưởng bởi các mục trên.

### Cửa sổ xác nhận (captcha) không mở

Gói `.deb`, `.rpm` và Flatpak tự cài sẵn thành phần này. Với AppImage hoặc tar.gz, cài thêm:

```sh
# Ubuntu / Debian / Mint
sudo apt install gir1.2-webkit2-4.1 python3-gi
# Fedora / RHEL / Rocky
sudo dnf install webkit2gtk4.1 python3-gobject
# openSUSE
sudo zypper install typelib-1_0-WebKit2-4_1 python3-gobject-Gdk
```

### Báo lỗi `GLIBC_2.xx not found` hoặc không cài được gói

Bản Linux đang dùng cũ hơn danh sách đã hỗ trợ. Hãy dùng gói **Flatpak**, hoặc nâng cấp hệ điều hành.

### Không kết nối được hoặc báo sai mật khẩu

- Kiểm tra lại ID và mật khẩu (mật khẩu UltraViewer có thể đổi mỗi lần mở lại).
- Máy kia phải đang mở UltraViewer/X-Remote và có Internet.
- Mạng công ty hoặc tường lửa có thể chặn kết nối; thử mạng khác.

### Nhật ký gửi kèm khi báo lỗi

`~/.config/x-remote/host.log` (bản Flatpak: `~/.var/app/net.xremote.app/config/x-remote/host.log`). Tệp này không chứa mật khẩu.

## Gỡ cài đặt

```sh
# .deb
sudo apt remove x-remote
# .rpm (Fedora / RHEL)
sudo dnf remove x-remote
# .rpm (openSUSE)
sudo zypper remove x-remote
# Flatpak
flatpak uninstall --user net.xremote.app
# tarball
rm -rf ~/.local/share/x-remote ~/.local/bin/x-remote \
       ~/.local/share/applications/x-remote.desktop
```

AppImage: chỉ cần xoá tệp `.AppImage`. macOS: kéo **X-Remote** trong **Applications** vào Thùng rác.

ID và cấu hình nằm trong `~/.config/x-remote`; xoá thư mục này nếu muốn xoá hẳn.

## Giấy phép

MIT
