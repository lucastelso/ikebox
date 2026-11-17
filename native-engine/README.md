# Embedded engine (compiled strongSwan)

The app ships its own strongSwan, compiled from the official source code, so
the user does not need Docker (ADR-0013 and ADR-0016).

| File | Role |
|---|---|
| `versions.txt` | Supported versions, each with its path and the tarball **SHA-256** |
| `keys/strongswan-release.asc` | strongSwan release public key (to check the GPG signature) |
| `build/Dockerfile` | Alpine builder (musl): charon comes out as one fully static executable (ADR-0020) |
| `patches/*.patch` | IPsec Client security patches, applied **after** checking the source, in order: `0001`, the VICI client does not choose the `updown` (run as root) or the `if_id` (ADR-0019); `0002`, it cannot make charon open certificate files by path or use tokens (audit of 0.2.0, P9) |
| `licenses/` | License texts of GMP and musl, which go into the static executable; where they came from is in `licenses/README.md` |
| `build/build-strongswan.sh` | Downloads the tarball, checks SHA-256 and GPG, applies the patches (all must apply), compiles (static, monolithic, no OpenSSL) and builds a deterministic tarball |
| `build.sh` | Runs the builder: `native-engine/build.sh [version\|all]` |
| `spike/` | Test of the compiled engine against the test gateway, on a simulated Fedora 45 "host" |

```bash
npm run build:engine     # builds the default version (the first in versions.txt)
npm run test:native      # full E2E (5 scenarios, including the ADR-0019 attack)
npm run test:systemd     # real systemd in a container: installer, helper, sandbox, 2 users
native-engine/build.sh all && native-engine/spike/run-e2e.sh 5.8.2   # fallback
```

The result goes to `native-engine/dist/` (ignored by git):

- `ipsec-client-engine-<version>-x86_64.tar.gz`: what users get. charon,
  its configuration, the license texts and a `BUILDINFO` (version, hash,
  patches with SHA-256, builder, GMP and musl versions, build options);
- `ipsec-client-engine-tools-<version>-x86_64.tar.gz`: `swanctl`, used only
  by the tests to drive charon. It never goes into a package.

Staging (`scripts/stage-network-component.sh`), the release build and
`verify-release.sh` refuse an engine without the patches of the commit, and
`verify-release.sh` also refuses one that is not fully static. When changing
the strongSwan version, port the patches if they do not apply; the 5.x
fallback versions do not have them ported. charon depends on **nothing** of
the computer it runs on besides the kernel.

Included plugins: kernel-netlink, socket-default, vici, updown, xauth-generic,
unity, nonce, random, drbg, aes, des, sha1, sha2, md5, hmac, gcm, chapoly,
curve25519, gmp (modp, including 768/1024), pem, pkcs1, pkcs8, x509 and pubkey;
since 0.7.0 also eap-identity, eap-mschapv2 and md4 (IKEv2 + EAP-MSCHAPv2,
ADR-0023). kdf is included in 6.x. Without OpenSSL, **there are no ECP
curves** and no ECDSA: gateway certificates must be RSA or Ed25519.
