# Changelog

All notable changes to this project are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **IKEv2 with a username and password (EAP-MSCHAPv2)**
  ([ADR-0023](docs/adr/0023-ikev2-eap-mschapv2.md)): the "IKEv2 with
  username and password" template in the editor, as on Windows, macOS and
  Android. The gateway always proves itself with its certificate, checked
  against the CA in the profile (chosen from a file or pasted, with its
  subject and validity shown) and the gateway's name; there is no way to
  skip it. The embedded engine gains the eap-identity, eap-mschapv2 and md4
  plugins. Tested end to end, with the wrong password, another CA and a
  wrong gateway name each diagnosed in plain words.
- "Copy anonymized" and `logs --anonymize` also hide certificate names (the
  organization in them).

### Known limits

- Gateway certificates must be RSA or Ed25519: an ECDSA CA is refused when
  the profile is read. Revocation (CRL/OCSP) is not checked. Both are in
  SECURITY.md.

## [0.6.0] - 2026-11-17

Profiles made in the app, phase 7 of the program: an editor with vendor
templates, and profile schema 2 underneath. The network component does not
change, but it carries the new version number: the app offers to update it.

### Added

- **Profile editor:** **Profiles → New** starts from a template (FortiGate,
  Cisco Easy VPN, IKEv2 with a PSK, blank), and the pencil on a profile
  edits it. Proposals are picked by name ("modern", "compatible", "legacy",
  each with its security badge) or typed in an advanced mode. Rust checks
  every change: problems show field by field, and the security badge and
  its reasons update while typing. The identifier is fixed once saved.

- **Profile schema 2** ([ADR-0022](docs/adr/0022-profile-schema-2.md)):
  authentication as rounds (`auth.local`, `auth.remote`). The app reads
  schema 1 and 2 and writes the lowest schema that expresses the profile, so
  exported profiles keep opening on 0.5 and older unless they use something
  new.
- **One-time XAuth passwords:** `"otp": true` on the XAuth round. The code is
  asked at every connection and never stored; one stored before is removed.
  The credentials dialog says "One-time code" and why it is not remembered.
- The gateway identity (`remote_id`) is masked in logs too.

## [0.5.0] - 2026-11-17

IPsec modes, phase 6 of the program: every mode the profile accepts is now
tested on the production path, each profile shows how safe its settings
are, and IKEv2 failures are explained. The network component changes (the
`updown` fix below): the app offers to update it.

### Added

- **Security level per profile:** a badge (good, legacy or weak) on the
  profile card, in the import preview and at the top of the connection
  screen, and the reason for each point in the profile details: aggressive
  mode with a PSK, DH groups under 2048 bits, DES/3DES, MD5/SHA-1, IKEv1,
  and the group PSK + XAuth risk (anyone with the group PSK can pose as the
  gateway, audit P13). It only warns: weak profiles still connect. The CLI
  shows it in `profiles list`, `show`, `validate` and `import`.
- **Failure diagnosis for IKEv2**, learned from real strongSwan 6.1.0 lines:
  the gateway requiring a virtual IP is a cause of its own, and the
  authentication message names the gateway identity (`auth.remote_id`) too.

- Design of the next modes, no code yet: profile schema 2 with
  authentication rounds and gateway certificates (ADR-0022), IKEv2 +
  EAP-MSCHAPv2 (ADR-0023), client certificates (ADR-0024); XAuth with
  one-time codes and L2TP/IPsec deferred with the reasons (ADR-0025).

### Changed

- The credentials dialog of a profile without XAuth asks for the
  "Pre-shared key (PSK)" instead of a "Group PSK".

### Fixed

- **IKEv1 main mode with a wrong PSK** said the gateway did not answer: its
  answers just did not decrypt. It now says to check the PSK.

- **Profiles without a virtual IP** (`network.request_vip: false`) connected
  but installed no route, so no traffic went through the tunnel. charon
  calls the `updown` with `up-host` when our side is the computer's own
  address, and the `updown` took it for a host-to-host tunnel. Found by the
  new mode tests.

