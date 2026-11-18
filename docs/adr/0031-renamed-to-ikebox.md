# ADR-0031: The project is renamed Ikebox for 1.0

- **Status:** Accepted. Phase I of the program to 1.0.0 (step 59).
- **Date:** 2026-11-18

## Context

Up to 0.9 the project was called **IPsec Client**: a description, not a name.
It said nothing of what sets the project apart, it is hard to search for,
and it reads as any other IPsec client. Opening the repository and releasing
1.0 is the last moment to change it cheaply: the only installations of 0.x
are the maintainer's.

## Decision

The project is called **Ikebox**: *IKE* in a *box*, strongSwan's IKE engine
running in an isolated sandbox. The title is "Ikebox — Sandboxed IPsec VPN
client for Linux (legacy IKEv1 & IKEv2)", and the README and the app credit
strongSwan as the engine that does the IPsec work.

The name changes everywhere, not only on screen:

| Before (0.x) | From 1.0 |
|---|---|
| IPsec Client | Ikebox |
| `ipsec-client` (app), `ipsec-client-cli` | `ikebox`, `ikebox-cli` |
| crates `ipsec-client-*` | `ikebox-*` |
| `io.github.lucastelso.ipsecclient` (app id, AppStream, polkit) | `io.github.lucastelso.ikebox` |
| `/opt/ipsec-client`, `/usr/lib/ipsec-client`, `/run/ipsec-client` | `/opt/ikebox`, `/usr/lib/ikebox`, `/run/ikebox` |
| `ipsec-client-helper.{socket,service}`, `ipsec-client-engine.service` | `ikebox-helper.*`, `ikebox-engine.service` |
| system user `ipsec-client-engine` | `ikebox-engine` |
| `~/.config/ipsec-client`, keyring service `ipsec-client` | `~/.config/ikebox`, `ikebox` |
| `IPSEC_CLIENT_*` variables | `IKEBOX_*` |
| `github.com/lucastelso/ipsec-client` | `github.com/lucastelso/ikebox` |
| the shield-and-lock logo | "ik", the dot of the i a box |

The history is not rewritten: release notes, the CHANGELOG of 0.x, the
development log, the audit and the earlier ADRs keep the old name, as they
were written.

Updating from 0.x keeps everything:

- **Packages:** the `.deb` declares `Conflicts`/`Replaces: ipsec-client` and
  the `.rpm` `Obsoletes: ipsec-client`, so installing Ikebox removes IPsec
  Client in the same transaction; AppStream lists the old id in
  `<replaces>`.
- **System:** the installer of the network component removes the old one
  (units, polkit action, `/opt/ipsec-client`, the old system user),
  whoever installed it. It uses names fixed in the script, never paths read
  from the old installation.
- **The person's data:** on the first start, the app or the CLI renames
  `~/.config/ipsec-client` to `~/.config/ikebox` (profiles and settings),
  unless the new one exists or the old one is a link; then it moves each
  profile's credentials from the old keyring service to the new one. A
  marker file keeps that second step pending until it completes, so a locked
  keyring only postpones it (`ikebox_core::legacy`).

## Consequences

- The window's own preferences kept by the webview (the selected profile,
  the theme) start over: they belong to the app id.
- The engine changes (the comments and a constant of the patches), so the
  network component is updated with 1.0, as it would be anyway.
- GitHub redirects the old repository address to the renamed one; the
  update notice, new in 1.0, already uses the new one.

## Alternatives considered

- **Change only the displayed name.** Less work, but the old name would stay
  in commands, folders, the keyring and the packages, and every bug report
  would mix the two.
- **A new repository and package with no migration.** Simpler for the code,
  but it would leave a second root helper behind and lose the stored
  credentials.
