# Licenses of what the engine links statically

The embedded engine is one static executable. Besides strongSwan (whose
license texts come from its own source tarball at build time), it contains:

| Component | Version | License | Files here |
|---|---|---|---|
| OpenSSL (libcrypto) | 3.5, Alpine package `openssl-libs-static` | Apache-2.0 | `openssl/LICENSE.txt` |
| musl libc | 1.2.6, Alpine package `musl-dev` | MIT | `musl/COPYRIGHT` |

The texts were taken from the upstream source tarballs of those versions,
checked when they were added:

- `openssl-3.5.9.tar.gz`, SHA-256
  `603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a`, good
  signature from key `C46ED3F2CBEFDA1FDAADA44264ED7B1DCCE71CB2` (OpenSSL,
  `openssl@openssl.org`), checked on 2026-11-18;
- `musl-1.2.6.tar.gz`, SHA-256
  `d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a`, good
  signature from key `836489290BB6B70F99FFDA0556BCDB593020450F` (musl libc).

`native-engine/build/build-strongswan.sh` copies them into the engine bundle
(`share/licenses/`). When the builder image moves to other versions of OpenSSL
or musl, update these files and this table.

GMP was linked until 0.9.0 (MODP groups and RSA); the OpenSSL plugin
replaced it in phase C of the program to 1.0.0 (ADR-0027).
