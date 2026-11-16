# ADR-0006: Engine image embedded and versioned by hash

- **Status:** Accepted for the Docker backend. The embedded engine is identified by the tarball hash (ADR-0013).
- **Date:** 2026-11-15

## Context

End users should not need to clone the repository or run `docker build` to
use the app. At the same time, an old image with outdated scripts would cause
bugs that are hard to diagnose.

## Decision

- The `engine/` directory (Dockerfile and scripts) is embedded in the binary
  with `include_bytes!`.
- The image name is `ipsec-client-engine:5.8.2-<hash8>`, where `hash8` is the
  first 8 characters of the SHA-256 of the content of `engine/`. The image also
  gets a label with the full hash.
- When connecting, if no image with that name exists, the app builds the tar
  context in memory and builds the image through the Docker API (bollard),
  showing the progress in the UI.
- Old images (`ipsec-client-engine:*` with another hash) can be removed by the
  diagnostics.

## Consequences

- The binary and the engine always go together. There is no "outdated image".
- The first use needs the network to download the base and the packages, and
  takes about 1 minute.
- No registry is needed. Publishing the image to a registry is a future
  optimization.

## Alternatives considered

- **Publishing the image to a registry (GHCR).** It needs publishing and
  signing infrastructure. It is an optimization that can come later.
- **A fixed tag (`:latest`).** It does not detect a mismatch between the app
  and the engine.
