# ADR-0014: Privileged helper, authorized once (polkit + systemd)

- **Status:** Accepted. Implemented in `crates/helper`, `crates/proto` and
  `packaging/` (branch `feat/helper`) and validated as root (step 27). The
  installation by the app is detailed in ADR-0018. ADR-0019 (0.2.0) closes a
  privilege escalation through VICI (an `updown` chosen by the client),
  changes the socket to `0600` of the uid and restricts `stop`/`logs` to the
  owner.
- **Date:** 2026-11-16

## Context

The embedded charon (ADR-0013) needs `CAP_NET_ADMIN` in the host network
namespace (XFRM, routes, interface), UDP ports 500/4500 and access to the
system D-Bus (resolvectl). The app runs as a regular user.

The maintainer's decision: the administrator permission is asked **once, at
installation**. After that, connecting and disconnecting do not ask for a
password, as with NetworkManager.

## Decision

- **`ipsec-client-helper`** (crate `crates/helper`, Rust):
  - a socket-activated systemd service (`/run/ipsec-client-helper.sock`);
  - runs as root and exposes a **minimal** API: `start` (with the parameters
    validated by the same rules as the core), `stop`, `status` and `logs`;
  - never receives secrets: they go from the app straight to charon, through
    the user's VICI socket.
- **Authorization** on every request:
  - the client uid via `SO_PEERCRED`;
  - a polkit check of the `io.github.lucastelso.ipsecclient.connect` action
    with `allow_active=yes`: the user of the active local session connects
    without a password, and remote or inactive sessions need an administrator.
- **charon sandbox:** the helper starts it with `systemd-run` as a transient
  unit, with:
  - a `CapabilityBoundingSet` with only `CAP_NET_ADMIN`,
    `CAP_NET_BIND_SERVICE`, `CAP_NET_RAW` and `CAP_CHOWN`;
  - `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`,
    `NoNewPrivileges=yes` (ADR-0019 adds `PrivateDevices`,
    `ProtectKernelTunables`, `ProtectKernelLogs`, `ProtectProc`, `PrivateIPC`
    and a syscall filter);
  - `ReadWritePaths` only on the socket directory and the runtime directory;
  - `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK`.
- **Installation:**
  - in the **rpm/deb** packages, the component is bundled in the app;
  - in the **AppImage**, the first use offers "Install network component". The
    system asks for the administrator password once (`pkexec`), and the
    installer copies the helper and the engine to `/opt/ipsec-client`, checks
    the hashes, installs the unit and the policy and enables the socket. There
    is also an uninstall (ADR-0018).

## Consequences

- The user authorizes once, and the day to day has no password.
- The code that runs as root lives in a small binary, with tests of the
  request validation. charon runs isolated by systemd.
- The AppImage needs the component installation step, because root cannot
  read the user's AppImage FUSE mount point.

## Alternatives considered

- **`pkexec` on every connection.** Asks for a password every day. Rejected
  by the maintainer.
- **`setcap` on charon.** The capabilities would sit on a binary any user can
  run, without per-session authorization.
- **Our own daemon without systemd.** It reinvents activation, sandboxing and
  logs.
