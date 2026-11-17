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
  **Phases 1 and 2 are done** (steps 37 and 38,
  [audit report](audits/v0.2.0.md)). Their fixes are merged, not released:
  username masking and core dumps (phase 1); the helper boundary, the log
  text coming from the gateway and the in-place installer (phase 2).
  **Next: phase 3 (compatibility).** Each phase ends by updating this line.
- **Waiting for the maintainer, from phase 2:** test `initiator_only` on the
  real VPN (audit P8) and decide the other proposals (P9 to P13).
- **Still open from before:** `chore/release` (CI builds the packages on the
  tag, rpm/deb post-install with `--origin package`, signed `SHA256SUMS`,
  acceptance on a clean VM); running charon without root (ADR-0019).
- **Pending with the maintainer:**
  1. delete the old `v0.1.1` tag on GitHub
     (`git push origin :refs/tags/v0.1.1`): it still points to the history
     from before the email rewrite;
  2. before making the repository public: ask GitHub Support to purge the old
     commits (still reachable by hash) and scan the history
     (`scripts/check-private.sh --history` + gitleaks).

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
npm run test:packaging # network component installer, as root in a container
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