### Added

- `npm run test:modes`, also in CI: every IPsec mode the profile accepts,
  certified on the production path (CLI → helper → embedded strongSwan
  6.1.0, real systemd): IKEv1 main mode with PSK, with and without XAuth;
  IKEv1 aggressive mode with PSK only; IKEv2 with PSK and a checked gateway
  identity; with and without forced NAT-T, PFS (including on an IKEv2
  rekey), virtual IP, split and full tunnel.

## [0.4.0] - 2026-11-17

A new look, chosen by the maintainer from three directions
([ADR-0021](docs/adr/0021-interface-state-color.md)). The network component
does not change, but it carries the new version number: the app offers to
update it, which is quick.

### Changed

- **The connection screen is colored by the state:** slate when
  disconnected, amber while connecting, green when connected, red when it did
  not connect. The top holds the profile, the state in large type and the one
  action to take now; the details come below in big tiles.
- Errors say what happened and what to do, with the technical details folded.
- The tabs moved to the bottom of the window.
- First use numbers its two steps (install the network component, import the
  profile).

### Fixed

Accessibility, found in the interface review:

- Text contrast in the light theme reaches WCAG AA everywhere (the
  "Preparing…" text was at 2.18:1).
- Every control shows keyboard focus.
- The dialogs' close button and the diagnostic levels speak the interface
  language to screen readers (the close button said "Fechar" in English).
- The system's "reduce motion" setting stops the spinning and pulsing.
- The title no longer wraps in the header at the default window width.

## [0.3.0] - 2026-11-17

Fixes from the [audit of 0.2.0](docs/audits/v0.2.0.md) (credentials,
privilege boundaries, compatibility) and the optimizations of
[ADR-0020](docs/adr/0020-static-engine-and-lighter-builds.md). They change
the network component: after updating the app, click "Update" on it.

### Security

- **XAuth username unmasked in logs.** The log masking only knew the
  identities of the current session. When the credentials were not in the
  keyring and another process read the log, the username was shown as is in
  the log panel, in `ipsec-client-cli logs` and in "Copy anonymized".
  Identities are now masked wherever charon prints them, known or not.
- **No core dumps.** The app, the CLI, the network component and charon no
  longer leave a core dump on a crash, which could put credentials and
  session keys on disk.
- **The network component only touches the VPN's own interface.** A user of
  the active session could make it delete another program's network interface
  (`docker0`, `wg0`, a bridge) or reset its DNS, by asking for that name. Only
  `ipsc<N>` is accepted now, and a link with that name that is not the VPN's
  is left alone.
- **Another user's session no longer shows up in the log.** On a shared
  computer, a user who connected after someone else received the end of that
  person's log (gateway, group id, VPN username). Each session now gets only
  its own lines.
- **Text sent by the gateway cannot drive the terminal.** The gateway's XAuth
  message goes into the log as it comes, and the CLI printed it unchanged:
  escape sequences could rewrite the terminal or fake log lines. Control
  characters are replaced in every log line.
- **Installing from the `.rpm`/`.deb` no longer runs a user file as root.**
  The installer runs from the app's root-owned directory instead of a copy in
  the user's runtime directory, which a program running as the user could
  swap after the password prompt. The AppImage still needs the copy.
- Network component hardening: a slow client can no longer hold it (deadline
  for the whole request, limit per user), the client process is checked
  again before polkit is asked, and its unit has a syscall filter.
- Imported files are read with a real size limit (a device or a pipe is
  refused), and `IPSEC_CLIENT_HELPER_SOCKET` only works in development
  builds.
- **charon no longer opens files named by the VICI client.** The session user
  could name a certificate by path, and charon, as root, opened it, which told
  whether the file existed. The embedded strongSwan carries a second patch
  that refuses certificate paths and tokens.
