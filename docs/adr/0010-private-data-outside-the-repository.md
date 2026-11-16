# ADR-0010: No real VPN data in the repository

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

The project was born to reach the maintainer's work VPN and will be published
as open source. The repository must not contain anything that identifies the
organization or the maintainer:

- gateway;
- group id;
- internal DNS and domains;
- internal networks;
- usernames and local paths;
- credentials, obviously.

That data must be entered by the user, in the app.

## Decision

1. **Examples and tests use only values reserved for documentation:**
   - RFC 5737 IPs (`192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`);
   - RFC 2606 domains (`example.com`, `*.test`).
   Test credentials are obviously fake.
2. **Real data stays outside git:**
   - `presets.local/` is in `.gitignore`;
   - the maintainer's private material stays outside the project folder (it
     used to be listed in `.git/info/exclude`, which is local and never
     versioned).
3. **Automatic guard** (`scripts/check-private.sh`):
   - reads private patterns from `.git/info/private-patterns`, which also
     stays outside git;
   - blocks commits whose staged content or message matches a pattern;
   - before publishing, `--history` scans the whole history;
   - `--artifacts` scans the inside of release packages (since 0.1.1);
   - the output shows only the location, never the matched text, so it does
     not leak through logs.
4. **In CI**, a generic secret scanner (gitleaks) complements the guard,
   because CI does not have the private patterns.

## Consequences

- Each developer sets up their own private patterns (CONTRIBUTING.md).
- End-to-end tests with the real VPN happen only locally, with the profile in
  `presets.local/` or in the user's configuration directory.
- The automated engine test uses a fake local IPsec gateway (`tests/engine/`),
  which works for anyone and for CI.

## Alternatives considered

- **A private repository with the real data.** It prevents publishing later
  and keeps the data in the history forever.
- **Manual discipline only.** A single careless commit leaks the data.
