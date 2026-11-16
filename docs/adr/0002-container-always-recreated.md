# ADR-0002: Container recreated on every connection

- **Status:** Accepted. In the embedded engine, it becomes "process recreated" (ADR-0013).
- **Date:** 2026-11-15

## Context

In the validated flow, reusing a stopped container with `docker start` left
old state behind (pidfiles, sockets, SAs). This caused intermittent failures
on reconnection.

## Decision

- Every connection runs `docker rm -f` on the previous container and creates a
  new one (`create` + `start`). The app never runs `docker start` on an
  existing container.
- The entrypoint also deletes old sockets and pidfiles and recreates the XFRM
  interface, as an extra guarantee.
- On disconnect: `terminate` via VICI, `stop` (10 s) and `rm`.

## Consequences

- The state is always clean and predictable.
- strongSwan only listens on UDP ports 500/4500 while the VPN is connected.
- Creating the container takes less than 1 s, which is negligible next to the
  IKE negotiation.
- Logs from previous sessions do not stay in the container. The app keeps the
  logs of the current session in memory.

## Alternatives considered

- **A persistent container with `--restart`.** It leaves the IKE daemon
  exposed all the time and accumulates state.
- **`docker start` on a stopped container.** That is the behavior that caused
  the failures in the manual flow.
