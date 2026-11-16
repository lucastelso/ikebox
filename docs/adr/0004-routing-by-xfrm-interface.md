# ADR-0004: Routing and DNS on a dedicated XFRM interface

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

The manual flow did three things that touched the host more than needed:

- it put the virtual IP (VIP) and the VPN route **on the physical interface**
  (Wi-Fi/Ethernet), which NetworkManager manages;
- it configured the DNS on that same interface;
- on cleanup, it ran **global** `ip xfrm state flush` and `ip xfrm policy
  flush`.

This conflicted with NetworkManager and wiped any other IPsec on the host. It
also needed fragile logic to find and remove "leftover" VIPs.

Since strongSwan 5.8.0 and Linux 4.19, there are **XFRM interfaces**. They
are virtual network interfaces bound to SAs and policies marked with an
`if_id`.

## Decision

- The entrypoint creates the `ipsc0` interface (`type xfrm if_id <N>`), with
  MTU 1400.
- The `if_id` is derived from the profile and is never 0.
- The connection's SAs and policies get `if_id_in`/`if_id_out = N`.
- charon configuration:
  - `install_virtual_ip_on = ipsc0`: the VIP lives on the dedicated interface;
  - `install_routes = no`: charon installs no routes;
  - `resolve` plugin off.
- `updown.sh` runs as root in the host network namespace:
  - **routes:** `ip route replace <remote TS> dev ipsc0 src <VIP>`;
  - **split DNS:** `resolvectl dns|domain|default-route ipsc0 …`, through the
    host D-Bus. `~domain` domains only route queries, they do not join the
    search list;
  - **full tunnel** (`0.0.0.0/0`): routes `0.0.0.0/1` + `128.0.0.0/1`, plus a
    host route to the gateway through the original path, so there is no loop.
- The `updown` arguments are validated in Rust and again in the script.
- **Teardown:** charon removes SAs and the VIP, and the entrypoint runs
  `resolvectl revert ipsc0` + `ip link del ipsc0`. Routes, VIP and DNS go
  away with the interface.

## Consequences

- The physical interface and NetworkManager are not touched.
- The MTU of 1400 on the interface avoids the "blackhole" of large packets on
  Wi-Fi.
- No global flush: other VPNs (WireGuard, Tailscale, other IPsec) are not
  affected.
- Cleanup is atomic: deleting the interface undoes everything.
- It requires kernel ≥ 4.19 with the `xfrm_interface` module. The app's
  diagnostics check that.
- It is a change from the validated flow and needs an end-to-end test. The
  repository has a test with a local IPsec gateway (`tests/engine/e2e.sh`).

## Alternatives considered

- **VIP and routes on the physical interface (manual flow).** It conflicts
  with NetworkManager and needs a global flush.
- **charon's table 220 (`install_routes = yes`).** It works, but spreads
  routes and policy rules over the host, and the DNS would still be on the
  physical interface.
- **VTI interfaces.** Older, they need marks (`mark`) and are less flexible
  than XFRM.