- The network component chooses the XFRM interface ID itself instead of
  taking the app's, so a session cannot touch the SAs of another IPsec stack
  on the computer.

### Fixed

Compatibility across distributions (audit of 0.2.0, part 3):

- **Missing dependencies on a clean installation.** The `.deb` and the
  `.rpm` only declared WebKitGTK, GTK and the tray library. Without `pkexec`
  the network component could not be installed, and without `ip` (iproute2)
  the VPN could not start; on Fedora and openSUSE, systemd's tools, `find`
  and `awk` were missing too. They are declared now (on openSUSE, `pkexec` is
  a package of its own).
- **Image-based distributions** (Fedora Silverblue/Kinoite, openSUSE MicroOS):
  installing the network component failed halfway. The polkit policy now goes
  to `/etc/polkit-1/actions` when `/usr` is read-only (polkit 124 or newer),
  and `/opt` stays a link to `/var/opt` instead of being replaced.
- **Diagnostics said systemd-resolved was active after it stopped**, while
  the VPN DNS was not being applied. It is now asked directly.
- Messages built outside the window (tray, notifications, errors) follow
  `LANGUAGE`, which Ubuntu sets, and locales such as `pt_PT@euro`.
- When the remote network of the VPN contained the gateway's own address, the
  tunnel swallowed its own traffic. The gateway now keeps a route through the
  original path, as in the full tunnel.

### Changed

- **The embedded strongSwan is one fully static executable** (musl): it no
  longer depends on the computer's C library, and the network component is
  40% smaller, since tools that only the tests use left it (ADR-0020).
- **The app no longer carries the Docker engine**, meant for development: the
  app is 17% smaller (the `.rpm`/`.deb` packages, 23%) and the network component is the only engine of a
  release build (development builds: `npm run dev:app`).
- The Docker engine and the test gateway run on Debian 13 with strongSwan
  6.0.1, instead of Ubuntu 20.04 and strongSwan 5.8.2.
- The network component's service runs with no capability at all.
- The installation guide lists the tested distributions, says how to get the
  VPN DNS on Debian and openSUSE, and how to install on RHEL, Rocky and
  AlmaLinux 10 (with EPEL). Version 9 of those is not supported: it has
  neither WebKitGTK 4.1 nor a recent enough glibc.
- `npm run test:compat` repeats the systemd test on Ubuntu 22.04 and 24.04,
  Debian 12 and 13, Fedora 45, openSUSE Tumbleweed and AlmaLinux 10, with the
  real polkit of each; `tests/compat/packages.sh` installs the built packages
  on the same seven.
- `scripts/verify-release.sh` checks the glibc required by every library
  inside the AppImage, not only by the app; a test build of the packages no
  longer replaces those of a published version.
- `npm run test:systemd` has a canary sweep: it looks for the test
  credentials in every file, process, journal entry, unit property and CLI
  output while connected and after disconnecting.
- `npm run test:systemd` also covers idle and slow clients (the old scenario
  never held a connection), the log of a second user, interface names that
  are not the VPN's, and the sandbox of the helper unit.
- SECURITY.md says what a group PSK does not protect against: whoever holds
  it can pose as the gateway.

## [0.2.0] - 2026-11-16

Security, privacy and internationalization release, the result of a security
and privacy audit. **If you use 0.1.0, update the app and then click "Update"
on the network component** (System → Network component). The security fix
lives in the component, so replacing the package alone is not enough.

### Security

