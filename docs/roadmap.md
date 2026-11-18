# Roadmap

> Up to 0.9 the project was called IPsec Client; the sections below keep the
> names of their time ([ADR-0031](adr/0031-renamed-to-ikebox.md)).

Each line is a branch, created from `main` and merged with `merge --no-ff`,
with `npm run check` green. The branch is deleted after the merge; the merge
commit keeps its name.

## Final goal

A **Linux desktop app that replaces FortiClient**: good-looking, simple and
secure, distributed as an **AppImage** (download and run) and as rpm/deb.
Requirements:

- the user **installs nothing besides the app**: no Docker, no Node/npm, no
  Rust;
- npm and Rust only exist on the developer's machine and in CI. Tauri compiles
  the frontend into the binary.

### Decisions of 2026-11-16 (maintainer)

| Topic | Decision |
|---|---|
| Engine | **Embedded in the app**: strongSwan compiled by us, no Docker for the user. Docker stays only in the tests |
| Privilege | Authorized **once at installation**, with the administrator password. After that it connects without a password, like NetworkManager |
| strongSwan | The **newest version that works** (6.x → 5.9.14), with 5.8.2 as a fallback |
| Order | **Engine proof → GUI → embedded engine → release** |
| Formats | AppImage + rpm + deb. **No Flatpak**: the sandbox prevents netlink/XFRM and privileged services |

## Target architecture

```
user: App (Tauri 2 + React) or CLI
  └── crates/core (profiles · secrets · session · vici · diagnostics · i18n)
       └── Backend
            ├── native ──(Unix socket + polkit)──► ipsec-client-helper (root, systemd)
            │                                        └── embedded charon (sandboxed systemd-run)
            └── Docker (tests/dev only)
```

## Done (base: core + CLI)

| # | Branch | Delivery | Status |
|---|---|---|---|
| 0 | `main` | README, ADRs 0001–0010, roadmap, development log, private data guard | ✅ |
| 1 | `chore/tooling` | lefthook, commitlint, shellcheck, `npm run check`, first CI | ✅ |
| 2 | `feat/engine-imagem` | Container engine + E2E with a local IPsec gateway | ✅ |
| 3 | `chore/scaffold-rust` | Cargo workspace, toolchain, `cargo-deny`, Rust CI | ✅ |
| 4 | `feat/engine-rust` | `core::engine`: embedded image and container lifecycle | ✅ |
| 5 | `feat/vici` | Our own VICI client (ADR-0011) | ✅ |
| 6 | `feat/perfis` | JSON profiles, validation, `load-conn` generation (ADR-0012) | ✅ |
| 7 | `feat/segredos` | Credentials, keyring (libdbus), masking | ✅ |
| 8 | `feat/sessao-cli` | Session, failure diagnosis, `ipsec-client-cli` | ✅ |
| 14 | `feat/import-swanctl` | Import `swanctl.conf` without reading the secrets | ✅ |
| — | `docs/guia-rapido` | Guide for people who receive an exported profile | ✅ |

## Phases

### Phase A: close the base

| Delivery | Acceptance | Status |
|---|---|---|
| Push `main` and the branches; first CI run on GitHub | CI green (or fixes in `fix/ci`) | ✅ (pushed by the maintainer) |
| Real validation by the maintainer (`connect` + Remmina) | recorded in the log | ✅ through the app (step 20); a long-duration test is still missing |

### Phase B: embedded engine proof (decides the path)

| Branch | Delivery | Acceptance | Status |
|---|---|---|---|
| `spike/engine-nativo` | `native-engine/`: compiles strongSwan in a container with an old glibc, monolithic, with its own plugins (static gmp, no OpenSSL) and a verified tarball. Tests 6.x → 5.9.14 → 5.8.2 | Against the test gateway, with charon on the host: IKEv1 aggressive + PSK + XAuth + modp1024/768, `ipsc0` with `if_id`, VICI by the user, updown, ping. Measure size and dependencies | ✅ all 4 versions passed the 26 checks; engine of about 4 MB, depending only on glibc |
| (same) | ADRs 0013 (embedded engine), 0014 (helper + polkit), 0015 (distribution without Flatpak), 0016 (strongSwan version) | accepted | ✅ strongSwan 6.1.0 chosen (fallback: 5.8.2) |

