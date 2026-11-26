# ADR-0032: The engine builds without Docker too (static musl only)

- **Status:** Accepted. Phase 2 of the program to 1.1.0.
- **Date:** 2026-11-25

## Context

The embedded engine (strongSwan with the Ikebox patches, ADR-0019) was
built only inside the pinned Alpine builder image
(`native-engine/build.sh`, ADR-0020). That gives reproducible releases, but
it closes the door to distribution packagers: AUR, NixOS, COPR and OBS build
in their own environments, usually without Docker and without network
access during the build. Feedback after 1.0 asked for a plain shell build
on the host, from sources already downloaded and checked.

What must not be lost is what the engine guarantees today: the sources
checked by SHA-256 **and** GPG; every Ikebox patch applied; one fully static
executable (musl and OpenSSL inside), so neither the host's glibc nor its
crypto policies change it; and the hardening of the release charon
(static-pie, full RELRO, BIND_NOW, non-executable stack). The maintainer
chose **static musl only**: no dynamically linked "system" variant.

## Decision

- **One script, two places.** `native-engine/build/build-strongswan.sh`
  runs inside the builder (the default of `native-engine/build.sh`, used by
  releases) and directly on a computer (`native-engine/build.sh --host`).
  Its defaults are the builder's paths; the host build passes its own. The
  builder's output did not change: the engine and tools tarballs built with
  the new script have the same SHA-256 as before it.
- **Offline sources.** `native-engine/build.sh --fetch DIR` downloads the
  strongSwan tarball and the OpenSSL tarball with their signatures and
  checks them; `--sources DIR` builds from such a folder and never touches
  the network. Each tarball is checked against its pinned SHA-256 and a
  signature from its own project's key, in a keyring holding only that key
  (the status lines are read, not gpg's text, which follows the computer's
  language).
- **The toolchain is checked, not trusted.** It must be musl (a static glibc
  charon would break name resolution), see the kernel's UAPI headers (linked
  in when `musl-gcc` on a glibc distribution does not search them), and give
  a static-pie executable with RELRO and BIND_NOW. Alpine's gcc does that
  from `-static`, so the builder's flags stay as they were; other
  toolchains are asked for `-static-pie` and `-z relro -z now` explicitly.
  Where `musl-gcc`'s specs file cannot link static-pie (Arch's), the build
  uses a copy of it with musl's `rcrt1.o` and the static-pie link line. The
  finished charon is checked for all of it, in both places.
- **OpenSSL: the toolchain's own, or compiled from a pin.** `--openssl auto`
  (the default) uses the toolchain's static OpenSSL for musl when one links
  (the builder's Alpine package, Nix's `pkgsStatic`), and otherwise compiles
  the release pinned in `native-engine/openssl.txt`: the same upstream
  version as the builder's package (3.5.9), checked by SHA-256 and by
  OpenSSL's release key (`keys/openssl-release.asc`, fingerprint
  `B146 647E 45A7 B339 47AB 226B 2A2C 87D1 6169 2D40`, as
  openssl-library.org/source gives it). It is built static, without
  loadable modules and without the legacy provider, with `/etc/ssl` as its
  configuration directory: what the builder's package does where it matters
  to charon (patch 0003, ADR-0027).
- **BUILDINFO says which build it is.** The builder's lines are unchanged;
  a host build records `builder=host:<distribution> (musl <version>;
  <compiler>)` and where its OpenSSL came from. `linkage=static`, the
  patches and the configure options are the same in both.
- **Tested where packagers are.** `tests/engine-host/run.sh` (`npm run
  test:engine-host`) builds on plain Fedora 45, Ubuntu 24.04 and Arch
  containers with only their own packages, as an unprivileged user and with
  no network, and runs the engine's E2E with each result. CI builds on its
  Ubuntu runner itself and runs the E2E. The weekly upstream watch flags an
  `openssl.txt` that falls behind the builder's OpenSSL.

## Consequences

- Packagers can build the engine with their distribution's musl packages
  (Fedora: `musl-gcc musl-libc-static kernel-headers`; Debian/Ubuntu:
  `musl-tools linux-libc-dev`; Arch: `musl`), from a sources folder. The
  pieces and the rules are in [docs/packaging.md](../packaging.md).
- A host build is not byte-identical to the release engine (another
  compiler, another musl, its own build paths); only the builder is
  reproducible, and releases keep using it. `verify-release.sh` checks
  release packages as before.
- FORTIFY_SOURCE comes from Alpine's fortify-headers; a musl toolchain
  without them ignores the flag. The other hardening is checked on every
  build.
- Two pins move together when the builder's OpenSSL changes:
  `native-engine/openssl.txt` and `native-engine/licenses/openssl/`.
- Official distribution repositories that forbid bundled libraries
  (Fedora's, Debian's) would not accept a static OpenSSL; AUR, Nix, COPR
  and OBS projects can.

## Alternatives considered

- **A dynamically linked variant with the distribution's OpenSSL:** more
  packaging reach, but the host's crypto policies could remove the legacy
  algorithms these VPNs need, the legacy-provider behavior would differ
  from patch 0003's assumption, and it would need its own E2E runs.
  Rejected by the maintainer.
- **Keep Docker as the only path:** reproducible, but it blocks the
  packagers who asked. Rejected.
- **A musl cross toolchain or `zig cc`:** works, but adds a toolchain no
  distribution ships by default; the distributions' own `musl-gcc` is
  enough with the checks above.
