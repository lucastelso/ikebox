# Third-party notices

IPsec Client is licensed under GPL-3.0-or-later (see `LICENSE` and
ADR-0017). It distributes or depends on the components below, under their own
licenses.

## Embedded engine (distributed compiled inside the app)

### strongSwan

- **License:** GPL-2.0-or-later, with an exception for combining with LGPL
  libraries and OpenSSL. The des, md4 and md5 plugins have their own
  permissive terms. The full texts go in the engine package, in
  `opt/ipsec-client/engine/share/licenses/strongswan/`.
- **Corresponding source code:**
  - the official tarball at <https://download.strongswan.org/>, with the
    version and SHA-256 pinned in `native-engine/versions.txt` and the GPG
    signature checked in the build (key in `native-engine/keys/`);
  - **one modification**, made on 2026-11-16 by IPsec Client:
    `native-engine/patches/0001-vici-forced-updown-if-id.patch`. It changes
    `src/libcharon/plugins/vici/vici_config.c` so that charon ignores the
    `updown` and the `if_id` sent by the VICI client when
    `charon.plugins.vici.force_updown`/`force_if_id` are set (ADR-0019). The
    build applies the patches in `native-engine/patches/` over the official
    tarball and records the SHA-256 of each one in the package `BUILDINFO`;
  - the build options are in `native-engine/build/build-strongswan.sh` and in
    the package `BUILDINFO`.

### GMP (GNU Multiple Precision Arithmetic Library)

- **License:** LGPL-3.0-or-later or GPL-2.0-or-later. The text goes in the
  engine package, in `opt/ipsec-client/engine/share/licenses/gmp/`.
- **Use:** statically linked into charon (`gmp` plugin, MODP DH groups).
- **Corresponding source code:** the Ubuntu 22.04 `gmp` source package, in
  the version recorded in `BUILDINFO` (currently `2:6.2.1+dfsg-3ubuntu1`), at
  <https://launchpad.net/ubuntu/+source/gmp>. To rebuild, use
  `native-engine/build.sh`.

## Desktop app

- **Tauri** and Rust crates: MIT, Apache-2.0, BSD, ISC, MPL-2.0, Zlib,
  Unicode-3.0 and CDLA-Permissive-2.0, checked by `cargo deny` (see
  `deny.toml`).
- **Frontend** (React, Radix UI, lucide, sonner etc.): MIT, Apache-2.0, ISC and
  0BSD.
- **WebKitGTK, GTK and dependencies** (in the AppImage): LGPL-2.1-or-later and
  other free licenses, as unmodified dynamic libraries.

## Docker backend (tests and development)

The `engine/` image is **built locally** from the Ubuntu 20.04 packages
(strongSwan 5.8.2). The project does not distribute that image.