### Phase C: graphical interface

| Branch | Delivery | Status |
|---|---|---|
| `chore/scaffold-gui` | Tauri 2.12 (stable) + React 19 + Vite + Tailwind 4 + shadcn/ui, biome, vitest, tauri-specta pinned, minimal capabilities, strict CSP | ✅ |
| `feat/ui-conexao` | Main screen (profile, Connect/Disconnect button, state with VIP, time and traffic), credentials with "remember", clear errors, masked log | ✅ |
| `feat/ui-perfis` | Preset manager: import JSON/swanctl.conf (native dialog and drag and drop), **export**, details, delete | ✅ |
| `feat/ui-editor-perfis` | Create/edit/duplicate profiles in the interface, with tabs and field-by-field validation from Rust | ⏳ |
| `feat/ui-diagnostico` | System tab: diagnostics (`doctor` with hints), light/dark/system theme, version | ✅ |
| `feat/tray` | Tray, notifications, single instance | ✅ |

### Phase D: the real embedded engine

| Branch | Delivery | Status |
|---|---|---|
| `feat/engine-nativo-build` | Reproducible strongSwan build in CI. Partly done: the build and the E2E already run in CI (`npm run test:native`), and the tarball goes into the component bundle (step 28). Still missing: checking reproducibility (same hash in two builds) and using the CI artifact in the release bundle | ⏳ |
| `feat/helper` | `crates/helper` (root): systemd socket, `SO_PEERCRED` + polkit, charon via `systemd-run` with hardening; `crates/proto`; units, policy and installer (`packaging/`) | ✅ validated as root (step 27) |
| `feat/backend-nativo` | `Backend` (native when the network component is installed; Docker as a fallback), delivered with `feat/helper` | ✅ |
| `feat/setup-appimage` | First use of the AppImage: installs the network component with the administrator password, once (ADR-0018) | ✅ validated by the maintainer (real password prompt, step 30) |

### Phase E: distribution

| Branch | Delivery | Status |
|---|---|---|
| `chore/instalacao-desktop` | Packages that show up in the menu (name `ipsec-client`, own shortcut in the Network category), [installation guide](installation.md), quick start through the app | ✅ |
| — | **Version 0.1.0**: first working version, validated by the maintainer on the real VPN with the app installed from the `.rpm` and the embedded engine, without Docker (step 30) | ✅ |
| `chore/release-pacotes` | Release packages built in an Ubuntu 22.04 container (`scripts/build-release.sh`), checked (`scripts/verify-release.sh`) and published by hand on GitHub Releases ([guide](release.md)); tag `v0.1.0` | ✅ (step 31) |
| `chore/release` | Automate in CI: build the packages and attach them to the release when the tag is created, with signed checksums. Includes the rpm/deb post-install (`install-native.sh --bundle /usr/lib/ipsec-client/network --origin package`) and, as an idea, the AppImage itself offering "Add to the applications menu". Acceptance: on a clean VM, with only the AppImage, connect with an exported profile | ⏳ (the manual part already exists: `chore/release-pacotes`) |
| `feat/updater` (optional) | Signed automatic update | ⏳ |

### Security and privacy audit (0.2.0)

| Branch | Delivery | Status |
|---|---|---|
| `fix/engine-updown-forcado` | **Local escalation to root fixed** (ADR-0019): the VICI client no longer chooses the `updown` or the `if_id` (strongSwan patch); DNS through the helper; VICI socket `0600` of the uid; `stop`/`logs` only for the owner; parallel helper; tighter engine sandbox; attack tests (`test:native`, `test:systemd`) | ✅ (step 32) |
| `feat/log-anonimizado` | "Copy anonymized" in the app and `logs --anonymize` in the CLI: gateway, IPs, domains and DNS replaced with stable placeholders (`Anonymizer`, in Rust) | ✅ (step 33) |
| `chore/higiene-privacidade` | Private data scan inside the packages (`check-private.sh --artifacts`, at the end of every release build), dev server does not serve private files, private files outside the project folder | ✅ (step 34) |
| — | charon without root (own user + ambient capabilities), after validating `resolvectl` without root | ⏳ (idea) |

