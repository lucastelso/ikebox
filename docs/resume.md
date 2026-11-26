# Current state and how to resume

> This file is updated **on every merge into `main`**. If work stops in the
> middle of a branch, that branch's last commit and the last entry of the
> [development log](development-log.md) say where it stopped.

## Where we are

- **Ikebox 1.0.0 is out** (2026-11-18): the first stable release, built,
  signed and attested by CI, published at
  [github.com/lucastelso/ikebox/releases](https://github.com/lucastelso/ikebox/releases).
  The repository is public. Up to 0.9 the project was called IPsec Client
  ([ADR-0031](adr/0031-renamed-to-ikebox.md)); 0.1.0 to 0.9.0 keep their
  release pages.
- **Programs behind us:** the post-0.2.0 program (phases 1 to 10) and the
  [program to 1.0.0](roadmap.md#program-to-100-decided-on-2026-11-18)
  (phases 0, A–I, steps 49 to 62).
- **Next:** nothing is planned yet. Candidates are in
  [roadmap → After 1.0](roadmap.md#after-10); a new program starts with the
  maintainer's choice. A patch release follows [release.md](release.md):
  `scripts/set-version.sh`, CHANGELOG, notes, tag, approval, local checks,
  publish.
- **Waiting for the maintainer:** P8 (`initiator_only`) is not concluded: it
  needs a session longer than the gateway's phase 1 lifetime (the
  [audit report](audits/v0.2.0.md) has the steps).

## Set up the environment (once)

1. Install the prerequisites from the README, "Prerequisites" section: both
   lists, "To build" and "To contribute" (cargo-deny included).
2. Run `npm ci`, which also installs the git hooks.
3. Create `.git/info/private-patterns` with your private patterns (see
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
