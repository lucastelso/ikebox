# Installation guide

**[Português](pt-BR/instalacao.md)** · English

How to install IPsec Client as a regular application: it shows up in the
applications menu, with an icon, and opens like any other program. You do
**not** need Docker, Rust, Node or a terminal to use it.

- [1. Which file to download](#1-which-file-to-download)
- [2. Install](#2-install)
- [3. First use](#3-first-use)
- [4. Update](#4-update)
- [5. Uninstall](#5-uninstall)
- [6. Build the package from source](#6-build-the-package-from-source)
- [Common problems](#common-problems)

---

## 1. Which file to download

| Your distribution | File | Example |
|---|---|---|
| Fedora, openSUSE, RHEL/Rocky/AlmaLinux 10 | **.rpm** | `ipsec-client-0.8.0-1.x86_64.rpm` |
| Ubuntu, Debian, Linux Mint, Pop!_OS | **.deb** | `ipsec-client_0.8.0_amd64.deb` |
| Others (Arch, etc.) | **.AppImage** | `ipsec-client_0.8.0_amd64.AppImage` |

The files are on the repository's
[Releases](https://github.com/lucastelso/ipsec-client/releases) page on GitHub,
together with `SHA256SUMS`, which lets you check the download
(`sha256sum -c SHA256SUMS --ignore-missing`). While the repository is
private, only people with access to it can download: ask for access, or get
the files from whoever gave you the profile.

> Prefer the **.rpm** or the **.deb** when there is one for your distribution:
> the app shows up in the menu on its own, updates go through the package
> manager, and its libraries (such as WebKitGTK) get your distribution's
> security updates. The AppImage carries its own copies.

### Tested distributions

The packages are installed and the VPN engine is tested, automatically, on:

| Distribution | Package | VPN DNS (internal names) |
|---|---|---|
| Ubuntu 22.04 and 24.04 | `.deb` | works out of the box |
| Debian 12 and 13 | `.deb` | needs `systemd-resolved` (see below) |
| Fedora 45 | `.rpm` | works out of the box |
| openSUSE Tumbleweed | `.rpm` | needs `systemd-resolved` (see below) |
| AlmaLinux 10 (also Rocky Linux and RHEL 10) | `.rpm`, with EPEL | check **System → Diagnostics** |

Requirements on any distribution: **systemd** (248 or newer), **polkit**,
glibc 2.35 or newer, WebKitGTK 4.1 and 64-bit x86. Without systemd the app
does not work. **RHEL, Rocky and AlmaLinux 9 are not supported**: they have
neither WebKitGTK 4.1 nor a recent enough glibc, so neither the `.rpm` nor
the AppImage runs there.

**VPN DNS.** Internal names of the VPN (`server.company.example`) are
resolved through **systemd-resolved**. Ubuntu and Fedora use it by default.
Debian and openSUSE do not: there the tunnel works, by IP address, and
**System → Diagnostics** warns about it. To have the names, install and
enable it (this changes who resolves names on the whole system, not only for
the VPN):

```bash
sudo apt install systemd-resolved                        # Debian
sudo zypper install systemd-resolved && sudo systemctl enable --now systemd-resolved   # openSUSE
sudo dnf install systemd-resolved && sudo systemctl enable --now systemd-resolved      # AlmaLinux, Rocky, RHEL
```

## 2. Install

### Fedora and similar (.rpm)

**Double-click** the file: the software store opens (GNOME Software or
Discover). Click **Install**. If it does not open, use the terminal.

Or from the terminal, in the folder where the file is:

```bash
sudo dnf install ./ipsec-client-*.x86_64.rpm
```

On **openSUSE**: `sudo zypper install --allow-unsigned-rpm ./ipsec-client-*.x86_64.rpm`
(the package is not signed yet; check the download with `SHA256SUMS`).

On **AlmaLinux and Rocky Linux 10**, WebKitGTK and the tray library come from
EPEL. Enable it once, then install:

```bash
sudo dnf install epel-release
sudo dnf config-manager --set-enabled crb
sudo dnf install ./ipsec-client-*.x86_64.rpm
```

On RHEL 10 itself, enable EPEL and CodeReady Builder as the EPEL
documentation says for RHEL, then install the same way.

On **Fedora Silverblue, Kinoite and other image-based systems**:
`rpm-ostree install ./ipsec-client-*.x86_64.rpm`, then restart. The network
component handles what is different there (a read-only `/usr`, `/opt` as a
link to `/var/opt`); this was tested in a simulation, not yet on a real
installation, so please report any problem.

### Ubuntu, Debian and similar (.deb)

On many systems, **double-clicking** the file opens the installer. If it does
not (the store in some Ubuntu versions does not install `.deb` files), use the
terminal, in the folder where the file is:

```bash
sudo apt install ./ipsec-client_*_amd64.deb
```

### Other distributions (.AppImage)

The AppImage is a single file: it installs nothing in the system.

1. Create a folder for it, move the file there and make it executable:

   ```bash
   mkdir -p ~/Applications
   mv ~/Downloads/ipsec-client_*_amd64.AppImage ~/Applications/ipsec-client.AppImage
   chmod +x ~/Applications/ipsec-client.AppImage
   ```

2. **To show it in the menu**, there are two ways:
   - **Easiest:** install [Gear Lever](https://flathub.org/apps/it.mijorus.gearlever)
     (software store or Flathub), open the AppImage with it and use its option
     to add it to the applications menu.
   - **By hand:**

     ```bash
     cd ~/Applications
     ./ipsec-client.AppImage --appimage-extract 'usr/share/icons/hicolor/128x128/apps/ipsec-client.png'
     install -Dm644 squashfs-root/usr/share/icons/hicolor/128x128/apps/ipsec-client.png \
       ~/.local/share/icons/hicolor/128x128/apps/ipsec-client.png
     rm -rf squashfs-root
     cat > ~/.local/share/applications/ipsec-client.desktop <<EOF
     [Desktop Entry]
     Type=Application
     Name=IPsec Client
     Comment=IPsec VPN client for Linux
     Exec=$HOME/Applications/ipsec-client.AppImage
     Icon=ipsec-client
     Terminal=false
     Categories=Network;
     StartupWMClass=ipsec-client
     EOF
     ```

> The AppImage needs the FUSE 2 library. If it does not open, install it:
> Fedora: `sudo dnf install fuse-libs`; Ubuntu 22.04: `sudo apt install libfuse2`;
> Ubuntu 24.04 or newer: `sudo apt install libfuse2t64`.

## 3. First use

1. Open **IPsec Client** from the applications menu (search for "VPN" or
   "IPsec"). It speaks the language of your system (English or Portuguese);
   you can change it in **System → Appearance → Language**.
2. **Install the network component (only once).** The main screen shows
   *"One step left: install the network component"*. Click **Install now** and
   type the **administrator password** when the system asks for it.
   - This is the part that creates the VPN tunnel in the system. After it,
     connecting **no longer asks for a password**, for any user of this
     computer.
   - The same button is in **System → Network component**.
   - You need an administrator account (one that uses `sudo`) only for this
     step.
3. **Import the VPN profile:** go to **Profiles → Import** and choose the
   `.json` file you received, or **drag the file** into the window. Check the
   preview and click **Import**.
4. **Connect:** in **Connection**, choose the profile and click **Connect**.
   Enter the **group PSK**, your **username** and your **password**. Check
   **Remember in the system keyring** so you do not have to type them again.
5. When **Connected** shows up, the VPN is on. Open Remmina, your browser or
   whatever you normally use.

While the app is open, the **tray** icon (near the clock) shows the state and
has Disconnect. **Closing the window closes the app, but does not disconnect
the VPN.** When you open it again, it shows the active connection. To
disconnect, use the **Disconnect** button.

On GNOME, the tray only shows up with the **AppIndicator** extension: on
Fedora, `sudo dnf install gnome-shell-extension-appindicator`; then log out,
log back in and enable the extension in the **Extensions** app.

## 4. Update

1. Install the new file over the old one, the same way as in section 2.
   - Fedora: `sudo dnf install ./ipsec-client-<new version>.x86_64.rpm`
   - Ubuntu: `sudo apt install ./ipsec-client_<new version>_amd64.deb`
   - AppImage: replace the file in `~/Applications` (keep the name
     `ipsec-client.AppImage`).
2. Open the app. If **"Network component update"** shows up, click **Update
   now** and type the administrator password. Some updates (like 0.2.0) only
   take effect after this step.

Your profiles and credentials stay where they were.

## 5. Uninstall

1. **In the app, with the VPN disconnected:** **System → Network component →
   Remove** (asks for the administrator password).
2. Remove the app:
   - Fedora: `sudo dnf remove ipsec-client`
   - Ubuntu: `sudo apt remove ipsec-client`
   - AppImage: delete the file and, if you created it by hand, the shortcut:
     `rm ~/.local/share/applications/ipsec-client.desktop`
3. **Optional, to leave no trace:**
   - profiles: `rm -r ~/.config/ipsec-client`;
   - stored credentials: before removing the app, delete each profile in
     **Profiles** checking **Also delete the stored credentials**.

If you removed the app before the component, remove the component from the
terminal:

```bash
sudo /opt/ipsec-client/libexec/install-native.sh uninstall
```

## 6. Build the package from source

For people who have the source code and want to build the `.rpm`/`.deb` to
test a change. Release packages are built another way: see
[how to build and publish a release](release.md).

1. Install the development prerequisites from the
   [README](../README.md#prerequisites), once. Docker is only used to compile
   the embedded strongSwan, the first time.
2. In the repository folder:

   ```bash
   npm ci
   npm run bundle -- --bundles rpm    # Fedora; on Ubuntu/Debian: --bundles deb
   ```

   It takes a few minutes the first time. The package ends up in
   `target/release/bundle/rpm/` (or `.../deb/`).
3. Install it as in section 2:

   ```bash
   sudo dnf install ./target/release/bundle/rpm/ipsec-client-*.x86_64.rpm
   ```

   Built it again **with the same version**? Use `sudo dnf reinstall` instead
   of `install`.

> Build the `.deb` on Ubuntu or Debian: the package declares the libraries of
> the distribution it was built on (the tray library has different names on
> different distributions). A package built on your own computer contains
> paths from your home directory: do not distribute it.

## Common problems

| Symptom | What to do |
|---|---|
| The app does not show up in the menu | Wait a few seconds or log out and back in. For the AppImage, check the shortcut in `~/.local/share/applications/` |
| The tray icon does not show up (GNOME) | Install and enable the AppIndicator extension (section 3) |
| Internal names of the VPN do not resolve, but IP addresses work | `systemd-resolved` is not active (Debian, openSUSE): see "VPN DNS" in section 1 |
| "pkexec was not found" | Install `pkexec`: the `pkexec` package on Debian, Ubuntu and openSUSE, `polkit` on Fedora. The `.rpm` and the `.deb` bring it as a dependency |
| "Authorization cancelled" | The password window was closed. Click install again |
| "Could not get administrator authorization" | Your account is not an administrator, or the password was wrong. Ask someone with `sudo` to install the component once: after that, any user can connect |
| "This computer is not ready to connect yet" | Click **Install network component** in the alert itself, or see **System → Diagnostics** |
| "The installed network component is from an older version" | Click **Update network component** in the alert, or go to **System → Network component → Update** |
| "UDP ports 500/4500 in use" | Another IPsec VPN is on; turn it off first |
| "Wrong XAuth username or password" | Click **Enter credentials** in the alert and type them again |
| "The gateway rejected phase 1 authentication" | The group PSK is wrong: **Enter credentials** and type it again |
| "The gateway did not answer" | Check your internet connection. Some networks (hotels, mobile) block VPNs |

**System → Diagnostics** shows what is missing, with a hint on how to fix it.
If you need help, send what shows up there and the connection log copied with
**Copy anonymized**: it has no password or PSK, and the gateway, IPs and
domains become placeholders.