### Internationalization (0.2.0)

| Branch | Delivery | Status |
|---|---|---|
| `feat/i18n-core` | Bilingual Rust messages (`tr!`/`trf!`), English CLI and helper, `set_language` | ✅ (step 36) |
| `feat/i18n-ui` | Bilingual interface with a language setting (follows the system); scripts, installer, tests and package metadata in English | ✅ (step 36) |
| `docs/english` | Technical documentation in English (ADRs renamed); user documentation in English with Portuguese copies in `docs/pt-BR/` | ✅ (step 36) |

**Before going public:** history scan (`check-private.sh --history` +
gitleaks). The commit email was already rewritten to the GitHub noreply
address (0.2.0). The license is defined (GPL-3.0-or-later, ADR-0017).

## Post-0.2.0 program (decided on 2026-11-16)

The maintainer asked for a new audit of 0.2.0 (security, credentials,
compatibility), deeper optimization (including the base images), a
better-looking interface and more IPsec modes for legacy VPNs. It runs **one
phase at a time**: each phase fits one working session, starts from
[resume.md](resume.md) and this section, and ends closed (local `--no-ff`
merges, green tests, resume/log/CHANGELOG updated). Product decisions of a
phase are asked at its start.

Rules for every phase:

- audit findings: clear security, leak or compatibility flaws are fixed right
  away, each with a regression test; everything else is a proposal in the
  report. No tag or release until the maintainer decides;
- privacy: never read credentials, the keyring, private profiles or the real
  VPN log; dynamic tests only against the test gateway with fake credentials;
  scans print files and counts only.

