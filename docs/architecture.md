# Architecture

This document describes **how** IPsec Client works. The **why** behind each
choice is in the [ADRs](adr/README.md).

## Overview

```
React UI ──(tauri-specta: typed commands/events)──► src-tauri (thin commands, state, tray)
                                                       │
                                            crates/core (ipsec-client-core)
   profiles    model, validation, presets, import/export, load-conn generation
   secrets     keyring (Secret Service) + memory (secrecy/zeroize) + redaction/anonymization
   engine      Backend: native (helper) or Docker (tests)
   session     state machine, VICI (Charon trait), events, errors
   diagnostics checks with stable ids (component, ports, resolved, keyring, xfrm_interface)
   component   network component bundle, state, install/remove via pkexec
   i18n        English/Portuguese messages (tr!/trf!)
                 │ helper socket (JSON lines)                 │ VICI (Unix socket, 0600)
                 ▼                                             ▼
  ipsec-client-helper (root) ──systemd-run──► embedded charon (strongSwan 6.1.0 + patch)
    sandboxed transient unit, entrypoint.sh → ipsc0 (XFRM), forced updown → routes + DNS

crates/cli (ipsec-client-cli): the same logic, without a GUI (English only)
```

## Names and locations

| Item | Value |
|---|---|
| XFRM interface | `ipsc0`, MTU 1400, `if_id` derived from the profile (≠ 0) |
| Helper socket | `/run/ipsec-client-helper.sock` (0666; the helper authorizes each request) |
| VICI socket | `/run/ipsec-client/<uid>/charon.vici` (directory 0711 root, socket 0600 owned by the uid) |
| Engine unit | `ipsec-client-engine` (transient, `systemd-run`) |
| Installed component | `/opt/ipsec-client/{engine,libexec,bin,VERSION,SHA256SUMS,ORIGIN}` |
| Profiles | `~/.config/ipsec-client/profiles/<id>.json` |
| Keyring | service `ipsec-client`, key `<profile>:<field>` |

## Embedded engine (ADRs 0013–0016, 0019)

strongSwan **6.1.0** is compiled from the official source (`native-engine/`),
with the IPsec Client security patch (`native-engine/patches/`), and runs on
the host, started by the privileged helper.

### Network component: `ipsec-client-helper` (ADR-0014)

