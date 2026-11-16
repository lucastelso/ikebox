# ADR-0007: Secrets only in memory or in the system keyring

- **Status:** Accepted. The implementation is described in docs/architecture.md
  (`core::secrets`).
- **Date:** 2026-11-15

## Context

A typical legacy VPN has three secrets:

- the group PSK, shared by the team;
- the XAuth username;
- the XAuth password, personal.

In the manual flow, all three were in plain text in a `swanctl.conf`.

## Decision

- **In memory**, secrets use `secrecy`/`zeroize`, which wipe the memory on
  drop. `Debug` and `Display` are redacted (`***`) on every type that holds a
  secret.
- **Optional persistence** ("remember credentials"): the `keyring` crate, with
  the Secret Service backend (GNOME Keyring/KWallet). The service is
  `ipsec-client` and the key is `<profile>:<field>`.
- Secrets **never** go to:
  - profile files (JSON);
  - environment variables or container arguments;
  - `docker inspect`;
  - app or charon logs. charon's log level stays ≤ 2, and at those levels it
    does not print secrets.
- The XAuth username is masked in every output. Example: `ab***yz`.
- A profile imported with a pre-filled PSK: the PSK goes to the keyring and the
  saved JSON does not have it.

## Consequences

- Without "remember", the user types the credentials on every connection.
- Without Secret Service (e.g. a session without a keyring), "remember" is
  disabled and the diagnostics explain why.

## Alternatives considered

- **Our own encrypted file.** It reinvents the keyring and creates the problem
  of where to keep the master key.
- **Container environment variables.** They show up in `docker inspect` and in
  `/proc/<pid>/environ`.
