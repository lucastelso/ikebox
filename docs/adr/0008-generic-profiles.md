# ADR-0008: Generic profiles in versioned JSON

- **Status:** Accepted. The part about embedding `presets.local/` was
  superseded by ADR-0012.
- **Date:** 2026-11-15

## Context

The app was born for one specific VPN, but it should work with any IPsec VPN
that strongSwan supports. Teams also need to hand out a ready-made
configuration, where each person only enters their own credentials.

## Decision

- A profile is a JSON file with `schema_version` and covers:
  - **general:** name, gateway, IKE v1/v2, main/aggressive mode, `encap`
    (forces NAT-T);
  - **phase 1:** IKE proposals, lifetimes, DPD;
  - **authentication:** `psk`/`xauth`/`pubkey`/`eap` rounds, with an identity
    type (`fqdn`/`keyid`/`ip`/`email`);
  - **phase 2:** ESP proposals (with PFS), lifetimes, local and remote TS,
    `dpd_action`/`close_action`;
  - **network/DNS:** VIP (pull/push), DNS servers, domains (`~domain` for
    split DNS), use of the received DNS servers, MTU.
- Validation happens in Rust, which is the source of truth.
- The profile generates the VICI `load-conn` message and the `updown`
  arguments. Snapshot tests (insta) make sure the generation does not change by
  accident.
- Profiles live in `~/.config/ipsec-client/profiles/<id>.json` and **never**
  contain secrets (ADR-0007).
- Presets:
  - `presets/` has versioned generic examples, with documentation addresses
    (RFC 5737) and `example.com`;
  - `presets.local/` keeps presets with real data. It is in `.gitignore`, it is
    embedded in the build when it exists and it can also be imported or
    exported.
- `schema_version` migrations are explicit and tested.

## Consequences

- It works for FortiGate, Cisco, Palo Alto, strongSwan etc., as long as
  strongSwan supports the combination.
- The editor UI has to expose a lot. The "Advanced" tab hides what rarely
  changes.

## Alternatives considered

- **Keeping the raw `swanctl.conf` as the profile.** It is hard to validate
  and edit in the UI, and it mixes secrets with configuration. It stays as an
  *import* (roadmap item "import swanctl.conf").
- **TOML/YAML.** JSON is native in the frontend and in serde, and
  `schema_version` handles the format evolution.
