# Contributing

## Rule #1: no real VPN data in the repository

This project must not contain information that identifies an organization or
a person (ADR-0010). That includes:

- gateway and internal networks;
- group id;
- internal DNS servers and domains;
- employee id or username;
- local paths (`/home/<you>`);
- and, of course, PSKs and passwords.

**In examples, tests and documentation, use only reserved values:**

| Type | Use |
|---|---|
| IPv4 | `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24` (RFC 5737) |
| IPv6 | `2001:db8::/32` (RFC 3849) |
| Domains | `example.com`, `example.org`, `*.test`, `*.invalid` (RFC 2606) |
| Test credentials | Obviously fake, for example `ipsec-client-test-psk` |

### Set up the private data guard

Create the file `.git/info/private-patterns`. It lives inside `.git/` and is
never versioned. Write **one regular expression per line**. Example:

```
# my private data (never versioned)
vpn\.mycompany\.example
203\.0\.113\.10
\bmy-employee-id\b
```

The hooks run `scripts/check-private.sh` automatically:

- on **pre-commit**, over the staged content;
- on **commit-msg**, over the message;
- on **pre-push**, over the whole history.

> Do not put PSKs or passwords in that file. It would create one more
> plain-text copy of them. The protection for secrets is architectural: they
> are never written to project files.

## License of contributions

The project is GPL-3.0-or-later (ADR-0017). By contributing, you agree that
your contribution is licensed under the same terms (*inbound = outbound*).
New dependencies need a compatible license, and `cargo deny` checks that on
the Rust side.

## Workflow

1. Create the branch from `main`, named `type/description`. Examples:
   `feat/profiles`, `fix/updown-ipv6`.
2. Make small commits following
   [Conventional Commits](https://www.conventionalcommits.org/), in English:
   ```
   feat(engine): create a dedicated XFRM interface in the entrypoint
   fix(updown): skip the route when the remote TS is the gateway itself
   docs(adr): record the decision on routing by if_id
   ```
   Accepted types: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `ci`,
   `build`, `perf`, `style`, `revert`. commitlint checks the message. (The
   history up to v0.1.1 is in Portuguese.)
3. Before merging, `npm run check` must be green.
4. Merge into `main` with `git merge --no-ff`. The branch is kept.
5. Versions follow [SemVer](https://semver.org/).

## Checks

| Command | What it does |
|---|---|
| `npm run check` | Everything: private data guard, shellcheck, fmt/clippy/test/deny (Rust) and biome/tsc/vitest (frontend) |
| `npm run lint:sh` | shellcheck on every script. Without shellcheck installed, it uses the official Docker image |
| `npm run test:engine` | End-to-end engine test with a local IPsec gateway. Needs Docker and free UDP ports 500/4500 |
| `npm run test:native` / `test:systemd` | Embedded engine end to end, and with real systemd in a container (includes the ADR-0019 attack) |
| `scripts/check-private.sh --history` | Scans the whole history. Run it before publishing |
| `scripts/check-private.sh --artifacts <dir>` | Scans the inside of release packages (private patterns, home path, hostname). Runs on its own at the end of `npm run release:build` |

## Style

- **Shell:** `bash` with `set -euo pipefail`, quotes on every expansion and
  clean shellcheck. Validate every external input (environment variables,
  arguments). Do not pipe into `grep -q` when a match matters: under
  `pipefail`, the early exit turns into SIGPIPE on the producer. Use
  `grep -q pattern < <(command)`.
- **Rust:** `cargo fmt`, `clippy -D warnings`, errors with `thiserror` in
  `core`. Never derive `Debug` on types with secrets without redaction.
- **Language:** documentation, identifiers and code comments in English
  (older comments are still being translated). User-facing messages are
  bilingual: in Rust, `tr!("English", "Português")` / `trf!(...)` from
  `ipsec_client_core::i18n`; in the UI, the dictionaries in `src/i18n`
  (`pt` must have the same shape as `en`). The CLI, the helper and the scripts
  speak English. User documentation has a Portuguese copy in `docs/pt-BR/`.
