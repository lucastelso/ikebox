# ADR-0005: Least privilege in the container

- **Status:** Accepted. In the embedded engine, the sandbox comes from systemd (ADR-0014, ADR-0019).
- **Date:** 2026-11-15

## Context

The manual flow ran the container with `--privileged`, `--cap-add SYS_MODULE`
and `/lib/modules` mounted. With that, the container had full access to the
host: devices, kernel modules and every capability.

## Decision

- The container gets only what it needs:
  - `--network host`, because XFRM and the routes must be in the host
    namespace;
  - `--cap-add NET_ADMIN`, for XFRM netlink, routes, addresses and interfaces;
  - Docker's default capabilities, which include `NET_BIND_SERVICE` for ports
    500/4500;
  - a bind of `/run/dbus/system_bus_socket`, so `resolvectl` can talk to the
    host's systemd-resolved;
  - a bind of the VICI socket directory (`$XDG_RUNTIME_DIR/ipsec-client`).
- `--security-opt label=disable` only when the host uses SELinux, and
  `apparmor=unconfined` only when it uses AppArmor. Without it, access to the
  D-Bus socket and to the VICI socket bind is blocked.
- **No** `SYS_MODULE` and no `/lib/modules`. The kernel loads the
  `xfrm_interface` module automatically when the interface is created.
- There is a "privileged mode" (`--privileged`) only as a fallback, off by
  default. It shows up in the advanced settings, with a warning.

## Consequences

- If charon is compromised, the damage is limited to the host network. The
  attacker gets no access to disks, devices or modules.
- On hosts with different security policies, the fallback mode may be
  needed. The diagnostics help to identify those cases.

## Alternatives considered

- **Always `--privileged` (manual flow).** Unnecessary attack surface.
- **User namespaces / rootless.** XFRM in the host namespace requires
  `CAP_NET_ADMIN` in the initial namespace.
