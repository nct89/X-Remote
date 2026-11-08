# X-Remote

Native remote-desktop client for **Linux** and **macOS**. Connect to a host by ID and
password, stream the remote screen, and forward mouse and keyboard input.

**[Download the latest release](https://github.com/nct89/X-Remote/releases/latest)**

This repository only contains installation instructions. The application is distributed as
prebuilt packages in [Releases](https://github.com/nct89/X-Remote/releases).

## Packages in v0.1.0

| Platform | File | Architecture |
|---|---|---|
| Linux | `X-Remote-x86_64.AppImage` | x86_64 |
| Linux | `x-remote_0.1.0_amd64.deb` | amd64 |
| Linux | `x-remote-0.1.0-1.fc44.x86_64.rpm` | x86_64 |
| Linux | `x-remote-0.1.0-linux-x86_64.tar.gz` | x86_64 |
| macOS | `X-Remote-0.1.0-macos-universal.dmg` | universal (arm64 + x86_64) |

The commands below use the `0.1.0` file names; substitute the version you downloaded.

---

## Linux

### AppImage — any distribution, no root

```sh
chmod +x X-Remote-x86_64.AppImage
./X-Remote-x86_64.AppImage
```

Optional desktop integration: install [AppImageLauncher](https://github.com/TheAssassin/AppImageLauncher)
and open the file once, or extract it and copy the desktop entry manually:

```sh
./X-Remote-x86_64.AppImage --appimage-extract
```

### Debian / Ubuntu / Linux Mint / Pop!_OS

```sh
sudo apt install ./x-remote_0.1.0_amd64.deb
```

If you use `dpkg` directly, resolve dependencies afterwards:

```sh
sudo dpkg -i x-remote_0.1.0_amd64.deb
sudo apt -f install
```

### Fedora / RHEL / Rocky Linux / AlmaLinux

```sh
sudo dnf install ./x-remote-0.1.0-1.fc44.x86_64.rpm
```

### openSUSE / SLES

```sh
sudo zypper install --allow-unsigned-rpm ./x-remote-0.1.0-1.fc44.x86_64.rpm
```

### Portable tarball — any distribution

Installs into `~/.local` (no root):

```sh
tar xf x-remote-0.1.0-linux-x86_64.tar.gz
./x-remote-0.1.0-linux-x86_64/install.sh
```

Then run `~/.local/bin/x-remote` (add `~/.local/bin` to `PATH` if needed).

### Runtime dependencies

X11 and OpenGL are required:

- Debian/Ubuntu: `libx11-6 libxcursor1 libxi6 libxrandr2 libxext6 libgl1`
- Fedora/RHEL: `libX11 libXcursor libXi libXrandr libXext mesa-libGL`

The "be-controlled" mode opens a WebKit2GTK verification page. Install WebKit2GTK 4.1 if you
need that mode:

- Fedora: `sudo dnf install webkit2gtk4.1 python3-gobject`
- Debian/Ubuntu: `sudo apt install gir1.2-webkit2-4.1 python3-gi`

### Uninstall (Linux)

```sh
# .deb
sudo apt remove x-remote

# .rpm
sudo dnf remove x-remote

# portable install
rm -rf ~/.local/share/x-remote ~/.local/bin/x-remote \
       ~/.local/share/applications/x-remote.desktop
```

---

## macOS

Requires macOS 11 (Big Sur) or newer. The bundle is universal, so it runs natively on both
Apple Silicon and Intel Macs.

1. Open `X-Remote-0.1.0-macos-universal.dmg`.
2. Drag **X-Remote** onto the **Applications** folder.
3. Eject the disk image.

### First launch (Gatekeeper)

The app is not notarized by Apple, so macOS may refuse the first launch. Do one of these:

- In Finder, **right-click X-Remote → Open**, then confirm **Open**.
- Or remove the quarantine flag:

```sh
xattr -cr /Applications/X-Remote.app
open /Applications/X-Remote.app
```

The first launch needs an internet connection: the app downloads its service key once and
keeps it locally for later launches.

### Uninstall (macOS)

```sh
rm -rf /Applications/X-Remote.app
```

---

## Verify your download

```sh
# Linux
sha256sum <file>

# macOS
shasum -a 256 <file>
```

| File | SHA-256 |
|---|---|
| `X-Remote-x86_64.AppImage` | `2bb89a49375ee1b58680d31f25bd5cf2919e6bd6e5563a61986aa2b4d45e36b5` |
| `x-remote_0.1.0_amd64.deb` | `1d20d122726b431d5a9d14be06c826244a3ae1c30d0f6641bd634fa8418e9947` |
| `x-remote-0.1.0-1.fc44.x86_64.rpm` | `f6576a3f8e27a5d96d18b2c83e6185d7ba52e71b270b42603dd01d32220e317d` |
| `x-remote-0.1.0-linux-x86_64.tar.gz` | `84c357293c81aa7bce7741e710ac9aff8bcfac0e15f71122ab2bbd3c9f45d2cf` |
| `X-Remote-0.1.0-macos-universal.dmg` | `1f86067b4580e0ba070b57f458f5d91e7587ec75037e97a8d55e570a14a2c463` |

GitHub also prints each file's SHA-256 on the release page.

## Privacy

To register and connect, X-Remote sends the remote-desktop service a machine identifier
derived from the disk model/serial/firmware, a MAC-shaped hardware id, the host name, the
user name and the local IP address. When hosting it asks the operating system for screen
capture and input control. The app keeps its host ID, connection history and a short log in
`~/.config/x-remote/` (directory `0700`, files `0600`). The first launch downloads a service
key over HTTPS with normal certificate validation and caches it there for six hours.

## License

MIT
