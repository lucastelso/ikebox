# Embedded engine (compiled strongSwan)

The app ships its own strongSwan, compiled from the official source code, so
the user does not need Docker (ADR-0013 and ADR-0016).

| File | Role |
|---|---|
| `versions.txt` | Supported versions, each with its path and the tarball **SHA-256** |
| `keys/strongswan-release.asc` | strongSwan release public key (to check the GPG signature) |
| `build/Dockerfile` | Ubuntu 22.04 builder (glibc 2.35, the same base as the AppImage) |
| `patches/*.patch` | IPsec Client security patches, applied **after** checking the source. Today there is one: the VICI client does not choose the `updown` (run as root) or the `if_id` (ADR-0019) |
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
`ipsec-client-engine-<version>-x86_64.tar.gz`, with charon, swanctl, the
configuration and a `BUILDINFO` (version, hash, patches with SHA-256, build
options, dependencies). Staging (`scripts/stage-network-component.sh`), the
release build and `verify-release.sh` refuse an engine without the patches of
the commit. When changing the strongSwan version, port the patch if it does
not apply; the 5.x fallback versions do not have the patch ported. charon
depends **only on glibc**.

Included plugins: kernel-netlink, socket-default, vici, updown, xauth-generic,
unity, nonce, random, drbg, aes, des, sha1, sha2, md5, hmac, gcm, chapoly,
curve25519, gmp (modp, including 768/1024), pem, pkcs1, pkcs8, x509 and pubkey.
kdf is included in 6.x. Without OpenSSL, **there are no ECP curves**.
