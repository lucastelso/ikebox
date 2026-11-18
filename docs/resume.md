# Current state and how to resume

> This file is updated **on every merge into `main`**. If work stops in the
> middle of a branch, that branch's last commit and the last entry of the
> [development log](development-log.md) say where it stopped.

## Where we are

- **Version 0.1.0 (2026-11-16): first working version, validated by the
  maintainer on the real VPN** (FortiGate, IKEv1 aggressive + group PSK +
  XAuth), reaching the work machine through Remmina. Validated path, the same
  as an end user's:
  1. app installed from the `.rpm`, opened from the applications menu;
  2. network component installed by the app itself, with the administrator
     password (`pkexec`, origin `app`);
  3. connection through the embedded strongSwan 6.1.0, **without Docker**.
- **Version 0.2.0 (2026-11-16), published and installed on the maintainer's
  computer:**
  - **security audit** (steps 32–35): no work data in the code, the history
    or the packages; a **local escalation to root** through VICI fixed
    (ADR-0019); "Copy anonymized"; private data scan inside release packages;
  - **commit email** rewritten to the GitHub noreply address in the whole
    history (the maintainer pushes with `--force`);
  - **internationalization** (step 36): bilingual app (follows the system
    language, setting in System → Appearance → Language), English CLI,
    helper and scripts, technical docs in English, user docs in English with
    Portuguese copies in `docs/pt-BR/`;
  - users of 0.1.0 must update the app **and** click "Update" on the network
    component.
- **Done:**
  - Rust core and `ipsec-client-cli`;
  - desktop app: connection, profiles (import/export), System tab, tray,
    notifications and single instance, in English and Portuguese;
  - embedded engine (helper with polkit and systemd sandbox, strongSwan with
    the ADR-0019 patch);
  - component installation by the app (ADR-0018);
  - `.rpm`/`.deb`/AppImage packages, with the
    [installation guide](installation.md).
- **Now: the post-0.2.0 program**, one phase per session
  ([roadmap](roadmap.md#post-020-program-decided-on-2026-11-16)): audit I
  (credentials, private data, supply chain) → audit II (privilege boundaries)
  → compatibility → optimization and base images → interface → IPsec modes.
  **Phases 1 to 10 are done: the post-0.2.0 program is complete** (steps 37
  to 48, [audit report](audits/v0.2.0.md),
  [ADR-0020](adr/0020-static-engine-and-lighter-builds.md) to
  [ADR-0025](adr/0025-deferred-xauth-otp-l2tp.md)). 0.3.0 carries phases 1–4
  (audit fixes, static engine, no Docker in release); 0.4.0 phase 5, the
  interface colored by the connection state; 0.5.0 phase 6, the IPsec modes
  certified by `npm run test:modes`, the security level and IKEv2
  diagnosis; 0.6.0 phase 7, profile schema 2 and the profile editor; 0.7.0
  phase 8, IKEv2 + EAP-MSCHAPv2; 0.8.0 phase 9, client certificates;
  **0.9.0** phase 10, importers for other clients (Cisco `.pcf`, vpnc,
  NetworkManager, `ipsec.conf`, FortiClient). After the 0.5.0 push, CI
  failed in three jobs (shellcheck version, AppArmor in the engine E2E):
  fixed in step 44, to be confirmed by the next CI run.
  **Now: the [program to 1.0.0](roadmap.md#program-to-100-decided-on-2026-11-18)**,
  one phase per session: 0 preparation → A security and supply chain → B
  complete packages → C engine with OpenSSL → D charon without root → E
  PKCS#11 (gated) → F app (tray, status monitor, update notice) → G release
  pipeline in CI → H opening → I acceptance and release. **Phase 0 is done
  locally** (step 49): the stale `v0.1.1` tag is gone, the history is clean.
  **The new repository exists** (private, `main` and the tags pushed by
  name on 2026-11-18) and its first CI run passed, which also confirms the
  CI fixes of step 44. The release pages of 0.1.0–0.9.0 are recreated from
  `docs/releases/` and the local packages (checked against their
  `SHA256SUMS` and with `check-private.sh --artifacts`, all clean).
  **Phase A is done** (step 50): P2, P3 and P4 closed, every download of
  the release build pinned, CI hygiene. **Next: phase B** (complete
  packages: rpm/deb scripts that manage the network component, the CLI in
  the packages, AppStream, third-party licenses).
- **Waiting for the maintainer:**
  1. A quick look at the app after the Tauri update (phase A): open it,
     connect, check the tray. A development build is in
     `target/release-artifacts/0.9.0-dev-1871e2428f69/`; `npm run
     release:build` from `main` makes a newer one.
  2. After the switch, `git config --unset remote.origin.tagOpt` (it kept a
     fetch from the old repository from bringing `v0.1.1` back).
  3. P8 (`initiator_only`) is not concluded: it needs a session longer than
     the gateway's phase 1 lifetime (the [audit report](audits/v0.2.0.md)
     has the steps). It is outside 1.0.

## Set up the environment (once)

1. Install the prerequisites from the README, "Prerequisites" section.
2. Install cargo-deny: `cargo install --locked cargo-deny`.
3. Run `npm install`, which installs the git hooks.
4. Create `.git/info/private-patterns` with your private patterns (see
   CONTRIBUTING.md).

## Check that everything is green

```bash
npm run check          # private data + shellcheck + fmt/clippy/tests/cargo-deny + biome/tsc/vitest
npm run test:engine    # engine E2E with a local IPsec gateway (~10 s)
npm run test:docker    # Rust core integration with real Docker
npm run test:cli       # full CLI flow
npm run test:native    # compiled strongSwan (embedded engine) + E2E (includes the ADR-0019 attack)
npm run test:systemd   # embedded engine with real systemd (privileged container)
npm run test:modes     # IPsec modes the profile accepts, on the same path (~3 min)
npm run test:packaging # network component installer, as root in a container
npm run test:compat    # test:systemd on seven distributions (~4 min after the first run)
```

While the network component installed on this computer is older than the
code, run `test:docker` and `test:cli` with `IPSEC_CLIENT_BACKEND=docker`: the
old helper refuses the new request, as it should.

## Rules that do not change

- No real VPN data in the repository (ADR-0010). The guard runs in the hooks.
- One branch per roadmap item and Conventional Commits in English.
- `--no-ff` merge with `npm run check` green; the branch is deleted after the
  merge (the merge commit keeps its name).
- On every relevant step, an entry in the development log. On every merge,
  update this file, the roadmap and the CHANGELOG.
