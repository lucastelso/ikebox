# ADR-0020: Static engine with musl, no Docker engine in release builds

- **Status:** Accepted. Phase 4 of the post-0.2.0 program (step 40).
- **Date:** 2026-11-17
- **Amends:** ADR-0001 (Docker engine), ADR-0013 and ADR-0016 (how the
  embedded engine is built).

## Context

The roadmap asked to measure first and then decide, with fixed criteria:
compatibility must not get worse; the E2E, the ADR-0019 attack and the canary
sweep must pass the same; size, build time and reproducibility count; musl
only goes in if the whole E2E passes, **name resolution inside the sandbox
included**.

What was there before this phase:

- charon was built on Ubuntu 22.04, static except for glibc (dynamic, for
  name resolution), and the engine tarball carried swanctl, strongSwan's
  stock `_updown`, the `xfrmi` tool and the configuration templates, none of
  them used in production;
- every build of the app and the CLI carried the Docker engine (bollard and
  its dependency tree), meant for development and tests;
- the Docker engine and the test gateway ran on Ubuntu 20.04 with strongSwan
  5.8.2, out of standard support.

## Decision

1. **The embedded engine is one fully static executable, built on Alpine
   with musl.** GMP and musl go inside it; it depends on nothing of the
   computer but the kernel.
   - The builder image is pinned by digest (Alpine 3.24.4, musl 1.2.6, GMP
     6.3.0). The license texts of GMP and musl, taken from their upstream
     sources with the signatures checked, live in `native-engine/licenses/`.
   - Hardening is stated, not left to compiler defaults: stack protector,
     `_FORTIFY_SOURCE=2`, stack clash protection; the static-pie link gives
     PIE, full RELRO and a non-executable stack.
   - `BUILDINFO` records the GMP and musl versions and `linkage=static`;
     `verify-release.sh` refuses an engine that is not fully static.
2. **Only what production runs goes to users.** `swanctl` moves to a tools
   tarball used by the tests; `_updown`, `xfrmi`, the swanctl configuration
   and the templates are no longer built into the bundle.
3. **Release builds have no Docker engine.** It becomes the `docker` feature
   of the core, the app and the CLI, off by default. Development
   (`npm run dev:app`) and the Docker tests turn it on; the Rust checks and CI
   build and test both ways.
4. **The Docker engine and the test gateway use Debian 13 slim** with
   Debian's strongSwan 6.0.1. The build fails if Debian moves to another
   upstream version, so the tests never change strongSwan silently.
5. **Release profile: `lto = "fat"`**, `codegen-units = 1`, symbols
   stripped. `panic` stays `unwind`: the helper frees a client's slot when the
   thread serving it panics; with `abort`, one panic would drop every
   connection being served.

Not changed, after measuring:

- **The release build image keeps Ubuntu 22.04** (2.63 GB): its glibc 2.35
  is the floor of the AppImage and the `.deb`; it already installs without
  recommended packages and keeps the cargo/npm downloads in named volumes.
  Most of it is the Rust toolchain and the WebKitGTK development files.
- **The frontend bundle** (443 KB of JavaScript, 137 KB compressed, loaded
  from inside the binary) needs no work.

## Measurements

| What | Before | After |
|---|---|---|
| Engine tarball shipped to users | 1.84 MB | **0.96 MB** (+ 0.58 MB tools tarball, tests only) |
| charon | 1.91 MB, needs glibc ≥ 2.34 | 2.06 MB, static (no libraries) |
| Stack canary reads in charon (stack protector) | 2661 | 2728 |
| Engine builder image | 568 MB (Ubuntu 22.04) | 506 MB (Alpine 3.24) |
| Engine build time | — | 1 min 25 s |
| App (release, stripped) | 10.5 MB | **8.6 MB** (9.3 MB without Docker, then fat LTO) |
| CLI | 5.0 MB, 159 crates | **3.7 MB**, 102 crates |
| Helper | 668 KB | 604 KB (fat LTO) |
| Docker engine / test gateway image | 173 MB, Ubuntu 20.04 (unsupported), strongSwan 5.8.2 | 209 MB, Debian 13, strongSwan 6.0.1 |

The Docker image grew: Debian's `systemd-resolved` (needed for `resolvectl`)
brings systemd 257 and udev. It is a development and test image only, and the
gain is a supported base and the strongSwan 6 line on both sides of the
tests.

## How the criteria were checked

- `npm run test:native` (five scenarios, including the ADR-0019 attack, the
  SIGKILL recovery, the full tunnel and modp768 PFS) and `npm run
  test:systemd` pass with the static engine.
- **Name resolution inside the sandbox:** `tests/systemd` now names the
  gateway by host name. A small DNS server serves the test domain to
  systemd-resolved, `/etc/resolv.conf` points at the resolved stub, and
  charon, with musl's resolver and no NSS modules, resolves the gateway from
  inside the engine sandbox. The scenario without systemd-resolved resolves
  it from `/etc/hosts`.
- The compatibility matrix (`npm run test:compat`, seven distributions) and
  the canary sweep pass the same.
- `test:engine`, `test:docker` and `test:cli` pass on the Debian 13 image,
  including the connection failure diagnosis, whose log signatures had been
  captured from strongSwan 5.8.2.

## Consequences

- The engine runs on any glibc or musl distribution with a recent enough
  kernel; the glibc floor of the app (2.35) now comes only from WebKitGTK.
- musl's resolver does not read `nsswitch.conf`: a gateway named by mDNS
  (`.local`) or another NSS-only source does not resolve. DNS (including
  systemd-resolved through its stub) and `/etc/hosts` work.
- A fix in GMP or musl needs an engine rebuild with a new builder image, and
  the license texts in `native-engine/licenses/` follow the versions.
- Developers who want the Docker fallback build with the feature
  (`npm run dev:app`); a release app without the network component says to
  install it, as it already did when Docker was missing.
- The older strongSwan versions of `native-engine/versions.txt` (5.x, 6.0.x)
  were not rebuilt on the new builder; they were never packaged (ADR-0016)
  and may need fixes to build with musl and a newer compiler.

## Alternatives considered

- **Keep glibc and link it statically.** glibc does not support static name
  resolution (NSS needs dynamic loading); it would have broken the very
  criterion the roadmap set.
- **Debian slim as the engine builder.** Smaller change, but the engine would
  still depend on the user's glibc.
- **Our 6.1.0 engine inside the Docker image.** It would make the Docker
  engine closer to production, but needs the engine tarball in the image
  build context the app embeds. The production engine is already covered by
  `test:native`, `test:systemd` and the compatibility matrix; the Docker image
  is development and test only.
