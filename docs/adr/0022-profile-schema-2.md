# ADR-0022: Profile schema 2: authentication rounds, trust anchors, new credentials

- **Status:** Accepted. Designed in phase 6 (step 43); the format, the
  migration and the lowest-schema writer are implemented in phase 7 (step
  45), with the `psk` and `xauth` rounds and a `psk` gateway. The EAP and
  certificate methods come with phases 8 and 9.
- **Date:** 2026-11-17

## Context

Schema 1 (ADR-0008) describes authentication with one field,
`auth.method`: `psk` (one PSK round) or `psk-xauth` (PSK round + XAuth
round, IKEv1). The gateway always authenticates with the same PSK. Phase 6
certified everything schema 1 accepts (`npm run test:modes`), and the next
modes do not fit:

- **IKEv2 + EAP** (username and password, ADR-0023): the client
  authenticates with EAP, the gateway with a **certificate**, which needs a
  trust anchor and an expected identity;
- **client certificates** (ADR-0024): the client authenticates with a
  private key that must stay out of the profile;
- the credentials also change: an EAP identity and password, a private key
  and its certificate.

What does not change: a profile never holds secrets (ADR-0007) and can be
handed to colleagues; validation lives in Rust; no client-chosen path or
command runs as root (ADR-0019).

## Decision

### Format

```jsonc
{
  "schema_version": 2,
  "id": "my-vpn", "name": "My VPN", "gateway": "vpn.example.com",
  "ike": { ... },                       // unchanged
  "auth": {
    "local": [                          // client rounds, in order (1 or 2)
      { "method": "psk", "id": "keyid:group" },
      { "method": "xauth" }
    ],
    "remote": {                         // how the gateway proves who it is
      "method": "psk",                  // "psk" | "certificate"
      "id": "vpn.example.com",          // required with "certificate"
      "ca": ["-----BEGIN CERTIFICATE-----\n..."]  // trust anchors (PEM), "certificate" only
    }
  },
  "child": { ... },                     // unchanged
  "network": { ... }                    // unchanged
}
```

A round can also say `"otp": true` (XAuth only, phase 7, ADR-0025): its
password is a one-time code, asked at every connection and never stored.

Local round methods:

| Method | IKE | Credentials (never in the profile) |
|---|---|---|
| `psk` | 1, 2 | PSK |
| `xauth` | 1, second round only | username, password |
| `eap-mschapv2` | 2 | EAP identity, password |
| `certificate` | 1, 2 | private key + certificate (ADR-0024) |

### Rules

- **The gateway is always authenticated.** `remote.method: "certificate"`
  requires `remote.id` and at least one CA in `remote.ca`. There is no
  "accept any certificate" switch, and no fallback to the system store in
  the first version (corporate gateways usually have a private CA; a public
  CA goes in `remote.ca` like any other).
- **EAP only with a certificate on the gateway side.** `eap-mschapv2`
  requires `remote.method: "certificate"`: MSCHAPv2 alone does not prove the
  gateway's identity well enough.
- `xauth` only in IKEv1 and only as the second round; `eap-*` only in IKEv2.
- The CAs in the profile are public data: they go to charon by VICI
  `load-cert` **as data**, never as a path (the `0002` patch refuses paths,
  ADR-0019 and audit P9). They are size-limited (8 certificates, 16 KiB
  each) and parsed in Rust before being sent (X.509, CA flag, not expired).
- New credential kinds in the keyring (`<profile>:<kind>`):
  `eap-identity` (not secret, masked like the XAuth username),
  `eap-password`, `client-key` and `client-cert` (ADR-0024). The secret
  path stays the same: memory → keyring → VICI.
- New identities (EAP identity, certificate subject and SANs, gateway
  `remote.id`) join the log masking and "Copy anonymized".

### Migration

- The app reads schema 1 and 2. Schema 1 is converted in memory:
  `psk` → `local: [psk{id: local_id}]`; `psk-xauth` → `local: [psk{id:
  local_id}, xauth]`; `remote: {method: psk, id: remote_id}`.
- It **writes the lowest schema that expresses the profile**: a profile that
  uses nothing new keeps being saved and exported as schema 1, so a
  colleague on an older version can still import it. Only profiles with
  EAP or certificates are written as schema 2.
- Unknown fields stay refused in both schemas.
- Snapshot tests: every schema 1 fixture produces the same `load-conn`
  before and after the change.

## Consequences

- The profile editor (phase 7) edits rounds instead of a fixed method.
- A schema 2 profile does not open on 0.5.x: the import error says which
  version is needed.
- The security level (phase 6) gains rules: a gateway authenticated by
  certificate removes the "group PSK" warning; `eap-mschapv2` is good
  only over a validated certificate, which the rules already require.

## Alternatives considered

- **Keep `auth.method` and add values** (`eap`, `cert`): every combination
  becomes a new value, and the gateway side (PSK or certificate) stays
  implicit. Rejected.
- **Copy swanctl's structure** (`local-1`, `local-2`, `remote-1`): faithful,
  but exposes too much (any key of a round) and makes validation harder.
  Rejected; the import from `swanctl.conf` maps to the new format.
- **Trust the system CA store:** convenient for public CAs, but the static
  engine has no store of its own and a store that trusts hundreds of CAs is
  a weak anchor for a VPN. Deferred.