| Phase | Goal | Delivery | Status |
|---|---|---|---|
| 0 | Preparation | old `v0.1.1` tag removed from the remote (it pointed to the pre-rewrite history); this program recorded here | ⏳ (the remote tag is deleted by the maintainer) |
| 1 | Audit I: credentials, private data, supply chain | [audit report](audits/v0.2.0.md) (part 1), fixes, canary sweep in `test:systemd` | ✅ (step 37) |
| 2 | Audit II: privilege boundaries | [audit report](audits/v0.2.0.md#part-2-privilege-boundaries) (part 2), fixes, new scenarios in `test:systemd` | ✅ (step 38) |
| 3 | Compatibility across distributions | [audit report](audits/v0.2.0.md#part-3-compatibility) (part 3), `npm run test:compat` and `tests/compat/packages.sh` on seven distributions, fixes | ✅ (step 39) |
| 4 | Optimization and base images | [ADR-0020](adr/0020-static-engine-and-lighter-builds.md): static musl engine, release without the Docker engine, Debian 13 test images, fat LTO; P9–P11 of the audit | ✅ (step 40) |
| 5 | Interface: 5a review and visual directions; 5b implementation | review and gallery (`scripts/ui-review/`); direction B chosen, [ADR-0021](adr/0021-interface-state-color.md); released in 0.4.0 | ✅ (steps 41–42) |
| 6 | IPsec modes I: certify what the profile already accepts, design schema 2 | `npm run test:modes` (8 modes, 7 diagnosed failures; fixed profiles without a virtual IP), security level per profile, IKEv2 diagnosis, [ADR-0022](adr/0022-profile-schema-2.md) to [ADR-0025](adr/0025-deferred-xauth-otp-l2tp.md); released in 0.5.0 | ✅ (step 43) |
| 7 | Profile editor and schema 2 in the code | schema 2 with rounds and one-time XAuth passwords ([ADR-0022](adr/0022-profile-schema-2.md)); editor with four templates and three proposal presets; released in 0.6.0 | ✅ (step 45) |
| 8 | IKEv2 + EAP-MSCHAPv2 | [ADR-0023](adr/0023-ikev2-eap-mschapv2.md): engine with eap-identity/eap-mschapv2/md4, EAP profiles with the gateway certificate always checked (RSA/Ed25519, no revocation check), editor, diagnosis, E2E; released in 0.7.0 | ✅ (step 46) |
| 9 | Client certificates | [ADR-0024](adr/0024-client-certificates.md): `.p12` import (modern and 3DES/RC2) into the keyring, certificate profiles in IKEv2 and IKEv1 main mode (XAuth optional), app, CLI, diagnosis, E2E; released in 0.8.0 | ✅ (step 47) |
| 10 | Importers for legacy clients | Cisco `.pcf`, vpnc, NetworkManager, `ipsec.conf`, FortiClient XML, secrets never read ([profiles.md](profiles.md#importing-from-other-vpn-clients)); released in 0.9.0 | ✅ (step 48) |

### Phase 1: audit I (credentials, private data, supply chain)

- Baseline: `npm run check`, `cargo deny`, `npm audit`, pinning (Actions by
  SHA, images by digest, strongSwan by SHA-256 + GPG), upstream advisories
  after strongSwan 6.1.0 and for Tauri/WebKitGTK, gitleaks,
  `check-private.sh`, hardening flags of the shipped binaries.
- Credentials, static: every `expose_secret`, `Debug`/`Serialize` of secret
  types, command DTOs, errors and `tracing`, CLI prompt, credentials dialog
  state, keyring items, core dump protection, charon log level.
- Organization data visible to other local users: transient unit environment
  and description, journal access; what is inherent (routes, VIP,
  `resolvectl status`) and what can be closed.
- Dynamic: **canary sweep** in the `test:systemd` container (connect with
  canary PSK, username and password; look for them in the journal,
  `systemctl show`, `/proc`, runtime directories, profiles, exports and
  logs). It becomes a permanent scenario.

### Phase 2: audit II (privilege boundaries)

| Boundary | Where | What to check |
|---|---|---|
| Local user/malware → helper (root) | `crates/proto`, `crates/helper/src/` | read limits, `SO_PEERCRED`, pid race in polkit, authorization of every request, argument injection into `systemd-run`/`journalctl`, TOCTOU in `/run/ipsec-client/<uid>` |
| Sandbox | `systemd.rs`, `packaging/*.service` | `systemd-analyze security` (helper on the host, engine in `test:systemd`), missing properties, capabilities really needed |
| User → charon (VICI) | `native-engine/patches/`, strongSwan source | repeat the ADR-0019 attack; other VICI options that make root charon read a file, load a module or run something |
| Gateway/network → client | `engine/strongswan.d/`, `entrypoint.sh`, `updown.sh` | minimal plugins, DoS limits, validation of what the gateway pushes, fake gateway holding the group PSK |
| Webview → Rust commands | `src-tauri/src/`, `capabilities/`, CSP | the 18 commands: input validation, import/export paths, devtools, external navigation, single instance |
| Installer / pkexec | `crates/core/src/component.rs`, `packaging/install-native.sh` | hashes and TOCTOU; in rpm/deb the bundle is already root-owned, so the copy to `$XDG_RUNTIME_DIR` opens a needless window |
| Files and scripts | `crates/core/src/profiles/`, `engine/*.sh` | modes, atomic writes, symlinks, parser limits, every expansion of external input |
| CI and release | `.github/workflows`, `scripts/build-release.sh` | permissions, triggers, secrets |

### Phase 3: compatibility

Matrix in clean containers: Ubuntu 22.04 and 24.04, Debian 12 and 13, current
Fedora, openSUSE Tumbleweed.

- Installation and dependencies: declared (`dpkg-deb -I`, `rpm -qpR`) versus
  used (polkit/`pkexec`, `iproute2`, `systemd-resolved` are not declared).
- Binaries: `ldd` and the highest glibc required by the app, helper and
  engine.
- systemd: minimum version of every sandbox property; Debian/Ubuntu variants
  of `test:systemd`. With them, try the helper unit without `CAP_CHOWN` and
  `CAP_DAC_READ_SEARCH` (audit P12).
- Script tools: `ip` with `if_id`, `resolvectl` (without `systemd-resolved`
  the VPN DNS does not work: does the diagnostics say so?), bash features.
- Immutable distributions: the polkit policy goes to
  `/usr/share/polkit-1/actions`, read-only there.
- Desktop: Wayland/X11, tray, AppImage on Ubuntu 24.04 (AppArmor), WebKitGTK
  with NVIDIA; `pt_PT`, `C.UTF-8`, `LANGUAGE`; 0.1.0 → 0.2.0 update.

### Phase 4: optimization and base images

Measure first, decide by ADR, then change. Starting recommendation:

| Image | Today | Proposal to evaluate | Why |
|---|---|---|---|
| Release build | Ubuntu 22.04, 2.63 GB | **keep the base**, slim it (no recommended packages, multi-stage, cargo/npm cache) | Ubuntu 22.04's glibc 2.35 is the compatibility floor; Debian 12 (2.36) would drop Ubuntu 22.04 |
| Engine build | Ubuntu 22.04, 568 MB, static with glibc | **Alpine + musl, fully static** | the engine stops depending on glibc and runs on any distribution; much smaller build image |
| Docker engine (dev/tests) | Ubuntu 20.04 + strongSwan 5.8.2 | **Debian slim (or Alpine) with our engine 6.1.0** | tests exercise the production engine; the unsupported Ubuntu 20.04 goes away |
| Test gateway | the same 5.8.2 image | **Debian slim with the packaged strongSwan** (full plugins, EAP included) | prepares the scenarios of phases 6+; keep a 5.8.2 scenario as a "legacy gateway" if worth it |

Also: Docker backend (`bollard`) out of the release build behind a feature;
release profile (`opt-level`, `panic`, LTO) measured before/after; duplicate
dependencies; status polling, timers with the window hidden, log streaming,
startup, time to "Connected", idle helper; loaded plugins and charon threads;
frontend bundle and log panel re-renders.

ADR criteria: compatibility must not get worse; the E2E, the ADR-0019 attack
and the canary sweep pass the same; size, build time, reproducibility. musl
only goes in if the full E2E passes, including name resolution inside the
sandbox.

### Phase 5: interface

- **5a, review and visual directions (no app changes):** screenshots of the
  real app (Vite with a scratch config that aliases `@tauri-apps/api` to a
  mock with RFC 5737 data, headless Firefox), states × {English, Portuguese}
  × {light, dark}; usability review (primary action, visible state, every
  error with a next step, text overflow at 380 px, keyboard/focus, ARIA,
  contrast, reduced motion); **2 or 3 visual directions** in a local gallery
  (`target/ui-review/`, nothing published). The maintainer chooses.
- **5b, implementation:** tokens in `src/index.css`, components and screens,
  both languages and themes, accessibility preserved, vitest updated.

### Phase 6: IPsec modes I

**Certify (implementation):** the profile already accepts IKEv1 main mode,
PSK without XAuth and IKEv2 + PSK, but only IKEv1 aggressive + PSK + XAuth is
tested.

- New E2E scenarios on the test gateway: IKEv1 main mode + PSK + XAuth, IKEv1
  PSK without XAuth, IKEv2 + PSK; with and without NAT-T, PFS, VIP, split and
  full tunnel.
- Validation and warnings: a per-profile security level (aggressive + PSK,
  DH < 2048, 3DES, MD5/SHA1 flagged as legacy, with an explanation);
  `remote_id` required where it protects against a fake gateway.
- Failure diagnosis for IKEv2; profile details show the mode and the security
  badge; the credentials dialog adapts; combinations table in
  [profiles.md](profiles.md).

**Design (ADRs, no implementation):** profile schema 2 with migration
(authentication rounds, trust anchors, new credential kinds); IKEv2 +
EAP-MSCHAPv2 (new plugins, server certificate always validated); client
certificates (PKCS#12 import, private key only in the keyring and sent to
charon through VICI as data, never as a path; OpenSSL/ECDSA versus RSA only);
deferred with a record: XAuth 2FA/OTP, L2TP/IPsec.

**Security principles for every new mode:**

- no path or command chosen by the client runs as root (ADR-0019 stands);
- new secrets follow the current path: memory → keyring → VICI, never disk,
  arguments, environment or logs;
- no "accept any certificate"; weak modes only with an explicit warning;
- new identities (certificate DN, EAP identity) enter the masking and "Copy
  anonymized";
- every new strongSwan plugin gets a surface review, an E2E test and a
  `BUILDINFO` record.

### Phases 7+ (planned, detailed at the end of phase 6)

Every phase keeps the security principles of phase 6 and ends with its
modes in `npm run test:modes` (success and diagnosed failures).

**Phase 7: profile editor and schema 2 in the code.**

- Schema 2 model, validation and migration ([ADR-0022](adr/0022-profile-schema-2.md)):
  read 1 and 2, write the lowest schema that expresses the profile;
  snapshot tests that every schema 1 fixture renders the same `load-conn`.
- Profile editor in the app (`feat/ui-profile-editor`): create and edit,
  generic vendor templates (FortiGate, Cisco, strongSwan, pfSense), the
  security badge updated while editing, "never remember the password" per
  profile ([ADR-0025](adr/0025-deferred-xauth-otp-l2tp.md)).
- Decision at the start: which templates, and whether the editor shows the
  raw proposals or picks them from a list.

**Phase 8: IKEv2 + EAP-MSCHAPv2** ([ADR-0023](adr/0023-ikev2-eap-mschapv2.md)).

- Plugins `eap-identity`, `eap-mschapv2`, `md4` with their surface review and
  `BUILDINFO` record; CA certificates loaded as data; gateway always
  authenticated by certificate.
- Test gateway with a CA made at test time; cases: good certificate, other
  CA, wrong identity, wrong password; their diagnosis.
- Credentials dialog with EAP identity and password; masking.

**Phase 9: client certificates** ([ADR-0024](adr/0024-client-certificates.md)).

- PKCS#12 import (pure Rust), key and certificate in the keyring, `load-key`
  and `load-cert` as data; expiry warning.
- RSA and Ed25519 first. Decision at the start, if a real gateway needs it:
  OpenSSL in the engine for ECDSA (its own ADR).

**Phase 10: importers for legacy clients.** Cisco `.pcf`, vpnc,
NetworkManager, `ipsec.conf`, FortiClient XML, mapped to schema 2, never
copying a secret into the profile (an encrypted `.pcf` group password is
not decrypted: the person types the PSK).

## Program to 1.0.0 (decided on 2026-11-18)

Goal: release **1.0.0** and open the repository, with security, the existing
features and optimization first. Same rules as the post-0.2.0 program: one
phase per session, product decisions at the start of each phase, every phase
closed (local `--no-ff` merges, green tests, resume/log/CHANGELOG updated).
No 0.10 release: day-to-day testing uses development builds
(`npm run release:build` without a tag) and wider testing uses
`v1.0.0-rc.N` tags.

### Maintainer decisions

| Topic | Decision |
|---|---|
| Publication | **New repository.** The current GitHub repository becomes a private archive (`ipsec-client-archive`); a new `ipsec-client` receives only `main` and the rewritten tags, so the commits from before the email rewrite never become public |
| Packages | Built by **CI on the tag**; a GPG signing subkey in a protected environment (maintainer approval) signs the `.rpm` and `SHA256SUMS`; GitHub build provenance attestations; the maintainer builds locally too and compares |
| Updates | **Optional notice**, off by default: once a day, look at the latest release and show a link. Nothing is downloaded or installed |
| Extra scope | Close to the tray; charon without root; ECDSA and ECP groups (OpenSSL in the engine); PKCS#11 |
| PKCS#11 | Starts with a study and an ADR. It goes into 1.0 only if it fits the static engine (ADR-0020) without relevant new root surface; otherwise it moves to 1.1 and does not hold the release |

### Phases

| Phase | Goal | Delivery | Status |
|---|---|---|---|
| 0 | Preparation | this program; stale local `v0.1.1` tag removed; history scanned (`check-private.sh --history`, gitleaks); new private repository created and pushed by the maintainer | ✅ (step 49; first CI run green) |
| A | Security and supply chain | P2 (store credentials only after connecting, app and CLI), P3 (Tauri 2.12.6), P4 and every unpinned build input (rustup-init, Node, AppImage tools, GMP/musl), CI hygiene, Dependabot rewritten | ✅ (step 50) |
| B | Complete packages | rpm/deb scripts manage the network component (`--origin package`; removing the package removes it), CLI with man page and completions in the packages, AppStream metainfo, third-party licenses in every package, corresponding source archive (`scripts/release-sources.sh`) | ✅ (step 51) |
| C | Engine with OpenSSL | [ADR-0026](adr/0026-strongswan-maintenance-policy.md) (strongSwan maintenance policy, audit P1); [ADR-0027](adr/0027-openssl-in-the-engine.md): OpenSSL replaces GMP, patch `0003`, ECDSA CAs and keys accepted, ECP in the "modern" preset; ECDSA gateway in `test:modes` | ✅ (step 52) |
| D | charon without root | [ADR-0028](adr/0028-charon-without-root.md): charon as `ipsec-client-engine` with `NET_ADMIN` only (setpriv, native capabilities), DNS through root on the session interface (FIFO), checked on the seven distributions; gateway-initiated phase 1/2 renewal in `test:modes` | ✅ (step 53) |
| E | PKCS#11 (gated) | study passed the gate; [ADR-0029](adr/0029-smartcards-and-tokens.md): the token stays in the person's process, charon signs through a key agent (patch `0004`). E1: engine, core, CLI, `test:modes` with SoftHSM (RSA and ECDSA, IKEv2 and IKEv1 + XAuth). E2: the app (editor option, certificate list and PIN in the connection dialog) and the user docs | ✅ (steps 54–55) |
| F | App | close to the tray (on by default, a notification the first time; System → Behavior), one status monitor in Rust feeding the tray and the window (no UI polling, nothing sent to a hidden window), optional update notice (off by default, once a day, `ureq` + rustls, version-only answer, packagers can build without it) | ✅ (step 56) |
| G | Release pipeline in CI (`chore/release`) | [ADR-0030](adr/0030-release-signing-and-provenance.md): `release.yml` (build, reproducibility, signing in the protected `release` environment, attestations, draft release), `weekly.yml` (upstream watch, advisories, reproducibility, the seven distributions with packages updated from the latest release), `scripts/sign-release.sh`, `check-reproducible.sh`, `upstream-watch.sh`; the signing key is the maintainer's step | ✅ (step 57; key and environment with the maintainer) |
| H | Opening | SECURITY.md for a public repository (private reporting, response times, supported versions), code of conduct, SUPPORT.md, issue forms with the privacy warning and the anonymized log, README for 1.0 with screenshots, Portuguese copies in sync; every code comment in English, presets and the test fixture renamed (fake values inside the tests stay as they are) | ✅ (step 58) |
| I | Acceptance and release | renamed **Ikebox** ([ADR-0031](adr/0031-renamed-to-ikebox.md)), with the move from IPsec Client tested on the seven distributions and on the real VPN; version 1.0.0, release notes, Portuguese copies of the contributing guide, code of conduct and support page; then the maintainer's release key, the repository made public, the tag and the signed release | ⏳ (steps 59–60; the key, the publication and the tag with the maintainer) |

Out of 1.0 (kept in [After 1.0](#after-10)): P8 `initiator_only` (needs a long
session on the real VPN), restricting 500/4500 to the gateway, several
connections, revocation checks, more languages; RHEL 9 stays unsupported
(P14).

**Release gate:** every `npm run check`/`test:*` suite green,
`tests/compat/packages.sh <1.0.0> --from <0.9.0>`, CI and the release
workflow green, the network component reproducible (same hash in two builds
and in the local build), `check-private.sh --history` and `--artifacts`
clean, gitleaks clean, the clean-VM checklist and the real VPN approved,
`gpg --verify` and `gh attestation verify` working from the release page.

## After 1.0

Not promised, in no particular order:

- P8 `initiator_only`: needs a session on a real gateway longer than its
  phase 1 lifetime (the [audit report](audits/v0.2.0.md) has the steps).
- Restrict ports 500/4500 to the gateway IP during the session.
- Several simultaneous connections. The per-profile `if_id` already allows it.
- Certificate revocation checks (CRL/OCSP).
- More languages, now that the i18n infrastructure exists.
- Long-duration test (rekey after about 11 h).
- English fake values inside the tests (`grupo-teste`, `usuario.exemplo`...):
  they cross the gateway configs and the E2E scenarios, so they change with
  the full suites.
- RHEL 9 and its rebuilds stay unsupported (P14: no WebKitGTK 4.1, glibc 2.34).
