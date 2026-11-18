# ADR-0026: strongSwan maintenance policy, with IKEv1 kept working

- **Status:** Accepted. Phase C of the program to 1.0.0 (step 52); closes
  audit P1. Amends ADR-0016.
- **Date:** 2026-11-18

## Context

strongSwan 6.1.0 disables IKEv1 by default (the embedded engine builds it
with `--enable-ikev1`) and announces its removal "in a future release". Most
VPNs this app exists for are IKEv1 (aggressive or main mode, group PSK +
XAuth). The engine is compiled by the project, so its security fixes reach
users only when the project ships them (SECURITY.md, risk 2), and since
phase C the engine also links OpenSSL (ADR-0027), which has advisories of
its own.

## Decision

1. **Follow strongSwan's current line while it carries IKEv1.** Every new
   strongSwan release (and every security advisory) is evaluated: the
   update goes in its own branch, with the full E2E (`test:native`,
   `test:systemd`, `test:modes`) green, and a release follows.
2. **When upstream removes IKEv1**, the engine stays on the last release
   with IKEv1, and the project applies upstream's security fixes to it:
   strongSwan publishes a patch per CVE for older releases, in the same
   place as its advisories. Each one goes into `native-engine/patches/`
   with its source and the CVE in the file name, is listed in `BUILDINFO`
   like the project's own patches, and gets a release. If a fix cannot be
   backported, the decision is recorded in a new ADR before the release.
3. **OpenSSL** (ADR-0027) follows the version of the engine builder's
   Alpine release: an OpenSSL advisory that affects the code paths charon
   uses (RSA, ECDSA, DH, the hashes and ciphers it does not take from
   strongSwan's own plugins) means a rebuild of the engine and a release.
   musl is followed the same way.
4. **Watching upstream:** the weekly CI of phase G checks the latest
   strongSwan and the Alpine OpenSSL/musl versions against the ones pinned
   in the repository and opens an issue when they differ; until then, the
   maintainer checks strongSwan's security page and the OpenSSL advisories
   before each release.
5. **Supported versions** (SECURITY.md): only the latest release gets
   fixes; users update the app (and, with the AppImage, the network
   component).

## Consequences

- IKEv1 keeps working after upstream drops it, at the cost of carrying
  backported patches. Their number is visible in `native-engine/patches/`.
- An unmaintained fork is avoided: the engine keeps upstream's code, plus
  small, listed patches.
- Releases may be driven by upstream advisories, not only by features.

## Alternatives considered

- **Moving to libreswan for IKEv1:** another daemon, another control
  protocol (no VICI), and losing the ADR-0019 patch and every test.
- **Dropping IKEv1 when upstream does:** the app would stop serving the VPNs
  it was made for.
- **Staying on 5.9.x:** older code, also out of upstream support; the
  ADR-0019 patch would need a port.
