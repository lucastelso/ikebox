# Packaging Ikebox for a distribution

For people who package Ikebox for AUR, Nix, COPR, OBS or similar. The
project's own `.rpm`, `.deb` and AppImage are built another way
([release.md](release.md)); this page is about building every piece in your
own environment, without Docker and, if you want, without network access.

## The pieces

| Piece | What it is | Built with |
|---|---|---|
| Engine | strongSwan with the Ikebox patches, one static executable (`engine.tar.gz`) | `native-engine/build.sh --host` |
| `ikebox-helper` | the network component's privileged part (root, systemd, polkit) | `cargo build --release -p ikebox-helper` |
| `ikebox-cli` | the command line | `cargo build --release -p ikebox-cli` |
| `ikebox` | the desktop app (Tauri 2, WebKitGTK 4.1) | `npm ci && npm run bundle` (or `cargo tauri build`) |
| Network component bundle | engine + helper + scripts + units + polkit policy, with `SHA256SUMS` | `scripts/stage-network-component.sh` |

The network component is installed into `/opt/ikebox` by
`install-native.sh install --bundle <dir> --origin package`, run as root from
the package's post-install, and removed by `install-native.sh uninstall`
from the pre-removal (see `packaging/package/postinst.sh` and `prerm.sh`,
which the project's packages use as they are).

## 1. The engine

### Sources, checked once

```bash
native-engine/build.sh --fetch ~/ikebox-sources
```

It downloads the strongSwan release pinned in `native-engine/versions.txt`
and the OpenSSL release pinned in `native-engine/openssl.txt`, each with its
signature, and checks both: the pinned SHA-256 and a good signature from
that project's key in `native-engine/keys/`. If your build system fetches
sources itself, take the URLs and hashes from those two files and fetch the
`.sig` (strongSwan) and `.asc` (OpenSSL) next to them.

### The build

```bash
native-engine/build.sh --host --sources ~/ikebox-sources --out dist/
```

With `--sources`, nothing is downloaded: the build works offline, and it
checks the files again. It needs a **musl** C toolchain and the kernel
headers:

| Distribution | Packages |
|---|---|
| Fedora | `musl-gcc musl-libc-static kernel-headers make pkgconf-pkg-config python3 perl bzip2 patch file binutils gnupg2` |
| Debian, Ubuntu | `build-essential musl-tools linux-libc-dev pkg-config python3 perl bzip2 patch file gnupg` |
| Arch | `base-devel musl python perl bzip2 patch file gnupg` |
| Alpine | `build-base linux-headers openssl-libs-static openssl-dev bash pkgconf python3 perl bzip2 patch file gnupg` |
| Nix | a musl `stdenv` (`pkgsMusl` or `pkgsStatic`); with `pkgsStatic.openssl` it is used as is |

`CC` chooses the compiler (by default `cc` when it is musl, otherwise
`musl-gcc`). `--openssl source` forces OpenSSL to be compiled from the pin
even when the toolchain has one; `--openssl system` refuses to compile it.

The result is in `dist/`: `ikebox-engine-<version>-x86_64.tar.gz` (what goes
into the package) and `ikebox-engine-tools-<version>-x86_64.tar.gz`
(`swanctl`, for tests only: do not ship it).

### What the build refuses, on purpose

These are the engine's security properties
([ADR-0032](adr/0032-engine-build-without-docker.md)); please keep them in
your package rather than working around them:

- **Every patch in `native-engine/patches/` must apply.** They stop a local
  user from making charon run a command as root or open files by path
  (ADR-0019). The network component bundle refuses an engine whose
  `BUILDINFO` lacks any of them.
- **Static musl, no glibc, no shared OpenSSL.** charon must not depend on the
  host's libraries or crypto policies (ADR-0020, ADR-0027). A dynamically
  linked engine is not supported.
- **Hardening:** charon must be static-pie with full RELRO, BIND_NOW and a
  non-executable stack. The build checks it.
- **The prefix is `/opt/ikebox/engine`.** The helper, the systemd units and
  the installer expect it there.

## 2. The helper and the command line

```bash
cargo build --release --locked -p ikebox-helper -p ikebox-cli
```

They need Rust (the version in `rust-toolchain.toml`), `pkg-config` and the
D-Bus headers (`dbus-devel` / `libdbus-1-dev`). They do not need Node,
WebKitGTK or Docker. The release profile leaves out the Docker engine, which
only development builds use.

For the command line's manual page and completions, see
`scripts/stage-cli.sh`. A package with only these two and the engine (no
desktop app) is a supported target: see
[CLI only](cli.md#cli-only-without-the-desktop-app). Do not install the
example polkit rule of `packaging/polkit/examples/`: it is the
administrator's choice ([ADR-0033](adr/0033-cli-only-and-servers.md)).

## 3. The network component bundle

```bash
scripts/stage-network-component.sh --out bundle/ \
    --engine dist/ikebox-engine-<version>-x86_64.tar.gz \
    --helper target/release/ikebox-helper
```

Ship that folder as `/usr/lib/ikebox/network` and run
`install-native.sh install --bundle /usr/lib/ikebox/network --origin package`
from the post-install, as `packaging/package/postinst.sh` does. The
installer checks every file against `SHA256SUMS`, creates the
`ikebox-engine` system user, installs the systemd units and the polkit
policy, and starts the helper's socket.

## 4. The desktop app

`npm ci && npm run bundle -- --bundles <format>` builds the app and stages
the pieces above itself (`src-tauri/tauri.release.conf.json`). With an
engine you built, put it at
`native-engine/dist/ikebox-engine-<version>-x86_64.tar.gz` first, and the
staging script uses it instead of building one in Docker. The app needs
WebKitGTK 4.1 at run time; the libraries are in the README's "To build"
list.

## Checking your package

- `ikebox-cli doctor` on the installed system says whether the component,
  the kernel module, systemd-resolved and the keyring are in place.
- `tests/engine-host/run.sh` builds the engine the way this page describes
  on Fedora, Ubuntu and Arch containers and runs the engine's end-to-end
  test with each result.
