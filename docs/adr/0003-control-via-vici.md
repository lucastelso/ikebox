# ADR-0003: charon controlled via VICI

- **Status:** Accepted. The library choice is detailed in ADR-0011. ADR-0019
  changes the socket permissions (`0600`, owned by the uid) and fixes the
  `updown`/`if_id` on the root side.
- **Date:** 2026-11-15

## Context

The manual flow built a `swanctl.conf` with the secrets in plain text and ran
`docker exec swanctl --load-all`. The secrets stayed on disk, and the result
was interpreted by `grep` on the text output.

charon exposes **VICI**, a stable binary protocol over a Unix socket.
`swanctl` also uses VICI under the hood.

## Decision

- The app talks VICI directly to charon, over a Unix socket that is a bind
  mount of `$XDG_RUNTIME_DIR/ipsec-client/charon.vici`.
- The entrypoint sets the socket to `root:<user gid>` and mode `0660`, so only
  the user who started the session can connect.
- The VICI client sits behind a `Charon` trait, which can be replaced by a
  mock in the tests.
  - One connection is used for commands.
  - Another connection is used for events (`ike-updown`, `child-updown`,
    `log`).
- Operations used: `version`, `load-shared`, `load-conn`, `initiate`,
  `terminate`, `list-sas`, `unload-conn`.
- The candidate library is `rsvici` (MIT, async/tokio), with a pinned version.
  If it does not fit, the protocol is simple enough for our own auditable
  implementation. That switch would be recorded in a new ADR.

## Consequences

- Secrets go straight from the app's memory to charon's memory.
- Errors arrive structured, without parsing text.
- The control log shows up live in the UI.
- The app depends on the VICI protocol of 5.8.x, which has been stable since
  5.2.

## Alternatives considered

- **`docker exec swanctl`.** Secrets would have to go to a file or to
  arguments (visible in `ps`), and the replies would be text to parse.
- **VICI socket over TCP.** It exposes the control to any local process,
  without access control through file permissions.
