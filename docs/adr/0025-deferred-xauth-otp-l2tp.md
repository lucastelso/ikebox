# ADR-0025: Deferred: XAuth with one-time codes, and L2TP/IPsec

- **Status:** Accepted (deferral, with the reasons recorded). Phase 6 of the
  post-0.2.0 program (step 43).
- **Date:** 2026-11-17

## Context

Phase 6 listed two legacy modes to decide on, besides EAP (ADR-0023) and
client certificates (ADR-0024): XAuth with a second factor, and L2TP over
IPsec. Both show up in old VPNs, and both cost more than they give today.

## Decision

### XAuth with a one-time code (2FA/OTP): deferred

- **Cases that already work:** a code appended to the password
  (`password123456`) or a password that is itself the code. The person types
  it in the credentials dialog and **does not** tick "remember"; nothing
  else is needed.
- **What does not work:** gateways that ask in a second XAuth exchange
  ("enter the code sent to your phone", RADIUS Access-Challenge).
  strongSwan's `xauth-generic` answers each request with the stored
  password and has no way to ask the person in the middle of a negotiation.
  It would need a new XAuth plugin talking back to the app during the
  negotiation, in root charon.
- **When to revisit:** a real gateway that needs the second exchange, with
  its log (anonymized) to design against. Until then, the documentation
  says how to use the code appended to the password, and the app gains a
  per-profile "never remember the password" option in the profile editor
  (phase 7). Done in phase 7 as `"otp": true` on the XAuth round of a
  schema 2 profile.

### L2TP/IPsec: out of scope

- It needs `xl2tpd` and `pppd` running as root next to charon, IPsec in
  transport mode, and PPP authentication (usually MSCHAPv2 again, inside
  the tunnel). That is two more daemons and parsers exposed to the gateway,
  and a second network configuration path (the `ppp0` interface) next to
  ours (XFRM interface + `updown`).
- NetworkManager already has `NetworkManager-l2tp` for those who need it.
- **When to revisit:** only with a strong request; it would be its own
  component, with its own sandbox and ADR.

## Consequences

- The modes covered after phase 7+ are: IKEv1 (main or aggressive) with PSK,
  with or without XAuth; IKEv2 with PSK, EAP-MSCHAPv2 or certificates; and
  client certificates in IKEv1 or IKEv2.
- The installation guide and SECURITY.md list what is not supported.

## Alternatives considered

- **A custom XAuth plugin now:** large change in root charon without a real
  gateway to test against. Rejected for now.
- **L2TP through NetworkManager from the app:** another backend with
  different guarantees (no forced `updown`, no if_id). Rejected.
