# IPsec Client

**[Português](README.pt-BR.md)** · English

IPsec VPN client for **Linux**, built for **legacy** VPNs: IKEv1, aggressive
mode, group PSK + XAuth, old DH groups. It is the kind of corporate VPN that
today's Linux clients can no longer connect to.

> **Status: v0.9.0 (pre-alpha).** The desktop app, installed from the `.rpm`
> and with its own embedded strongSwan (**no Docker**), connects end to end to
> a real FortiGate VPN, and every IPsec mode its profiles accept (IKEv1 main
> or aggressive, PSK with or without XAuth, IKEv2 with PSK, with a username
> and password, or with your own certificate, the gateway's certificate
> always checked) is tested on the same path. Profiles can be imported from
> other clients (Cisco `.pcf`, vpnc, NetworkManager, `ipsec.conf`,
> FortiClient) without reading their passwords. Profiles can be created and edited in the app, from vendor
> templates, and each one shows its security level. The interface speaks
> English and Portuguese. Coming from 0.2.0 or older,
> update the app **and** the network component (see the [CHANGELOG](CHANGELOG.md)).
> The packages (AppImage, rpm and deb) are on the
> [Releases](https://github.com/lucastelso/ipsec-client/releases) page. What
> exists and what comes next is in the [roadmap](docs/roadmap.md); the steps
> taken so far are in the [development log](docs/development-log.md).

---

## Why this project exists

Many companies and public agencies still run VPN concentrators (FortiGate,
for example) configured with IKEv1 and old algorithms. On Windows and macOS,
the vendor ships an official client, such as FortiClient. On Linux, support
was dropped, and the native tools do not help:

- current distributions, with their crypto policies, refuse or no longer
  offer those algorithms;
- NetworkManager does not handle the **group PSK + XAuth in aggressive mode**
  combination well.

IPsec Client solves this by shipping **its own strongSwan** (6.1.0, compiled
from the official source code). It keeps the legacy algorithms without
weakening anything in the system. For the user, it is an app with profiles,
safely stored credentials, diagnostics and a tray icon.

## Who it is for

- **Linux users** who need to reach a corporate IPsec VPN that has no
  official Linux client.
- **Teams** that want to hand a ready-made profile to colleagues, with the
  gateway, proposals, routes and DNS. Each person only enters their own
  credentials.
- **Automation:** there is a command line (`ipsec-client-cli`) with the same
  logic as the app.

### Who it is not for

| Case | Use |
|---|---|
| FortiGate SSL VPN (SSL-VPN) | [openfortivpn](https://github.com/adrienverge/openfortivpn) |
| L2TP/IPsec, OpenVPN, WireGuard | Out of scope: there are good native clients |
| Windows / macOS | The vendor's official client |

## Installation

Download the file for your distribution from the
[Releases](https://github.com/lucastelso/ipsec-client/releases) page:

| Your distribution | File | How to install |
|---|---|---|
| Fedora, openSUSE, RHEL 10 (with EPEL) | `.rpm` | `sudo dnf install ./ipsec-client-*.x86_64.rpm` |
| Ubuntu, Debian, Mint | `.deb` | `sudo apt install ./ipsec-client_*_amd64.deb` |
| Others | `.AppImage` | `chmod +x` and open it |

Then open **IPsec Client** from the applications menu. The first time, it asks
for the administrator password **once** to install the network component.

The full walkthrough (first use, updating, uninstalling and building the
packages from source) is in the **[installation guide](docs/installation.md)**.

> Got a profile from a colleague? Follow the **[quick start](docs/quick-start.md)**.

## Languages

The app follows the system language: Portuguese on a Portuguese system,
English on any other. You can change it in **System → Appearance →
Language**. The command line speaks English. The user documentation exists in
both languages (Portuguese versions in [docs/pt-BR](docs/pt-BR/)); the
technical documentation is in English.

## How it works

```
┌──────────────────────────────── Linux host ─────────────────────────────────┐
│  IPsec Client (app)  or  ipsec-client-cli                (your user)        │
│     │ profiles: ~/.config/ipsec-client   credentials: system keyring        │
│     │ "start the engine" (socket + polkit)   │ VICI (socket 0600, yours)    │
│     ▼                                        │                              │
│  ipsec-client-helper (root, minimal API)     │                              │
│     │ systemd-run, sandboxed                 ▼                              │
│     └──► charon (embedded strongSwan 6.1.0) ── updown → routes + DNS        │
│              │ XFRM SAs and policies (with if_id)                           │
│              ▼                                                              │
│  kernel: ipsc0 interface (XFRM) ═══════ ESP/UDP 4500 ═══════► VPN gateway   │
└─────────────────────────────────────────────────────────────────────────────┘
```

- **Connecting:**
  - the app asks the **network component** (the helper) to start strongSwan;
  - the helper checks who asked (polkit) and starts strongSwan as a temporary,
    isolated systemd service with network permissions only;
  - the app hands the profile and the credentials to strongSwan in memory,
    through the control socket (VICI), and starts the negotiation.
- **Connected:**
  - the virtual IP, the routes and the VPN DNS live on the dedicated `ipsc0`
    interface;
  - your Wi-Fi/Ethernet interface is not touched;
  - other VPNs (WireGuard, Tailscale) are not affected.
- **Disconnecting:** the strongSwan service stops, and the `ipsc0` interface
  goes away along with the routes, the IP and the DNS.

The details are in [docs/architecture.md](docs/architecture.md). The reason
behind each choice is in the [ADRs](docs/adr/README.md).

## Security and privacy

- **Credentials** (PSK, XAuth username and password):
  - never go to disk, environment variables or logs;
  - the network component, which runs as root, **never** receives them: they
    go from the app straight to strongSwan;
  - "remember credentials" uses the system keyring (GNOME Keyring/KWallet).
- **Network component:**
  - installed once, with the administrator password; the installer checks
    the hashes of every file;
  - receives from the app only validated numbers, the profile name and the
    DNS settings, never paths or commands; the command strongSwan runs as root
    is fixed by root, not by the app (ADR-0019);
  - connecting without a password is only allowed for the user of the active
    local session (polkit); only that user can stop the VPN or read its log.
- **strongSwan:** runs isolated by systemd (read-only system, no access to
  devices or kernel settings, network permissions only) and only listens on
  UDP ports 500/4500 **while the VPN is connected**.
- **Logs:** "Copy anonymized" replaces the gateway, IPs and domains with
  placeholders before the log leaves the app.
- **Repository:** contains no data from any real VPN. Profiles with your
  organization's data stay in `presets.local/`, which git ignores, or are
  created and imported by the app itself.
- **Known risks:** legacy VPNs use weak cryptography by nature. This and the
  other risks are in [SECURITY.md](SECURITY.md) and in ADRs
  [0014](docs/adr/0014-privileged-helper-polkit.md),
  [0018](docs/adr/0018-component-installed-by-the-app.md) and
  [0019](docs/adr/0019-forced-updown-and-if-id.md).

## Requirements

- Linux x86_64 with kernel ≥ 4.19 and the `xfrm_interface` module (the default
  in current distributions), glibc ≥ 2.35 and WebKitGTK 4.1.
- systemd ≥ 248 and polkit, the default in desktop distributions.
- `systemd-resolved` running, for the VPN DNS. Without it, the tunnel works by
  IP (Debian and openSUSE do not use it by default).
- Tested automatically on Ubuntu 22.04 and 24.04, Debian 12 and 13, Fedora 45,
  openSUSE Tumbleweed and AlmaLinux 10 ([installation guide](docs/installation.md#tested-distributions)).
  RHEL 9 and its rebuilds are not supported.
- An administrator account, only to install the network component once.
- Optional: Secret Service (GNOME Keyring/KWallet), to remember credentials;
  on GNOME, the AppIndicator extension, for the tray icon.

## Command line

`ipsec-client-cli` does the same as the app, from the terminal. The `.rpm` and
the `.deb` install it, with a manual page (`man ipsec-client-cli`) and shell
completions; with the AppImage, build it from source (see the
[CLI guide](docs/cli.md)):

```bash
ipsec-client-cli doctor             # is this computer ready?
ipsec-client-cli profiles import my-vpn.json
ipsec-client-cli secrets set my-vpn
ipsec-client-cli connect my-vpn     # Ctrl+C disconnects
```

A profile is a JSON file without credentials: see
[docs/profiles.md](docs/profiles.md), which includes the mapping from a
`swanctl.conf`. The full CLI guide is in [docs/cli.md](docs/cli.md).

## Development

### Prerequisites

Fedora:

```bash
sudo dnf install -y rustup webkit2gtk4.1-devel openssl-devel curl wget file \
  libappindicator-gtk3-devel librsvg2-devel libxdo-devel dbus-devel shellcheck
sudo dnf group install -y c-development
rustup-init -y --component rustfmt,clippy
```

Debian/Ubuntu:

```bash
sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev shellcheck
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

You also need Node.js ≥ 22, cargo-deny (`cargo install --locked cargo-deny`)
and Docker. Docker is only used to compile the embedded strongSwan and for the
tests with a local IPsec gateway; end users do not need it.

### Commands

```bash
npm install                 # tools, frontend dependencies and git hooks (development only)
npm run dev:app             # opens the desktop app in development mode (with the Docker engine as a fallback)
npm run check               # everything that must be green before a merge
npm run bundle -- --bundles rpm   # builds the installable package (rpm, deb or appimage)
npm run release:build -- v0.9.0   # release packages (all three) in an Ubuntu 22.04 container
npm run test:engine         # end-to-end engine test with a local IPsec gateway
npm run test:docker         # Rust core integration with real Docker (engine, VICI, session)
npm run test:cli            # full CLI flow against the test gateway
npm run test:native         # compiled strongSwan (embedded engine) + end-to-end test
npm run test:systemd        # embedded engine with real systemd in a container (sandbox, 2 users, attack)
npm run test:modes          # IPsec modes on the same path: IKEv1 main/aggressive, PSK, XAuth, IKEv2; NAT-T, PFS, VIP, full tunnel
npm run test:packaging      # network component installer, as root in a container
npm run test:compat         # test:systemd repeated on seven distributions (Ubuntu, Debian, Fedora, openSUSE, AlmaLinux)
tests/compat/packages.sh <dir>  # installs built .deb/.rpm on the same seven, with their package managers
scripts/test-native.sh      # installs the real component (sudo) and tests the CLI through it
scripts/check-private.sh    # looks for private data in the staged content
```

The end-to-end tests start a **fake IPsec gateway** in a container and connect
to it with IKEv1 aggressive mode, PSK + XAuth and modp1024/modp768, the same
family as legacy VPNs. They check:

- the interface, the routes, the split DNS and a ping through the tunnel;
- that the logs have no secrets;
- that everything is cleaned up on disconnect, even after the engine is
  killed;
- the full tunnel (`0.0.0.0/0`), in an isolated network namespace;
- that a malicious VICI client cannot choose the command run as root.

No test uses a real VPN.

### Repository layout

```
crates/core/     Rust core: profiles, secrets, engine, session, diagnostics, network component, i18n
crates/cli/      ipsec-client-cli
crates/proto/    protocol between the app and the helper (shared validation)
crates/helper/   ipsec-client-helper: the network component that runs as root
src-tauri/       desktop app (Tauri v2): thin commands over the core, DTOs, tray, packages
src/             interface (React 19 + TypeScript + Tailwind 4 + shadcn/ui components), src/i18n
native-engine/   embedded strongSwan build (source verified by SHA-256 + GPG, security patches)
engine/          engine scripts (entrypoint, updown, config) and the test Docker image
packaging/       systemd units, polkit policy and the network component installer
scripts/         utilities (private data, shellcheck, component bundle, release)
tests/           end-to-end tests (engine, CLI, systemd) and the installer test
docs/            guides, architecture, roadmap, development log, ADRs; docs/pt-BR in Portuguese
assets/          logo (icon source)
presets/         generic profile examples (yours go in presets.local/, ignored)
```

### Workflow

- One branch per feature.
- [Conventional Commits](https://www.conventionalcommits.org/) in English
  (the history up to v0.1.1 is in Portuguese).
- `--no-ff` merges into `main` with `npm run check` green.

The details are in [CONTRIBUTING.md](CONTRIBUTING.md).

## Documentation

- **[Installation guide](docs/installation.md)** ([pt-BR](docs/pt-BR/instalacao.md))
- **[Quick start: connect with a profile you received](docs/quick-start.md)** ([pt-BR](docs/pt-BR/guia-rapido.md))
- [Connection profiles (format, swanctl → profile, sharing with colleagues)](docs/profiles.md)
- [Command line (ipsec-client-cli)](docs/cli.md)
- [Architecture](docs/architecture.md)
- [Architecture decisions (ADRs)](docs/adr/README.md)
- [Roadmap](docs/roadmap.md)
- [How to build and publish a release](docs/release.md)
- [Current state and how to resume development](docs/resume.md)
- [Development log](docs/development-log.md)
- [Audit of 0.2.0](docs/audits/v0.2.0.md)
- [Security](SECURITY.md) ([pt-BR](docs/pt-BR/seguranca.md)) · [Contributing](CONTRIBUTING.md) · [Changelog](CHANGELOG.md)

## License

[GPL-3.0-or-later](LICENSE). Any redistributed version must stay open source,
which is what you expect from a security tool. The reason for the choice is in
[ADR-0017](docs/adr/0017-gpl-3-license.md). strongSwan, GMP and musl, embedded
in the engine, and the libraries compiled into the app have their own
licenses: see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). The installed
app carries every text in `/usr/lib/ipsec-client/licenses/`, and each release
attaches the source of the embedded engine.
