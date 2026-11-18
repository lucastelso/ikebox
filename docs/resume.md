# Current state and how to resume

> This file is updated **on every merge into `main`**. If work stops in the
> middle of a branch, that branch's last commit and the last entry of the
> [development log](development-log.md) say where it stopped.

## Where we are

- **Released:** 0.1.0 to 0.9.0 (the [CHANGELOG](../CHANGELOG.md) and
  `docs/releases/` have each one). 0.1.0 was the first version validated on
  a real VPN (FortiGate, IKEv1 aggressive + group PSK + XAuth), through the
  same path as an end user: the `.rpm` from the menu, the network component
  installed by the app, the embedded strongSwan without Docker.
- **Programs behind us:** the post-0.2.0 program (phases 1 to 10, steps 37
  to 48: audits, static engine, interface, IPsec modes, schema 2, EAP,
  client certificates, importers), and the
  [program to 1.0.0](roadmap.md#program-to-100-decided-on-2026-11-18) up to
  phase H (steps 49 to 58): security and supply chain, complete packages,
  OpenSSL in the engine (ECDSA, ECP), charon without root, smartcards and
  tokens, close to the tray, the optional update notice, signed releases
  built by CI, and the opening (community files, English leftovers).
- **Now: phase I.** Done: the project is **Ikebox** (step 59,
  [ADR-0031](adr/0031-renamed-to-ikebox.md)); updating from IPsec Client
  0.9.0 is tested on the seven distributions and on the real VPN (the
  maintainer's computer moved its profile and credentials and connected);
  the version is **1.0.0**, with its notes in `docs/releases/v1.0.0*.md`
  (step 60). Left for the maintainer: the release key, the repository made
  public, the tag and the publication (below).
- **Waiting for the maintainer:**
  0. **The release signing key and the `release` environment** (phase G),
     then the key's fingerprint in place of `<FINGERPRINT>` in both READMEs
     and in `docs/releases/v1.0.0*.md` (the release workflow refuses
     otherwise):
     the steps are in [release.md](release.md#signing-key-once) ("Signing
     key" and "GitHub setup"); commit `packaging/keys/ikebox-release.asc`
     and run a rehearsal (**Actions → Release → Run workflow** on `main`).
     Until then the signing job of a tag fails, by design.
  1. A development build on the real VPN (phases A, D, F): connect, check
     that the DNS of the VPN's domains resolves (it is applied by the
     engine's root part now), the tray and closing to the tray.
     `npm run release:build` from `main` makes one.
  2. `git config --unset remote.origin.tagOpt` (it kept a fetch from the
     old repository from bringing `v0.1.1` back).
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
code, run `test:docker` and `test:cli` with `IKEBOX_BACKEND=docker`: the
old helper refuses the new request, as it should.

## Rules that do not change

- No real VPN data in the repository (ADR-0010). The guard runs in the hooks.
- One branch per roadmap item and Conventional Commits in English.
- `--no-ff` merge with `npm run check` green; the branch is deleted after the
  merge (the merge commit keeps its name).
- On every relevant step, an entry in the development log. On every merge,
  update this file, the roadmap and the CHANGELOG.
