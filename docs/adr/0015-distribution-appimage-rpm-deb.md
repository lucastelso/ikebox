# ADR-0015: Distribution as AppImage, rpm and deb (no Flatpak)

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

The user should not need to install anything besides the app: no Docker, no
Node/npm, no Rust.

Node, npm and Rust are **build** tools. Tauri compiles the frontend
(HTML/JS/CSS) into the binary, so the user never sees npm.

The app needs a privileged component on the host (ADR-0014).

## Decision

- Formats, built by the Tauri bundler in an Ubuntu 22.04 environment (for
  glibc compatibility):
  - **AppImage**: download, make it executable and open it. It carries
    WebKitGTK. On first use, it installs the network component with the
    administrator password, once;
  - **rpm** (Fedora/openSUSE) and **deb** (Debian/Ubuntu): they carry the
    component bundle inside the app.
- Releases on GitHub, with `SHA256SUMS`. A signed automatic update stays
  optional (Tauri updater, which supports the AppImage).
- **No Flatpak.**

## Consequences

- A single download works on most distributions with glibc ≥ 2.35.
- The AppImage is large, because it carries WebKitGTK (about 80–100 MB), and
  its bundled libraries do not get the distribution's security updates.

## Alternatives considered

- **Flatpak.** The sandbox prevents netlink/XFRM and privileged services on
  the host. Working around it needs `flatpak-spawn --host`, which voids the
  sandbox, and Flathub refuses such apps. It is the same reason the
  NetworkManager VPN plugins do not exist as Flatpaks.
- **Snap.** The same confinement problem and less adoption among the target
  audience (Fedora).
- **The frontend in a container.** It solves nothing (the frontend is already
  compiled into the binary) and would bring Docker back.
