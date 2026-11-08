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
| `X-Remote-x86_64.AppImage` | `4bd309c195d94aa6a16e2aa9515c84353ed582de8173317db7c136e53047ad31` |
| `x-remote_0.1.0_amd64.deb` | `9847ad4617f42d0b0f473993855e4deb006b867d861aa5d7fc02ae27f6b2e1bb` |
| `x-remote-0.1.0-1.fc44.x86_64.rpm` | `e14af384e9cf234d694284566e87b92ce9790b7448f34e7b57c2209a061e8d42` |
| `x-remote-0.1.0-linux-x86_64.tar.gz` | `e612ce5e63ac0817912da3cf884d70d7e3935a6e8d0a6b900a36e6ab7f415f56` |
| `X-Remote-0.1.0-macos-universal.dmg` | `c15f9a665f4b560c36e723358bf0b55abc3369b798e90f366c31a8924610c169` |

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
