# ADR-0011: Our own VICI client instead of `rsvici`

- **Status:** Accepted (details the library choice left open in ADR-0003)
- **Date:** 2026-11-16

## Context

ADR-0003 chose VICI to control charon and named `rsvici` as the candidate,
with the caveat that our own implementation would be considered if it did not
fit. Evaluation of `rsvici` 0.1.5:

- about 1,100 lines, pre-1.0, depends on `serde_vici` and three more stream
  crates;
- it serializes messages via serde into regular buffers, which are not wiped.
  `load-shared` carries the PSK and the XAuth password, and ADR-0007 requires
  zeroization;
- it includes a TCP transport, which the app does not use (Unix socket only).

The protocol is small: length-prefixed packets, 8 packet types and 6 message
element types. The engine test already had a minimal Python client
(`tests/engine/vici_version.py`).

## Decision

Implement the client in `crates/core/src/vici/`:

- `message`: ordered model (`Message`/`Value`) with encoding and decoding.
  **Every value lives in `Zeroizing<Vec<u8>>`**. `Debug` never shows values.
  Decoding limits the depth (32) and validates the sizes;
- `client`: Unix socket transport, with packets of at most 512 KiB, the same
  limit as charon. Send buffers are wiped. There are two connections:
  `ViciClient` for commands, including streamed ones such as `list-sas`, and
  `ViciEvents` for subscribed events;
- `charon`: the `Charon` trait, with the typed operations the app uses and
  mockable in the session, the reply types without identities (`IkeSa`,
  `ChildSa`) and the event parsing (`CharonEvent`). `SharedSecret` has a
  redacted `Debug`, including the owners, which are identities.

## Consequences

- Secrets only go through wiped buffers, from the UI to the socket.
- No new dependency besides `zeroize`, which was already planned for
  ADR-0007.
- We maintain the protocol. The risk is low: VICI has been stable since 5.2,
  and there are unit tests (wire format, malformed input, fake server) and an
  integration test with a real IPsec tunnel against the test gateway.

## Alternatives considered

- **`rsvici`**: discarded for the reasons above.
- **`swanctl` via `docker exec`**: discarded in ADR-0003.
