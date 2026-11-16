# ADR-0018: The app installs the network component (embedded bundle + pkexec)

- **Status:** Accepted. Implemented in the `feat/setup-appimage` branch and
  validated by the maintainer with the real password prompt (step 30).
- **Date:** 2026-11-16

## Context

The embedded engine (ADR-0013) depends on a component installed as root: the
helper, the compiled strongSwan, the systemd units and the polkit policy
(ADR-0014). With the **AppImage**, nobody installs it: the user only downloads
a file and runs it.

There is also a technical limitation: root cannot read the AppImage FUSE mount
point, which belongs to the user. That is why the installer cannot run
straight from inside the AppImage.

## Decision

- **Component bundle inside the app.**
  - `scripts/stage-network-component.sh` builds a directory with:
    - the files: helper, strongSwan tarball, `entrypoint.sh`, `updown`,
      configuration, units, policy and the installer itself;
    - `VERSION`, with the app version and the strongSwan version;
    - `SHA256SUMS`, with the hash of every other file.
  - The release build (`npm run bundle`, which uses `tauri.release.conf.json`)
    puts that directory among the app resources, in `<resources>/network`.
  - Development builds do not carry the bundle. To test, point to one with
    `IPSEC_CLIENT_COMPONENT_BUNDLE`, which only works in debug builds.
- **Installation by the app** (`ipsec-client-core::component`):
  1. The app copies the bundle to a new directory, mode 0700, in
     `$XDG_RUNTIME_DIR` (tmpfs, the user's only). In the copy, it sets the
     permissions and checks the hashes.
  2. It runs `pkexec <copy>/install-native.sh install --bundle <copy>
     --origin app`, with a clean environment: no `LD_LIBRARY_PATH` from the
     AppImage reaches root. The system asks for the administrator password
     once.
  3. The installer, now as root, does its part:
     - copies the bundle to a directory **of its own**;
     - refuses symlinks and special files;
     - checks `SHA256SUMS` and requires the full file list;
     - refuses a tarball with paths outside `opt/ipsec-client/engine`;
     - only then installs.
- **Installation record** in `/opt/ipsec-client`:
  - `VERSION` and `SHA256SUMS`, copied from the bundle;
  - `ORIGIN`, which says who installed it: `app`, `package` or `manual`.

  The app compares the SHA-256 of the installed `SHA256SUMS` with that of the
  bundle it carries. If they differ, the component is **outdated** and the
  app offers "Update".
- **Removal** with the copy of the installer kept in
  `/opt/ipsec-client/libexec/install-native.sh`. Before calling `pkexec`, the
  app checks that this file and the directories above it belong to root and
  that nobody else can change them. Removal never runs a user file as root.
- **Component installed by a system package** (`ORIGIN=package`): the app
  neither installs nor removes it. It says that is done with the package
  manager.
- **In the interface:**
  - on the System screen, the "Network component" card shows the state and
    the versions, with Install/Update/Reinstall and Remove (with confirmation)
    buttons;
  - on the main screen, a first-use notice shows up when something is missing;
  - on the prerequisite error, an "Install network component" button.

  With an active connection, installing and removing are refused, because
  they would bring the tunnel down.
- **In the terminal:** `ipsec-client-cli component status|install|uninstall`.

## Consequences

- AppImage users install everything from the app itself, with the
  administrator password once. After that, connecting does not ask for a
  password (ADR-0014).
- Updating the app brings a new component, and the app notices the difference
  on its own.
- The same installer and bundle serve three paths: the app,
  `scripts/test-native.sh` (with `--origin manual`) and, later, the post-install
  of the rpm/deb packages (with `--origin package`).
- `tests/packaging/install-test.sh` tests the installer as root in a
  throwaway container, including tampered bundles, and runs in CI.
- **The helper goes compressed in the bundle** (`ipsec-client-helper.gz`).
  When building the AppImage, linuxdeploy rewrites the RPATH of every ELF
  executable it finds in the app, and the helper hash stopped matching: the app
  would refuse to install. The problem was found while checking the first
  release AppImage. Since then, `scripts/verify-release.sh` checks the
  component inside the three packages at the end of every release build.
- **Residual risk:** a malicious program running as the user could swap the
  bundle copy between the app's check and root's read. Such a program already
  has access to everything of the user, and no AppImage can protect itself
  from it, because the downloaded file itself is the root of trust. The hashes
  protect against corruption and against other users. For the package
  manager's trust, use the rpm or the deb.

## Alternatives considered

- **Running the installer straight from the AppImage.** Impossible: root does
  not read the user's FUSE mount.
- **Our own polkit policy for the installation.** It would give a nicer
  message in the password prompt, but the policy would have to be installed
  already. A chicken-and-egg problem.
- **Downloading the component from the internet on first use.** It needs the
  network, a server and signature checks, and the app would stop working
  offline.
- **Installing in `/usr/lib/ipsec-client`**, as ADR-0014 planned. Changed to
  `/opt/ipsec-client`, which does not collide with distribution package files
  and is easy to remove entirely.
