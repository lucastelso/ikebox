# ADR-0027: OpenSSL in the embedded engine (ECDSA and ECP groups), replacing GMP

- **Status:** Accepted. Phase C of the program to 1.0.0 (step 52). Amends
  ADR-0013, ADR-0020, ADR-0023 and ADR-0024 (their "RSA and Ed25519 only").
- **Date:** 2026-11-18

## Context

The engine had no OpenSSL (ADR-0013): RSA and the MODP groups came from
GMP, Ed25519 and X25519 from strongSwan's curve25519 plugin. So it could not
verify or sign **ECDSA** certificates (ADR-0023, ADR-0024 refused them in the
profile and at the `.p12` import) nor negotiate the **ECP** Diffie-Hellman
groups (19, 20, 21), common in IKEv2 configurations of FortiGate, Cisco and
cloud gateways. The maintainer put ECDSA in the scope of 1.0.

## Decision

- The engine builds strongSwan's **`openssl` plugin** against Alpine's
  static libcrypto (`openssl-libs-static`, OpenSSL 3.5, pinned by version
  in the builder like musl). charon stays one fully static executable.
- **GMP goes away.** OpenSSL does RSA and the MODP groups: one
  implementation per algorithm, and no LGPL code linked statically.
- strongSwan's own plugins **stay** for what they already did (AES, SHA-1/2,
  MD5, HMAC, GCM, ChaCha20-Poly1305, X25519/Ed25519): in the monolithic
  build they load before `openssl`, so their tested code keeps those
  algorithms. **DES and MD4** must stay there: OpenSSL 3 has them only in
  its legacy provider, a loadable module that a static build cannot load.
  Single DES matters for old gateways; MD4 for EAP-MSCHAPv2. Patch `0003`
  makes the openssl plugin stop announcing the legacy-provider algorithms
  (RC5, CAST, Blowfish, IDEA, single DES, MD4), so none of them can be
  picked from OpenSSL and fail at runtime. The engine's configuration
  (`engine/strongswan.d/`) is shared with the Docker engine used as the
  test gateway, whose OpenSSL does have the legacy provider: nothing about
  it goes there (setting `load_legacy = no` there broke the gateway's
  MSCHAPv2 during this phase).
- The app accepts ECDSA CAs and ECDSA keys in `.p12` files; the "modern"
  proposal preset adds ECP384 and ECP256 after curve25519 (profiles saved
  with the old list are still recognized as "modern").
- `BUILDINFO` records the OpenSSL version; `verify-release.sh` requires it.
  The license text (Apache-2.0) is in `native-engine/licenses/openssl/`,
  taken from the signed upstream tarball, and the sources archive carries
  the OpenSSL release with Alpine's packaging (ADR-0017).

## Consequences

- The engine tarball grows from about 1 MB to 2.6 MB (charon 6.9 MB): the
  price of libcrypto.
- More code in the root process that parses what the gateway sends
  (certificates, key exchange). OpenSSL is the most reviewed of the
  options, and its advisories are followed (ADR-0026).
- Verified end to end: the whole `test:native` (including IKEv1 aggressive
  with MODP_1024 and MODP_768 PFS, now through OpenSSL) and `test:modes`
  with a new gateway that uses ECDSA on both sides (P-384 gateway, P-256
  client) and only ECP groups.
- The license of the combination: strongSwan's license carries an OpenSSL
  exception, and its "or later" allows GPL version 3, compatible with
  Apache-2.0.

## Alternatives considered

- **OpenSSL next to GMP:** two implementations of RSA and MODP in the same
  root process, for nothing.
- **gcrypt or botan plugins:** less common, and gcrypt has no ECDSA in
  strongSwan.
- **Keep refusing ECDSA:** some real gateways only have ECDSA certificates
  and ECP groups; they would stay out of reach.
