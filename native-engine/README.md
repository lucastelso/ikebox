# Embedded engine (compiled strongSwan)

The app ships its own strongSwan, compiled from the official source code, so
the user does not need Docker (ADR-0013 and ADR-0016). It is one fully static
executable with musl and OpenSSL inside (ADR-0020, ADR-0027), and it builds
either in the pinned Alpine builder (the official, reproducible path) or on
a computer with a musl toolchain (ADR-0032).

| File | Role |
|---|---|
| `versions.txt` | Supported strongSwan versions, each with its path and the tarball **SHA-256** |
| `openssl.txt` | The OpenSSL release host builds compile when the toolchain has no static OpenSSL for musl: version, URL, **SHA-256** (the same upstream release as the builder's package) |
| `keys/strongswan-release.asc` | strongSwan's release public key (GPG check of its tarball) |
| `keys/openssl-release.asc` | OpenSSL's release public key, `B146 647E 45A7 B339 47AB 226B 2A2C 87D1 6169 2D40` |
| `patches/*.patch` | Ikebox security patches, applied **after** the source is checked, in order (below) |
| `licenses/` | License texts of OpenSSL and musl, which go into the static executable; where they came from is in `licenses/README.md` |
| `build/build-strongswan.sh` | The build itself, the same in the builder and on a host: checks SHA-256 and GPG, applies the patches (all must apply), checks the toolchain, compiles, checks charon's hardening and packs deterministic tarballs |
| `build/Dockerfile` | The Alpine builder (musl, OpenSSL 3.5), pinned by digest |
| `build.sh` | Runs the build: `native-engine/build.sh [--host] [VERSION\|all]`, `--fetch DIR` for the sources |
| `spike/` | Test of the compiled engine against the test gateway, on a simulated Fedora 45 host |

The patches:

- `0001`: the VICI client does not choose the `updown` (run as root) or the
  `if_id` (ADR-0019);
- `0002`: it cannot make charon open certificate files by path (audit of
  0.2.0, P9);
- `0003`: the openssl plugin does not announce the legacy-provider
  algorithms, which a static build cannot run (ADR-0027);
- `0004`: keys held by an ssh-agent (smartcards and tokens), through a
  socket chosen by root (ADR-0029).

## Building

```bash
npm run build:engine        # in the builder (Docker): what releases use
native-engine/build.sh --host                          # on this computer, with its musl toolchain
native-engine/build.sh --fetch ~/src && \
  native-engine/build.sh --host --sources ~/src        # offline, from checked sources
```

The host build needs a musl C toolchain and the kernel headers (Fedora:
`musl-gcc musl-libc-static kernel-headers`; Debian/Ubuntu: `musl-tools
linux-libc-dev`; Arch: `musl`) plus `make perl pkg-config python3 bzip2 patch
file binutils gnupg`. Packagers: [docs/packaging.md](../docs/packaging.md).

What every build guarantees, in the builder and on a host:

- the tarballs are checked by SHA-256 **and** by their project's GPG
  signature, each against a keyring with only that project's key;
- every patch applies, or the build stops;
- musl, not glibc (a static glibc would break name resolution), and charon
  static-pie with full RELRO, BIND_NOW and a non-executable stack;
- deterministic tarballs. Only the builder is reproducible byte for byte:
  a host build uses another compiler and musl.

The result goes to `native-engine/dist/` (ignored by git), or to `--out`:

- `ikebox-engine-<version>-x86_64.tar.gz`: what users get. charon, its
  configuration, the license texts and a `BUILDINFO` (version, hash,
  patches with SHA-256, builder, OpenSSL and musl versions, build options);
- `ikebox-engine-tools-<version>-x86_64.tar.gz`: `swanctl`, used only by
  the tests to drive charon. It never goes into a package.

## Testing

```bash
npm run test:native        # builder engine: full E2E (5 scenarios, including the ADR-0019 attack)
npm run test:engine-host   # host builds on Fedora, Ubuntu and Arch containers, each through the E2E
npm run test:systemd       # real systemd in a container: installer, helper, sandbox, 2 users
native-engine/build.sh all && native-engine/spike/run-e2e.sh 5.8.2   # fallback
```

Staging (`scripts/stage-network-component.sh`), the release build and
`verify-release.sh` refuse an engine without the patches of the commit, and
`verify-release.sh` also refuses one that is not fully static. When changing
the strongSwan version, port the patches if they do not apply; the 5.x
fallback versions do not have them ported. When the builder's OpenSSL moves
to another release, move `openssl.txt` and `licenses/openssl/` with it (the
weekly upstream watch says so). charon depends on **nothing** of the
computer it runs on besides the kernel.

Included plugins: kernel-netlink, socket-default, vici, updown,
xauth-generic, unity, nonce, random, drbg, aes, des, sha1, sha2, md5, md4,
hmac, gcm, chapoly, curve25519, openssl (RSA, ECDSA, MODP and ECP groups),
pem, pkcs1, pkcs8, x509, pubkey, eap-identity, eap-mschapv2, agent and
sshkey; kdf in 6.x.
