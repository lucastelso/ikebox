# ADR-0024: Client certificates: PKCS#12 import, key only in the keyring, sent as data

- **Status:** Accepted. Designed in phase 6 (step 43), implemented in phase 9
  (step 47, release 0.8.0) with the decisions taken at its start: IKEv2 and
  IKEv1 main mode (XAuth may follow the certificate in IKEv1), old `.p12`
  files (3DES/RC2, SHA-1) accepted, RSA and Ed25519 keys only.
- **Date:** 2026-11-17

## Context

Some companies authenticate each person with a certificate instead of (or
in addition to) a password, usually delivered as a PKCS#12 file (`.p12`,
`.pfx`) protected by a password. The private key is the most sensitive
credential the app would hold: whoever has it is that person on the VPN, for
years.

Constraints already decided: no secret on disk, in arguments, the
environment or logs (ADR-0007); charon (root) never reads a path chosen by
the client (ADR-0019, `0002` patch, audit P9).

## Decision

- **Import, in the app** (Rust, user process): the person picks the `.p12`
  and types its password. The app opens it in memory with a pure Rust
  parser, checks it (one private key, its certificate, key and certificate
  match, not expired, key usage allows signatures), shows the subject and
  expiry, and stores:
  - the **private key** in the keyring (`<profile>:client-key`, PKCS#8 PEM);
  - the **certificate** in the keyring too (`<profile>:client-cert`): it is
    not secret, but carries the person's name and stays out of the
    shareable profile.
  The `.p12` password is not stored. The original file is not copied; the
  app tells the person it can delete it.
- **Connecting:** the key goes to charon by VICI `load-key` (`type =
  any`) **as data** (the same memory → keyring → VICI path as the PSK,
  buffers zeroed on drop), and the certificate in the connection itself,
  `local-1 { auth = pubkey; certs = [<PEM>] }`, also as data (simpler than
  the `load-cert` of the design, like the gateway CAs of ADR-0023). The
  gateway side is always a certificate, with CA and `remote.id`
  (ADR-0022).
- **Algorithms:** the engine verifies and signs RSA (`gmp`) and Ed25519
  (`curve25519`) today. **ECDSA (P-256/P-384) needs a crypto library:**
  - **chosen:** start with RSA and Ed25519 (no new library), and refuse an
    ECDSA `.p12` at import with a clear message;
  - when a real gateway needs ECDSA, add the `openssl` plugin with a static
    libcrypto (musl, pinned, its version in `BUILDINFO`), with its own ADR:
    a larger engine (to be measured) and another library to follow for
    advisories, but also every curve and faster AES.
- **Masking:** the certificate subject, its SANs and the issuer join the log
  masking and "Copy anonymized".
- **Expiry:** the profile details and the connection screen warn 30 days
  before the certificate expires.

## Consequences

- A person moves to a new computer by importing the `.p12` again (the
  keyring is not exported).
- The key is as protected as the keyring: unlocked with the session. That
  is the same level as the stored passwords, and is stated in SECURITY.md.
- No smartcard/PKCS#11 in this step.

## Alternatives considered

- **Keep the `.p12` on disk and send its path to charon:** exactly what the
  `0002` patch forbids (root reading a client-chosen path). Rejected.
- **Keep the `.p12` in the keyring and decrypt it at each connection:**
  needs its password every time or a stored password, which is the same
  secret in two places. Rejected.
- **OpenSSL from the start:** larger engine and attack surface for a case
  not seen yet. Deferred, as above.
- **PKCS#11 (smartcards, YubiKey):** the key never leaves the token, which is
  better, but it needs the `pkcs11` plugin loading a module (a path, in root
  charon). Deferred to its own ADR.
