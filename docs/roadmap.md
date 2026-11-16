# Roadmap

Each line is a branch, created from `main` and merged with `merge --no-ff`,
with `npm run check` green. Branches are kept as history.

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

## Ideas

- Several simultaneous connections. The per-profile `if_id` already allows it.
- Restrict ports 500/4500 to the gateway IP during the session.
- More languages, now that the i18n infrastructure exists.
- Certificate and EAP authentication (profile schema 2).
- Long-duration test (rekey after about 11 h).
- Option to close the window to the tray (today, closing quits the app while
  the VPN stays connected).
- Unify the UI and tray status polling.
