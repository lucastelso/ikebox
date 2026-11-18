# ADR-0016: strongSwan 6.1.0 in the embedded engine, with 5.8.2 as a fallback

- **Status:** Accepted. Since ADR-0019, the build also applies the IPsec Client
  security patch, which has not been ported to the 5.x fallbacks. How new
  releases and the removal of IKEv1 upstream are handled: ADR-0026.
- **Date:** 2026-11-16

## Context

The maintainer asked for "the newest version that works". The spike compiled
and tested four versions with the same configuration: 6.1.0 (current), 6.0.7,
5.9.14 and 5.8.2 (the version validated in the manual flow).

All of them passed the **26 E2E checks** against the test gateway (which runs
Ubuntu's 5.8.2, like legacy VPNs). 6.1.0 logged no weak or deprecated
algorithm warnings.

The covered scenarios were:

- IKEv1 aggressive with group PSK + XAuth and modp1024;
- PFS with modp768 only in phase 2;
- XFRM interface and a VICI socket reachable by the user;
- split DNS;
- full tunnel;
- recovery after `SIGKILL`.

## Decision

- The embedded engine uses **strongSwan 6.1.0**.
- `native-engine/versions.txt` keeps 6.0.7, 5.9.14 and **5.8.2** buildable and
  testable (`native-engine/spike/run-e2e.sh <version>`). 5.8.2 is the fallback
  if a user's real VPN shows a difference in behavior.
- Each version is pinned by SHA-256 and checked against the official GPG
  signature. The public key lives in `native-engine/keys/` (fingerprint
  `948F158A4E76A27BF3D07532DF42C170B34DBA77`).
- strongSwan updates come in their own PR, with the full E2E green.

## Consequences

- The app gets strongSwan's security fixes since 2020.
- The behavior with real gateways may differ from 5.8.2. That is mitigated by
  the E2E and by validating on the maintainer's VPN before a release. If
  something fails, going back to 5.8.2 needs no code change (but the security
  patch must be ported first).
- Without OpenSSL, there is no ECP (ADR-0013). The example IKEv2 preset uses
  `curve25519`.
