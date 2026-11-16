# ADR-0012: Local presets are not embedded in the binary

- **Status:** Accepted (supersedes the part of ADR-0008 about embedding
  `presets.local/` in the build)
- **Date:** 2026-11-16

## Context

ADR-0008 planned that `presets.local/`, ignored by git and holding the real
profiles of the user's organization, would be embedded in the binary when it
existed at build time. That way, a local build would already come with the
work profile.

The problem is that any locally compiled binary or package (rpm, deb,
AppImage) would carry the gateway, the group id, internal networks and DNS.
That binary could be published by mistake in a release, attached to an issue
or sent to someone outside. The private data would leave without anyone seeing
it in a diff. That is exactly what ADR-0010 wants to prevent.

## Decision

- The binary only embeds the generic examples from `presets/`, when useful,
  never `presets.local/`.
- Real profiles reach the app by **importing a file** (JSON or
  `swanctl.conf`). To hand them to a team, the way is to **export** the
  profile, which contains no credentials, and send it over a private channel.
- `presets.local/` stays in `.gitignore`, as a convenient place for the real
  files before importing them.

## Consequences

- No build artifact contains organization data.
- One extra step for the user: importing the profile once. In the UI, that is
  a button.

## Alternatives considered

- **Embedding with a warning.** It depends on discipline in every release,
  and a single slip leaks the data.
- **Embedding only in debug builds.** Debug builds also circulate.
