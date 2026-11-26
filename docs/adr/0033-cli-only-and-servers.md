# ADR-0033: A CLI-only build, and an opt-in polkit rule for servers

- **Status:** Accepted. Phase 3 of the program to 1.1.0.
- **Date:** 2026-11-26

## Context

People on servers, under WSL or on minimal setups asked for Ikebox without
the desktop app: no Tauri, no Node, no WebKitGTK. The pieces already
allowed it (`ikebox-cli` and `ikebox-helper` do not depend on the app, and
since ADR-0032 the engine builds without Docker), but nothing said so, and
nothing checked that it stayed true.

The second obstacle was authorization. The polkit policy (ADR-0014) lets
only the user of the **active local session** control the VPN
(`allow_active = yes`, `allow_inactive = no`, `allow_any = no`). That is
right for a desktop, where it stops other local users and remote logins
from touching the VPN, but an SSH session, a WSL shell or a server without
a desktop is never "active local", so `ikebox-cli connect` was refused
there, with an English message that did not say what to do.

## Decision

- **A documented CLI-only build:** the engine (`native-engine/build.sh
  --host`, or the builder), `cargo build --release -p ikebox-cli -p
  ikebox-helper`, `scripts/stage-network-component.sh` and `ikebox-cli
  component install --bundle`. It needs Rust, a C toolchain with musl,
  `pkg-config` and the D-Bus headers: no Node, no WebKitGTK, no Docker. A
  CI job builds and tests the two crates in a container that has none of
  the GUI libraries, so a dependency on them would fail the build.
- **The default stays as it is.** The packages keep installing the same
  policy; nothing changes for desktops.
- **An optional rule, never installed:**
  `packaging/polkit/examples/50-ikebox-group.rules` (polkit 0.106 and newer)
  and `50-ikebox-group.pkla` (polkit 0.105, Ubuntu 22.04). An administrator
  who copies one lets the members of an `ikebox` group control the VPN from
  any session. What a member can do is exactly what the active local user
  can do today: start their own VPN, and stop or read the log only of a
  session they own (the helper checks the owner, ADR-0019 and audit part 2).
- **The refusal says what to do.** When polkit refuses, the CLI and the app
  say, in the person's language, that only the active local session may
  control the VPN and where the server instructions are.
- **Tested with the real polkit** of each distribution in `test:systemd`:
  with the rule, a group member without a session is authorized and a
  non-member is not; without it, the member is refused again.

## Consequences

- On a server, the administrator decides who may connect, with a group,
  instead of the desktop session deciding it. Members of the group can
  start a VPN without a password from an SSH session; that is the purpose,
  and the reason it is opt-in.
- Without a desktop there is usually no Secret Service keyring: the CLI asks
  the credentials at every connection, or reads them from
  `--credentials-stdin` (for example from a secrets manager); they still
  never reach the disk or the helper.
- WSL is not tested: it needs systemd enabled in WSL and a kernel with the
  `xfrm_interface` module; `ikebox-cli doctor` says what is missing.

## Alternatives considered

- **`allow_any = yes` or `auth_admin` for remote sessions in the shipped
  policy:** it would change every desktop installation, or ask for the
  administrator's password on every connection. Rejected.
- **A system service that connects at boot:** a different product (a site
  VPN), with credentials stored by root. Out of scope.
- **Checking the group in the helper itself:** a second authorization path
  next to polkit, with its own bugs. polkit already has groups.
