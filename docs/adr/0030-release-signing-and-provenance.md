# ADR-0030: Releases built in CI, signed with GPG, attested by GitHub

- **Status:** Accepted. Phase G of the program to 1.0.0 (step 57). Closes the
  roadmap item `chore/release` and audit items P16 and P17.
- **Date:** 2026-11-18

## Context

Until 0.9.0 the maintainer built the packages on their computer and uploaded
them by hand; `SHA256SUMS` was signed only optionally, the `.rpm` not at all
(openSUSE's zypper refuses it without a flag, audit P17), and the
compatibility matrix ran only by hand (P16). For a public 1.0, a downloader
must be able to check what they got, and the build should not depend on one
computer. The maintainer chose: CI builds on the tag; a GPG signing subkey in
a protected environment signs the `.rpm` and `SHA256SUMS`; GitHub's build
provenance attestation says which commit and workflow made each file; the
maintainer builds locally too and compares.

## Decision

- **`release.yml`**, on a `vX.Y.Z` tag:
  1. **build** (no secrets): `scripts/build-release.sh` (the same as by
     hand: verification and the private data scan for the home path and the
     hostname), `scripts/check-reproducible.sh`, and the engine's
     corresponding source (`scripts/release-sources.sh`);
  2. **sign**, in the `release` environment, which needs the maintainer's
     approval and is the only place holding the signing subkey and its
     passphrase: `scripts/sign-release.sh` signs the `.rpm` (rpmsign) and
     `SHA256SUMS` (detached, armored) and checks both against the public key
     in `packaging/keys/`, as a downloader would;
  3. **publish**: provenance attestations of every file (when the
     repository is public: they are free there) and a **draft** release
     with the notes of `docs/releases/`.
  The maintainer then downloads the draft, runs `scripts/check-private.sh
  --artifacts` with the private patterns that never leave their computer
  (ADR-0010), compares the network component with a local build, and
  publishes. `workflow_dispatch` runs a rehearsal from any commit (build and
  sign, nothing published).
- **The key:** an Ed25519 primary key for certification only, kept off-line
  by the maintainer, and an Ed25519 signing subkey valid for two years,
  the only part given to GitHub. A leaked subkey is revoked with the primary
  key and replaced without changing the published key's fingerprint.
  Ed25519 `.rpm` signatures were checked with the `rpm` of Fedora 45,
  openSUSE Tumbleweed and AlmaLinux 10.
- **Reproducible network component:** same commit, same `SHA256SUMS` (the
  app's way of telling whether the installed component is current), checked
  on every release and weekly. It already was: the engine and the helper
  came out byte for byte the same in two clean builds.
- **`weekly.yml`:** the upstream watch (`scripts/upstream-watch.sh`: a newer
  strongSwan, or new OpenSSL/musl revisions compared with the latest
  release, each opens an issue), a fresh advisory database, reproducibility,
  `test:compat` and `tests/compat/packages.sh` updated from the latest
  release (P16).
- `verify-release.sh` also checks that the three package names and the
  component carry the same version.

## Consequences

- Downloads can be checked two ways: `gpg --verify SHA256SUMS.asc` with the
  project key, and `gh attestation verify <file> --repo
  lucastelso/ipsec-client` (installation guide). `dnf` and `zypper` check
  the `.rpm` once the key is imported.
- The `.deb` and the AppImage are covered by `SHA256SUMS.asc` and the
  attestation, not by signatures of their own (Debian's package signatures
  are rarely checked; there is no APT repository).
- A release needs the maintainer twice: approving the signature, and
  publishing the draft after the local checks.
- Attestations need a public repository; the rehearsals while it is private
  build and sign only.

## Alternatives considered

- **Signing on the maintainer's computer only:** no key in CI, but the
  release would still depend on one machine and a manual upload.
- **Sigstore only (no GPG):** no key to keep, but `dnf`/`zypper` check GPG
  signatures, not Sigstore bundles.
- **Signing the AppImage and the `.deb` themselves:** tools exist, but
  nothing on the user's side checks them by default.
