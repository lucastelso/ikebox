# ADR-0028: charon runs without root, with the network capabilities only

- **Status:** Accepted. Phase D of the program to 1.0.0 (step 53). Takes the
  "next step" of ADR-0019 and SECURITY.md (risk 1).
- **Date:** 2026-11-18

## Context

charon is the process that parses what arrives from the network (IKE from
the gateway, or from anyone who reaches UDP 500/4500 while connected) and
what the session user sends through VICI. Until 0.9.0 it ran as root inside
the sandbox of the engine unit (ADR-0014, ADR-0019): a memory-safety bug in
it would give root, limited by the sandbox. It needs only the network
capabilities. Two things kept it root: handing the VICI socket over to the
session user, and DNS. A probe on systemd 262 confirmed that
systemd-resolved does **not** accept `CAP_NET_ADMIN` for `SetLinkDNS` and
friends: an unprivileged caller goes to polkit and is refused.

## Decision

- The installer creates a **system user `ipsec-client-engine`** (no home,
  no shell; `systemd-sysusers`, or `useradd` where it is missing). It is
  kept on removal, as system users usually are.
- `entrypoint.sh` **stays root** inside the same sandbox, for what needs
  it: the XFRM interface, the runtime files, handing the socket over, DNS
  and cleanup. It starts charon with `setpriv` as `ipsec-client-engine`,
  with `NET_ADMIN`, `NET_BIND_SERVICE` and `NET_RAW` as ambient capabilities
  and the **bounding set reduced to those three**, so neither charon nor
  anything it starts can get any other. charon is built with
  `--with-capabilities=native` (it has to see its capabilities, or it turns
  its sockets off) and then drops what it no longer needs: once its sockets
  are open it keeps **only `NET_ADMIN`**, which the updown inherits.
- **VICI socket:** charon creates it in `/run/ipsec-client/<uid>`, which
  the entrypoint opens to charon's group for the session (`1731
  root:ipsec-client-engine`: the session user still only traverses it, and
  root's again afterwards). The socket is then made `0600` by its owner and
  handed to the session user by root, as before: the helper and the app see
  no difference.
- **DNS through root, on the session interface only.** The updown, without
  root, writes a request to a FIFO in the runtime directory: `up` with the
  servers the gateway sent, or `down`. The entrypoint reads at most 1 KiB
  per line, accepts only addresses, and applies them to `$IFACE` with
  `resolvectl`, together with the static servers, domains and flags from its
  own validated environment, never from the request. A polkit rule letting
  `ipsec-client-engine` set DNS was rejected: it would let a compromised
  charon change the DNS of any interface, the physical one included.
- The engine unit gains `CAP_SETUID`, `CAP_SETGID`, `CAP_SETPCAP` (to start
  charon so) and `CAP_KILL` (charon is now another user's process: to stop
  it gracefully and to see whether it lives). `systemd-analyze security`
  rates the unit 2.7 instead of 2.3 for that, while the process exposed to
  the network went from root to an unprivileged uid with `NET_ADMIN`.
- The Docker engine (development, tests) is unchanged: there charon is
  still root.

## Consequences

- A bug in charon no longer gives root: the attacker gets the
  `ipsec-client-engine` uid with `NET_ADMIN`, inside the sandbox (no
  `/home`, read-only system, no devices). `NET_ADMIN` still means the
  computer's network, which a VPN client needs; the DNS of other interfaces
  is out of reach.
- New boundary: `ipsec-client-engine` → root through the DNS FIFO. Covered
  by a `test:systemd` scenario (shell syntax, other interfaces, junk and an
  oversized line: nothing runs, only the session interface changes, the
  engine survives).
- The packages require `setpriv` (util-linux), present on every supported
  distribution; the installer refuses to continue without it.
- `test:systemd` checks the uid of charon and its effective, ambient and
  bounding capabilities; `test:modes` and `test:compat` pass with it.
- A new `test:modes` scenario has the gateway renew phase 1 (a full
  reauthentication in IKEv1) and phase 2 while connected: the client
  answers and keeps the tunnel. It informs audit P8 (`initiator_only` would
  drop exactly that), which still needs a long session on a real gateway.

## Alternatives considered

- **`DynamicUser=yes`:** the uid is only known once the unit starts, and the
  socket directory must be open to it before; the static user is simpler
  and visible in `ps`.
- **charon as the session user:** it could then be signalled (and its
  memory read by tools that run as that user, depending on ptrace rules)
  by any program of that user.
- **strongSwan's own `charon.user`:** it drops privileges after loading the
  plugins, but does not hand capabilities to the updown; `setpriv` with
  ambient capabilities does both.