| Piece | What it does |
|---|---|
| `crates/proto` | JSON-per-line protocol (`ping`, `start`, `stop`, `status`, `logs`) and the **validation rules shared** with `core` (interface, `if_id`, MTU, log level, profile id, DNS). Unknown fields are refused, so the client cannot pass paths or commands |
| `crates/helper` | Root, socket activated. Uses `SO_PEERCRED` and asks polkit (`io.github.lucastelso.ipsecclient.connect`, `allow_active=yes`, non-interactive). Creates `/run/ipsec-client/<uid>` (0711 root:root) and starts the engine with `systemd-run --unit=ipsec-client-engine` and a sandbox, passing the profile DNS (validated). Waits until the VICI socket is handed over to the user (0600, owned by the uid) and replies with its path. Refuses when the engine belongs to another user; `stop` and `logs` only for the owner. Serves up to 8 connections in parallel (ADR-0019). Messages in English |
| Engine sandbox | Capabilities `NET_ADMIN`, `NET_BIND_SERVICE`, `NET_RAW` and `CHOWN`; `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `PrivateDevices`, `PrivateIPC`, `ProtectKernelModules`, `ProtectKernelTunables`, `ProtectKernelLogs`, `ProtectProc=invisible`, `SystemCallFilter=@system-service`, `RestrictAddressFamilies` (unix, inet, inet6, netlink), `MemoryDenyWriteExecute`, `UMask=0077`; only `RuntimeDirectory` and the user's socket directory are writable |
| `packaging/` | Units `ipsec-client-helper.{socket,service}` (the helper is hardened too), the polkit policy and `install-native.sh`. The installer takes a bundle (`--bundle`), checks `SHA256SUMS` on a root-only copy, installs into `/opt/ipsec-client`, records `VERSION`/`SHA256SUMS`/`ORIGIN` and runs `restorecon` for SELinux. Its lines are tagged `[network component]` |
| `core::component` (ADR-0018) | Component bundle (`scripts/stage-network-component.sh`, shipped in `<resources>/network` in release builds; the helper goes compressed, because the AppImage's linuxdeploy rewrites ELF executables), state (not installed, ready, outdated or broken), installation via `pkexec` from a private copy in `$XDG_RUNTIME_DIR` and removal through root's installer |
| `core::engine::Backend` | `Native` (helper) when installed, or `Docker` as a fallback. `IPSEC_CLIENT_BACKEND=native\|docker` forces one. An old helper that refuses the new request becomes `EngineError::HelperOutdated` ("update the network component") |

### Engine scripts (`engine/`)

| File | Role |
|---|---|
| `entrypoint.sh` | Validates the environment; cleans leftovers; recreates `ipsc0`; writes the runtime config; starts charon; hands the VICI socket over (`chmod 0600` then `chown <uid>`); on `SIGTERM`, stops charon and cleans the interface, routes, DNS and XFRM state with the `if_id`. With `IPSEC_CLIENT_UPDOWN` + `IPSEC_CLIENT_DNS_*`, writes `updown.args` (one argument per line, root only) and the `force_updown`/`force_if_id` options of the strongSwan patch |
| `updown.sh` | Called by charon for each TS going up or down; installs/removes routes on `ipsc0` (`proto 241`) and configures split DNS with `resolvectl`. In the embedded engine it runs as `updown --args-file <file>`; it validates the arguments and the values from charon (`PLUTO_*`) again |
| `strongswan.d/zz-ipsec-client.conf` | `install_routes = no`, `resolve` plugin off, `updown.dns_handler = yes` (passes the received DNS servers to the updown), log to stderr (level 1) |

### Engine environment (from the helper)

All values are **non-sensitive**: secrets never go through here.

| Variable | Example | Validation |
|---|---|---|
| `IPSEC_CLIENT_IFACE` | `ipsc0` | `^[a-z][a-z0-9_-]{0,14}$` |
| `IPSEC_CLIENT_IF_ID` | `4096` | integer 1–4294967295 |
| `IPSEC_CLIENT_MTU` | `1400` | integer 576–9000 |
| `IPSEC_CLIENT_SOCKET_UID` | `1000` | integer ≥ 0 (socket owner) |
| `IPSEC_CLIENT_LOG_LEVEL` | `1` | integer 0–2 (never 3+, which may show sensitive material) |
| `IPSEC_CLIENT_UPDOWN` | `/opt/ipsec-client/libexec/updown` | absolute path, executable |
| `IPSEC_CLIENT_DNS_SERVERS` / `_DOMAINS` / `_FLAGS` | `192.0.2.53` / `~corp.example.com` / `use-received` | IPs, domains, `use-received`/`default-route`, comma separated |

### Engine contract for the app

- **Readiness.** The helper replies once the VICI socket is handed over to the
  user (owner = uid, 0600). In the Docker engine, the app waits for the
  `[engine] ready` log line **and** a successful connection to the socket. A
  socket file alone does **not** mean readiness: an engine killed with
  `SIGKILL` leaves the old socket behind.
- **Shutdown.** `systemctl stop` (or `docker stop`) sends SIGTERM. The
  entrypoint stops charon, which sends DELETE to the gateway, and removes the
  interface, the `proto 241` routes, the DNS, the XFRM SAs/policies with the
  `if_id` and the socket.
- **Abrupt death (SIGKILL).** The next engine removes the old interface, the
  routes, the DNS and the SAs/policies with the `if_id`.
- **Logs.** charon runs at level 1 and does not log the PSK or the password
  (asserted in the E2E). It **logs the XAuth username**, so the app masks the
  known identities before showing the log (ADR-0007). "Copy anonymized" also
  replaces the gateway, IPs and domains.

## Docker engine (tests and development, ADR-0001)

The same `engine/` scripts in an `ubuntu:20.04` image with strongSwan 5.8.2
(`engine/Dockerfile`), started with `--network host` and only `NET_ADMIN`
(ADR-0005). It is the engine of the test gateway and the fallback when the
network component is not installed. Its VICI socket is in
`$XDG_RUNTIME_DIR/ipsec-client/charon.vici` (0660, the user's group).

## Rust modules

| Module | What it does |
|---|---|
| `core::vici` (ADR-0011) | `Message`/`Value` (ordered, values in buffers wiped on drop, no values in `Debug`, maximum depth 32); `ViciClient` (commands, streamed commands such as `list-sas`); `ViciEvents` (`ike-updown`, `child-updown`, `log`); the `Charon` trait; `SharedSecret` |
| `core::profiles` | `Profile` (serde, `deny_unknown_fields`, no credentials; format in [profiles.md](profiles.md)); `validate` (returns **all** problems, field by field); `to_load_conn` (the XAuth username comes from `ConnParams`, never from the profile; no `updown` for the embedded engine); `if_id` (SHA-256 of the id → 1..=2³¹-1); `ProfileStore` (atomic saves, 0600 in a 0700 directory, 1 MiB import limit); `import_swanctl` |
| `core::secrets` (ADR-0007) | `Credentials` (`SecretString`, masked `Debug`); `KeyringStore` (Secret Service via libdbus); `MemoryStore`; `Redactor`/`mask` (identities in log lines); `Anonymizer` (identities, gateway, profile DNS/domains and every IP become stable placeholders) |
| `core::diagnostics` | Checks without root, each with a stable `id`, a name in the current language and a hint: user, network component (or Docker), `xfrm_interface`, resolved, D-Bus, ports 500/4500, keyring, image |
| `core::session` | `connect` (credentials → active connection? → preflight → engine → VICI events → `establish` → `list-sas`; on any failure, stops the engine), `diagnose` (likely cause from real charon lines), `status`/`disconnect` (work from any process), `anonymizer_for`, `SessionEvent` |
| `core::i18n` | `Lang` (English by default, Portuguese), `set_lang`, `Lang::from_env`, and the `tr!`/`trf!` macros used by every user-facing message in `core` and `src-tauri` |

## Desktop app (`src-tauri` + `src`)

| Layer | What it does |
|---|---|
| `src-tauri/src/commands.rs` | Thin `async` commands over the core. Disk and keyring go to `spawn_blocking`, and the `ConnectionLock` prevents two simultaneous `connect`s. `set_language` sets the language of the Rust messages; `anonymize_log` returns the text to copy |
| `src-tauri/src/dto.rs` | The UI contract: `ProfileSummary`, `CredentialsStatus` (presence and masked username only), `CredentialsInput` (secrets only come in, redacted `Debug`), `Connection`, `SessionUpdate` (event), `ComponentInfo` and `UiError` (kind + message + details) |
| `src-tauri/src/desktop.rs` | Tray (`tray-icon`), a state monitor every 3 s with notifications on transitions, single instance through a socket in `$XDG_RUNTIME_DIR`. Menu texts follow the language |
| `src/bindings.ts` | Generated by tauri-specta (`Result` mode: typed errors). A Rust test keeps it up to date |
| `src/i18n` | Typed dictionaries (`en`, `pt` with the same shape), `I18nProvider`, `useT`/`useLanguage`. The language follows the system (`pt*` → Portuguese) until the user chooses; the choice is stored locally and sent to Rust |
| `src/features/` | `connection` (reducer, hooks, home screen, log panel with "Copy anonymized"), `profiles` (list, import dialog, export/delete), `component` (card, first-use banner), `system` (diagnostics, appearance, language) |

## Connection flow (`core::session`)

1. **Preflight:** network component (or Docker), free UDP 500/4500,
   systemd-resolved, complete credentials (keyring or dialog).
2. **Engine:** the helper starts the sandboxed unit with the profile DNS; the
   entrypoint recreates `ipsc0`, writes the forced `updown`/`if_id` and hands
   the socket over.
3. **Load** via VICI: the app subscribes to the events and sends
   `load-shared` (PSK, XAuth) → `load-conn` (without `updown`) → `initiate`.
   The control log shows up live.
4. **Connected:** the `child-updown up` event leads to *Connected*, with VIP,
   TS and connection time; a periodic `list-sas` updates the traffic.
5. **Disconnect:** `terminate` → stop the unit. On start, the app re-attaches
   to an engine that is already running.
6. **Clear errors:** `FailureKind` explains the likely cause (PSK or group id,
   XAuth credentials, proposals, traffic selectors, unreachable gateway).

### State machine

```
Disconnected ─connect─► Preparing (preflight, engine)
     ▲                       │ ok
     │                       ▼
     │                  Negotiating (load-shared/load-conn/initiate)
     │                       │ child-updown up
     │                       ▼
     ├────disconnect──── Connected ◄──── (rekey / DPD restart)
     │                       │ failure / child-updown down without recovery
     └────────────────── Error (message + log)
```

## End-to-end tests

- [`tests/engine/e2e.sh`](../tests/engine/e2e.sh): a fake gateway (IKEv1
  aggressive, PSK + XAuth, modp1024/768, network `192.0.2.0/24`, VIPs from
  `198.51.100.0/24`, DNS `192.0.2.53`) and the engine as the app starts it, on
  the `ipsct0` interface with its own `if_id`. Scenarios: normal cycle (XFRM,
  MTU, VIP, route, split DNS, socket, no secrets in logs, ping, full cleanup),
  `SIGKILL` recovery, full tunnel in an isolated namespace, modp768 PFS and,
  for the embedded engine, the ADR-0019 attack.
- [`tests/systemd/run.sh`](../tests/systemd/run.sh): a Fedora container with
  real systemd; the real installer, the CLI through the helper, the applied
  sandbox, two users and the attack.
- [`tests/cli/e2e.sh`](../tests/cli/e2e.sh): the full CLI flow, including
  `logs --anonymize`.
- [`tests/packaging/install-test.sh`](../tests/packaging/install-test.sh): the
  installer as root, including tampered bundles.

No test uses a real VPN. IPv6 in `updown` has no automated test yet.