- **Local privilege escalation fixed (ADR-0019).** In the embedded engine,
  the VICI client (the user's side) chose the `updown` command, which charon
  runs as root. A program running as the user of the active session could run
  commands as root, without a password. It was not exploitable from the
  network or by the gateway.
  - A minimal strongSwan patch makes charon ignore the `updown` and `if_id`
    coming from VICI and use the ones set by root.
  - The `updown` arguments go in a file, never through a shell, and the
    profile DNS now goes through the helper, validated.
- VICI socket `0600` owned by the user (before, `0660` for the group).
- The helper only lets the session owner stop the VPN and read its log, and
  serves connections in parallel: an idle client no longer blocks it.
- Tighter engine sandbox: no real devices, read-only `/proc/sys` and `/sys`,
  no view of other processes or of the kernel log, and a syscall filter.
- The builds refuse a strongSwan without the security patches of the commit.

### Privacy

- Leak audit: no data from the maintainer's organization in the code, the
  history or the 0.1.0 packages.
- **"Copy anonymized"** in the app and `logs --anonymize` in the CLI: the
  gateway, IPs, DNS and domains become placeholders (`<gateway>`, `<ip-1>`,
  `<domain-1>`) in the text that leaves the app.
- `scripts/check-private.sh --artifacts` scans the inside of the release
  packages and runs at the end of every release build.
- The development server does not serve private files left in the project
  folder.
- The commit email in the whole history was replaced with the GitHub noreply
  address.

### Added

- **The app speaks English and Portuguese.** It follows the system language
  (Portuguese on a `pt*` locale, English otherwise) and can be changed in
  System → Appearance → Language. Rust messages (errors, diagnostics, tray,
  notifications) follow the same choice.
- `npm run test:systemd`: real systemd in a container, with the real
  installer, the applied sandbox, two users and the attack.
- The `test:native` E2E got the attack scenario. It passes with the patch and
  fails with the 0.1.0 engine.

### Changed

- The CLI, the helper, the installer and the scripts speak English.
- The technical documentation is in English (ADRs renamed); the user
  documentation is in English with Portuguese copies in `docs/pt-BR/`.
- Commits are written in English from now on.
- Engine readiness line: `[engine] ready`; installer lines tagged
  `[network component]`.

### Fixed

- Two bugs only real systemd revealed: the socket `chmod` after `chown`
  (root without `CAP_FOWNER`) and the helper readiness check connecting to a
  socket owned by another uid.
- `cmd | grep -q` under `pipefail` could turn a match into SIGPIPE: in
  `check-private.sh --artifacts`, matches inside ELF files could be missed;
  in the E2E, the secret checks were unreliable.

## [0.1.0] - 2026-11-16

First working version. On 2026-11-16, the maintainer validated it on a real
FortiGate VPN (IKEv1 aggressive + group PSK + XAuth), through the same path as
an end user:

- app installed from the `.rpm` and opened from the applications menu;
- network component installed by the app itself, with the administrator
  password;
- connection through the embedded strongSwan 6.1.0, without Docker, and
  remote access (RDP) through the VPN.

The packages (AppImage, rpm and deb) are built by `scripts/build-release.sh`
in an Ubuntu 22.04 container and published on GitHub Releases.

### License

- The project is **GPL-3.0-or-later** (ADR-0017). `THIRD_PARTY_NOTICES.md`
  identifies the strongSwan and GMP distributed in the engine, together with
  the corresponding source code. Their license texts go in the engine package.

### Added

- Initial documentation: README, architecture, roadmap, development log and
  ADRs 0001–0010.
- Private data guard (`scripts/check-private.sh`), with local patterns outside
  git.
- Repository tooling: lefthook (pre-commit, commit-msg, pre-push), commitlint
  (Conventional Commits), shellcheck (local or through a pinned Docker image)
  and `npm run check`.
- CI on GitHub Actions (checks + gitleaks) and dependabot.
- Engine (`engine/`): Ubuntu 20.04 image pinned by digest + strongSwan
  5.8.2-1ubuntu3.6, entrypoint with a dedicated XFRM interface, a VICI socket
  restricted to the user and cleanup on SIGTERM, updown with `proto 241` routes
  and split DNS via resolvectl.
- End-to-end engine test (`npm run test:engine`) against a local IPsec
  gateway (IKEv1 aggressive + PSK + XAuth), also in CI. Scenarios: normal
  cycle, recovery after SIGKILL, full tunnel on an isolated network and modp768
  PFS.
- Rust workspace (`crates/core`, `crates/cli`) with a pinned toolchain
  (1.100.0), lints, `cargo-deny` and checks in `npm run check`, the hooks and
  CI.
- `core::engine`: embedded image context with a hash and a deterministic tar,
  a validated `EngineSpec` (least privilege), image build and container
  lifecycle via the Docker API (Unix socket only), following the readiness
  contract.
- `core::vici`: our own VICI client (ADR-0011), with messages wiped on drop,
  command and event connections and the typed `Charon` trait. An integration
  test establishes a full IPsec tunnel against the test gateway from Rust
  alone.
- `core::profiles`: JSON profile (schema 1, no credentials), field-by-field
  validation, `load-conn` and `updown` generation (single quotes +
  validation), per-profile `if_id` and a store with atomic saves and private
  permissions. Includes the generic `presets/`, the profiles guide (with the
  swanctl → profile mapping) and ADR-0012.
- `core::secrets`: credentials in `SecretString`, the system keyring (Secret
  Service via libdbus), in-memory storage, `load-shared` secrets and identity
  masking in logs.
- `core::diagnostics` and `core::session`: preflight without root, connection
  with events, `status`/`disconnect` from any process and diagnosis of failure
  causes (PSK/group, XAuth, IKE/ESP proposals, networks, unreachable gateway).
- `ipsec-client-cli`: `doctor`, `profiles`
  (list/show/import/export/validate/delete/path), `secrets` (set/status/forget,
  keyring), `connect` (foreground with Ctrl+C, `--detach`, `--remember`,
  `--credentials-stdin`), `status` (exits with 3 when disconnected), `logs`
  (masked) and `disconnect`. `npm run test:cli` end-to-end test.
- `profiles import-swanctl`: creates the profile from an existing
  `swanctl.conf`, skipping the `secrets` section without reading it and
  without copying the XAuth username.
- A quick start guide for people who receive an exported profile.
- Embedded engine (`native-engine/`): strongSwan compiled from the official
  source (SHA-256 + GPG), static, without OpenSSL. charon depends only on glibc,
  and the whole engine is about 4 MB. Testable versions: 6.1.0 (default),
  6.0.7, 5.9.14 and 5.8.2 (`npm run test:native`). ADRs 0013–0016.
- Desktop app (Tauri 2.12, React 19, TypeScript, Vite 8, Tailwind 4,
  shadcn/ui; typed IPC with tauri-specta, strict CSP, minimal capabilities, no
  shell/fs plugins): connection screen, profile manager (import, export,
  details, delete), tray, notifications, single instance and a System tab
  (diagnostics, theme, version).
- Network component (embedded engine, no Docker): `crates/helper`, the only
  root component (systemd socket activation, `SO_PEERCRED`, polkit, charon via
  `systemd-run` with a sandbox); `crates/proto`; `packaging/` with the systemd
  units, the polkit policy and the installer.
- **The app installs the network component** (ADR-0018), with the
  administrator password once (`pkexec`), notices when it carries a different
  component and offers to update it; `ipsec-client-cli component
  status|install|uninstall`; installer test as root in a container.
- Installation guide (rpm, deb and AppImage, first use, update, uninstall,
  building from source, common problems) and an own desktop entry.
- Release packages: `scripts/build-release.sh` (`npm run release:build`) and
  `scripts/verify-release.sh`, with the release guide and the 0.1.0 notes.

### Changed

- The helper goes compressed in the component bundle
  (`ipsec-client-helper.gz`): in the AppImage, linuxdeploy rewrote the binary
  and the hash stopped matching. Found before publishing.
- Packages named `ipsec-client`, resources in `/usr/lib/ipsec-client` and
  maintainer "IPsec Client contributors".
- The profile MTU is applied to the engine interface (before, always 1400).
- Example IKEv2 preset: `curve25519` instead of `ecp256`, because the
  embedded engine has no ECP.
