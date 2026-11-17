# ADR-0023: IKEv2 + EAP-MSCHAPv2, with the gateway certificate always validated

- **Status:** Accepted. Designed in phase 6 (step 43), implemented in phase 8
  (step 46, release 0.7.0) as designed, with the decisions taken at the
  start of phase 8: RSA and Ed25519 gateway certificates only (an ECDSA CA
  is refused when the profile is read), no revocation check, the CA chosen
  from a file or pasted in the editor.
- **Date:** 2026-11-17

## Context

Many current gateways (strongSwan, FortiGate, Palo Alto, Windows RRAS,
pfSense/OPNsense, MikroTik) offer IKEv2 with a username and password through
EAP-MSCHAPv2: it is what Windows, macOS and Android call "IKEv2 with a
username". In IKEv2 the gateway authenticates first, **with a certificate**,
and only then the client sends EAP. If the client does not check that
certificate, anyone in the path can pose as the gateway and collect the
MSCHAPv2 exchange, which is crackable offline.

The embedded engine (ADR-0020) has `x509`, `pem`, `pkcs1`, `pubkey` and
`gmp` (RSA), and `curve25519` (Ed25519); it has no EAP plugin and no ECDSA
(that needs OpenSSL, ADR-0024).

## Decision

- **Profile** (schema 2): `local: [{ "method": "eap-mschapv2" }]` and
  `remote: { "method": "certificate", "id": "<gateway name>", "ca": [PEM] }`.
  The validation refuses EAP without a certificate on the gateway side, and a
  certificate without `id` or CA (ADR-0022).
- **Engine:** new plugins `eap-identity`, `eap-mschapv2` and `md4` (MSCHAPv2
  needs MD4 and DES; `des` is already there). Each one gets the review below,
  an E2E scenario and its record in `BUILDINFO`.
- **Charon side:** `load-conn` with `local-1 { auth = eap-mschapv2; eap_id =
  <identity> }` and `remote { auth = pubkey; id = <remote.id>; cacerts =
  [<PEM>...] }`: the CA certificates go **as data** in the same message
  (what swanctl itself sends; the design said `load-cert`, and the
  connection's own `cacerts` turned out simpler, with the same guarantee).
  charon then checks the chain up to the profile CA and that the
  certificate carries `remote.id` (SAN). There is no option to skip it.
- **Credentials:** `eap-identity` (masked, like the XAuth username) and
  `eap-password` (secret), by `load-shared` type `EAP` with the identity as
  owner, the same path as the XAuth password.
- **Failure diagnosis:** new causes, learned from real lines of the test
  gateway the way phase 6 did for IKEv2 (each failure caused on purpose in
  `test:modes`): the gateway certificate is not trusted, its identity does
  not match `remote.id`, and the EAP password is wrong.
- **Tests:** `tests/modes/gateways/ikev2-eap.conf` with a test CA and
  gateway certificate made at test time (`pki` from the test image), a user
  with a fake password, and the cases: good certificate, certificate from
  another CA, wrong identity, wrong password.

### Surface review of the new plugins

| Plugin | What it parses from the network | Notes |
|---|---|---|
| `eap-identity` | the EAP-Identity request (empty) | trivial |
| `eap-mschapv2` | the server challenge and the success message (`S=` authenticator) | runs only after the gateway passed the certificate check |
| `md4` | nothing (hash) | used by MSCHAPv2 only |

Revocation (CRL/OCSP) is not in the first version: it would need the
`revocation` plugin with HTTP fetches from root charon. Recorded as a gap in
SECURITY.md when the mode ships.

## Consequences

- The credentials dialog asks for the EAP identity and password, like the
  XAuth fields today.
- The security level rates IKEv2 + EAP over a validated certificate as good
  (with modern proposals).
- Gateways with an ECDSA certificate need ADR-0024's OpenSSL decision first;
  the import error must say so instead of failing at connect time.

## Alternatives considered

- **EAP-TLS, EAP-PEAP, EAP-TTLS:** more plugins and TLS code in root charon
  for few gateways that need them. Deferred.
- **EAP-GTC / EAP-MD5:** send the password in the clear (GTC) or are weaker
  than MSCHAPv2. Rejected.
- **Validate the certificate against the system store:** see ADR-0022.
