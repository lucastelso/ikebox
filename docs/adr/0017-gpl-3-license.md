# ADR-0017: GPL-3.0-or-later license

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

The project will be published as open source. It is a **security** tool
(credentials, network, privileges) that replaces a proprietary client, and the
app distributes the **compiled strongSwan** inside it (ADR-0013).

| Component | License | Relation to the app |
|---|---|---|
| strongSwan (charon, swanctl) | GPL-2.0-or-later, with an exception for combining with LGPL libraries and OpenSSL. The des/md4/md5 plugins have their own permissive terms | Distributed compiled; runs as a **separate process** and talks over the VICI socket |
| GMP (static in charon) | LGPL-3.0-or-later or GPL-2.0-or-later | Part of the charon binary (allowed by the strongSwan exception) |
| Rust dependencies | MIT, Apache-2.0, BSD, ISC, MPL-2.0, Zlib, Unicode-3.0, CDLA-Permissive-2.0 | Linked into the app |
| Frontend dependencies | MIT, Apache-2.0, ISC, 0BSD | Embedded in the app |
| WebKitGTK/GTK (AppImage) | LGPL | Dynamic libraries carried in the AppImage |

All dependencies are compatible with GPLv3. The app is not a derivative work
of strongSwan, because they are separate processes that communicate through a
protocol. The app's license is therefore a free choice.

## Decision

- The project's code is licensed under **GPL-3.0-or-later** (`LICENSE`).
- The SPDX `GPL-3.0-or-later` goes into the metadata: `Cargo.toml`,
  `package.json` and the rpm/deb packages.
- Contributions come in under the same license (*inbound = outbound*;
  CONTRIBUTING.md).
- **Compliance of the distributed components** (`THIRD_PARTY_NOTICES.md`):
  - the engine tarball carries the strongSwan, GMP and musl license texts
    (`share/licenses/`) and a `BUILDINFO` with versions, hashes and patches;
  - the corresponding source code is identified precisely: the official
    strongSwan tarball (URL + SHA-256 + signature) plus our patches, GMP and
    musl from Alpine (ADR-0020) and the build scripts in `native-engine/`.

**Updated in phase B of the program to 1.0.0 (2026-11-18):** every package
carries `/usr/lib/ipsec-client/licenses/` (the project's license, the
notices, the texts of the crates and npm packages compiled into the
programs, and the engine's texts), and each release attaches the
corresponding-source archive made by `scripts/release-sources.sh`
(strongSwan with the patches, GMP and musl with Alpine's packaging at fixed
aports commits).

**Updated in phase C (2026-11-18):** OpenSSL (Apache-2.0) replaced GMP in
the engine (ADR-0027). strongSwan's license carries an OpenSSL exception,
and its "GPL version 2 or later" lets the combination be distributed under
GPL version 3, compatible with Apache-2.0; the app itself is GPL-3.0-or-later
already. The engine carries the OpenSSL license text, and the sources
archive the OpenSSL release with Alpine's packaging.

## Consequences

- Any redistributed version, modified or not, must keep the source code
  available, so nobody can ship a closed derivative of a security tool.
- The license is from the same family as the strongSwan inside the app, and
  GPL-2.0-or-later code can be reused if needed.
- Companies can use the app freely. The obligation only exists when
  **redistributing**.
- Changing the license later would need the consent of every contributor.
  Today there is only the maintainer.

## Alternatives considered

- **MIT OR Apache-2.0**, the Rust ecosystem default: maximum adoption, but it
  allows closed derivatives.
- **GPL-2.0-or-later**, like strongSwan: it would be incompatible with the
  Apache-2.0 dependencies if someone picked only v2.
- **AGPL-3.0**: protects use as a network service, which does not apply to a
  desktop app.
- **MPL-2.0**: per-file copyleft only, weaker protection for the whole.
