# Connection profiles

A profile is a JSON file that describes **the VPN**:

- gateway;
- proposals;
- routes;
- DNS.

It **never** contains credentials: no PSK, no XAuth username or password.
That is why a profile can be handed to colleagues, and each person enters
their own credentials in the app, with the option to store them in the system
keyring.

- Format: [ADR-0008](adr/0008-generic-profiles.md).
- Why `presets.local/` is not embedded in the binary:
  [ADR-0012](adr/0012-local-presets-not-embedded.md).
- Examples: [`presets/`](../presets).

## Where they live

| Location | Use |
|---|---|
| `~/.config/ipsec-client/profiles/<id>.json` | The user's profiles (directory 0700, files 0600) |
| `presets/` (in the repository) | Generic examples, with documentation addresses only |
| `presets.local/` (in the repository, **ignored by git**) | A place for your profiles with real data, before importing them |

## Format

There are two schemas. The app reads both and **writes the lowest one that
expresses the profile**: almost every profile is still written as schema 1,
so a colleague on an older version (0.5 or before) can import what you
export. Only a profile that uses something new (today: a one-time XAuth
password) is written as schema 2 ([ADR-0022](adr/0022-profile-schema-2.md)).

### Schema 1

```jsonc
{
  "schema_version": 1,
  "id": "my-vpn",                    // [a-z0-9-]: file, charon connection, keyring key
  "name": "My VPN",                  // display name
  "gateway": "vpn.example.com",      // DNS name or IP
  "ike": {
    "version": 1,                    // 1 or 2
    "aggressive": true,              // IKEv1 only
    "encap": true,                   // forces NAT-T (UDP 4500)
    "proposals": ["aes256-sha1-modp1024"],
    "rekey_time_secs": 86400,        // optional
    "dpd_delay_secs": 30             // optional (absent = no DPD)
  },
  "auth": {
    "method": "psk-xauth",           // "psk" | "psk-xauth" (IKEv1 only)
    "local_id": "keyid:group",       // optional: identity of the PSK round
    "remote_id": "vpn.example.com"   // optional: expected gateway identity
  },
  "child": {
    "name": "internal-net",          // default: "rede"
    "esp_proposals": ["aes256-sha1-modp1024"],
    "rekey_time_secs": 43200,        // optional
    "remote_ts": ["192.0.2.0/24"],   // networks reached; 0.0.0.0/0 = full tunnel
    "local_ts": [],                  // optional; empty = the virtual IP received
    "dpd_action": "restart",         // "none" | "trap" | "restart"
    "close_action": "restart"        // "none" | "trap" | "restart"
  },
  "network": {                       // optional (defaults below)
    "request_vip": true,             // asks the gateway for a virtual IP
    "mtu": 1400,
    "dns": {
      "servers": ["192.0.2.53"],     // static DNS servers
      "domains": ["~corp.example.com"], // "~x" only routes; "x" also joins the search list; "~." = everything
      "use_received": true,          // also uses the DNS servers sent by the gateway
      "default_route": false         // uses these DNS servers for names outside the domains
    }
  }
}
```

### Schema 2

The same file, except for `auth`, which lists the authentication rounds of
the client and how the gateway authenticates:

```jsonc
{
  "schema_version": 2,
  // id, name, gateway, ike, child and network as in schema 1
  "auth": {
    "local": [
      { "method": "psk", "id": "keyid:group" },   // round 1: PSK, with its identity
      { "method": "xauth", "otp": true }          // round 2 (IKEv1): XAuth username and password
    ],
    "remote": { "method": "psk", "id": "vpn.example.com" }  // "id" optional
  }
}
```

- `"otp": true` says the XAuth password is a **one-time code** (a token or an
  app on the phone): the app asks for it at every connection and never
  stores it in the keyring.
- Schema 1 `"method": "psk"` is `local: [psk]`; `"psk-xauth"` is
  `local: [psk, xauth]`; `local_id` is the `id` of the PSK round and
  `remote_id` the `id` of `remote`.
- **IKEv2 with a username and password** (EAP-MSCHAPv2, since 0.7.0,
  [ADR-0023](adr/0023-ikev2-eap-mschapv2.md)): one round
  `{ "method": "eap-mschapv2" }` (an optional `id` is the IKE identity the
  gateway expects), and the gateway proven by its certificate:

  ```jsonc
  "remote": {
    "method": "certificate",
    "id": "vpn.example.com",       // required: a name in the certificate
    "ca": ["-----BEGIN CERTIFICATE-----\n..."]  // the CA(s), in PEM
  }
  ```

  The certificate must chain to one of `ca` and carry `id`; there is no
  "accept any certificate". The CAs are public data and go to charon as
  data. They must be CA certificates, valid today, with an **RSA, ECDSA or
  Ed25519** key (ECDSA since 1.0, ADR-0027). Revocation (CRL/OCSP) is not checked
  ([SECURITY.md](../SECURITY.md)). The username and password are
  credentials, asked when connecting (`eap-identity`, `eap-password` in the
  CLI). Example: [`presets/exemplo-ikev2-eap.json`](../presets/exemplo-ikev2-eap.json).
- **Your certificate** (since 0.8.0,
  [ADR-0024](adr/0024-client-certificates.md)): one round
  `{ "method": "certificate" }` (an optional `id` is the IKE identity the
  gateway expects; by default, the certificate's subject), with the gateway
  proven by its certificate as above. In IKEv2, or in IKEv1 **main mode**,
  where an XAuth round may follow (`[certificate, xauth]`, as on Cisco ASA).
  The certificate and its private key are **not** in the profile: they come
  from a `.p12`/`.pfx` file, imported once into the system keyring (the app
  asks for it when connecting; in the CLI, `secrets import-p12` or
  `connect --client-p12`). Modern and old files (3DES/RC2 with SHA-1) are
  read; the key must be **RSA, ECDSA or Ed25519**, and the file's password is never
  stored.

Unknown fields are refused, in both schemas. That way, a typo, or an attempt
to put the `"psk"` in the file, does not go unnoticed.

## Supported modes

Every combination below is tested on the production path (CLI → helper →
embedded strongSwan 6.1.0) by `npm run test:modes` and `npm run test:systemd`,
each against a test gateway of its own:

| IKE | Mode | `auth.method` | Credentials asked | Tested with |
|---|---|---|---|---|
| 1 | aggressive (`aggressive: true`) | `psk-xauth` | group PSK, username, password | split; PFS modp1024/modp768; NAT-T |
| 1 | aggressive | `psk` | PSK | split; PFS modp1024; NAT-T |
| 1 | main (`aggressive: false`) | `psk-xauth` | group PSK, username, password | split with PFS modp2048; full tunnel with NAT-T |
| 1 | main | `psk` | PSK | with and without a virtual IP; PFS; NAT-T |
| 2 | — | `psk` | PSK | gateway identity (`remote_id`); PFS on rekey; full tunnel with NAT-T; without a virtual IP |
| 2 | — | `eap-mschapv2` (schema 2), gateway by certificate | username, password | gateway RSA certificate checked against the profile CA; wrong password, other CA and wrong gateway name diagnosed |
| 2 | — | `certificate` (schema 2), gateway by certificate | your certificate (.p12) | client and gateway RSA certificates from the same CA; a certificate from another CA is diagnosed |
| 1 | main | `certificate` + `xauth` (schema 2), gateway by certificate | your certificate (.p12, an old 3DES one), username, password | mutual RSA certificates, then XAuth |

Notes from those tests:

- **`encap: true`** forces ESP in UDP 4500 (NAT-T) even without NAT. With
  `false`, strongSwan still switches to UDP by itself when it detects NAT.
- **PFS** is a DH group in `child.esp_proposals` (e.g. `aes256-sha256-modp2048`).
  In IKEv1 it applies from the first phase 2. In IKEv2 the first CHILD_SA
  comes out of IKE_AUTH without a DH exchange: the group only shows up on
  the first rekey.
- **Without a virtual IP** (`network.request_vip: false`) the tunnel uses the
  computer's own address, and the gateway must accept it as the client's
  network. An IKEv2 gateway with an address pool usually *requires* the
  virtual IP; the app then says "the gateway requires the client to ask for a
  virtual IP".
- **`auth.remote_id`** makes charon check the identity the gateway presents.
  With a PSK it does not stop someone who has the PSK from posing as the
  gateway (see below); it matters for gateways authenticated by certificate,
  which come with profile schema 2
  ([ADR-0022](adr/0022-profile-schema-2.md)).
- XAuth only exists in IKEv1, and aggressive mode only in IKEv1: the
  validation refuses other combinations.

## Security level

The app rates each profile and shows a badge on the profile card, in the
import preview and at the top of the connection screen; the details of the
profile (and `ipsec-client-cli profiles show`/`validate`) explain each
point. **It only warns:** a weak profile still connects, because what can
change depends on what the gateway accepts.

| Level | When |
|---|---|
| **Weak** | a practical attack is known: aggressive mode with a PSK (the gateway hands a hash of the PSK to whoever knows the group id, to be guessed offline); DH groups under 2048 bits (`modp768`, `modp1024`, `modp1024s160`, `ecp192`); DES or NULL encryption; MD5 |
| **Legacy** | obsolete, with no cheap attack: IKEv1 itself (RFC 9395); a group PSK with XAuth (everyone with the group PSK can pose as the gateway and receive your username and password); 3DES, Blowfish, CAST (64-bit blocks); SHA-1; `modp1536` |
| **Good** | none of the above |

## Creating and editing in the app

**Profiles → New** opens the editor from a template, and the pencil on a
profile card edits it. The editor shows what the profile is (no raw JSON);
Rust checks every change and says, field by field, what is missing or
wrong, and the security badge updates as you type. It only saves a valid
profile.

| Template | Starts with |
|---|---|
| FortiGate | IKEv1 aggressive, forced NAT-T, group PSK + XAuth, "compatible" proposals, DPD 30 s |
| Cisco Easy VPN | IKEv1 aggressive, forced NAT-T, group PSK + XAuth, "legacy" proposals, DPD 30 s. The group name goes in the group id as `keyid:<group>` |
| IKEv2 with a PSK | IKEv2, "modern" proposals, DPD 30 s |
| IKEv2 with username and password | IKEv2, EAP-MSCHAPv2, "compatible" proposals, DPD 30 s. The gateway's name and its CA stay blank: the CA is chosen from a file or pasted (PEM), and the editor shows its subject and validity |
| IKEv2 with your certificate | IKEv2, your certificate, "compatible" proposals, DPD 30 s. As above for the gateway; your `.p12` is asked when you first connect, not in the editor. In IKEv1 the same choice works in main mode, with XAuth optional |
| Blank | IKEv2 with strongSwan's `default` proposals |

The proposals are picked by name, each with the badge it gives, or typed in
**Advanced** exactly as the gateway has them:

| Preset | Phase 1 | Phase 2 | Level |
|---|---|---|---|
| Modern (IKEv2) | `aes256gcm16-prfsha256-curve25519` | `aes256gcm16-curve25519`, `aes256gcm16` | good |
| Compatible | `aes256-sha256-modp2048`, `aes128-sha256-modp2048` | IKEv1: the same with PFS; IKEv2: `aes256-sha256-modp2048`, `aes256-sha256` | good |
| Legacy | `aes256-sha1-modp1024`, `aes128-sha1-modp1024`, `3des-sha1-modp1024` | IKEv1: the same; IKEv2: `aes256-sha1`, `aes128-sha1` | weak |

In IKEv1, every phase 2 proposal of a preset carries the same DH group:
quick mode sends a single key exchange. The identifier cannot change once
saved: it is also the key of the stored credentials. Fields the editor does
not show (rekey times, child name, local networks, DPD and close actions)
are kept as they were, or come from the template.

## Building a profile from a `swanctl.conf`

If you already have a working `swanctl.conf`, the CLI creates the profile for
you (the app does the same in **Profiles → Import**):

```bash
ipsec-client-cli profiles import-swanctl path/to/swanctl.conf \
    --id my-vpn --name "My VPN" \
    --dns 192.0.2.53 --domain '~corp.example.com'
```

- The `secrets` section is **skipped without being read**, and `xauth_id`
  (the username) is not copied. The credentials come later, with `secrets set`.
- DNS does not exist in `swanctl.conf`. Enter it with `--dns`/`--domain`, or
  use `--use-received-dns` if the gateway sends it.
- Keys without an equivalent (e.g. `updown`) produce a warning, and `include`
  is not followed.
- `--output file.json` writes the profile to a file (0600) instead of saving
  it to the list. It is useful to review it or pass it on.

The mapping used by the import, which also works for building the profile by
hand:

| `swanctl.conf` | Profile |
|---|---|
| `remote_addrs = X` | `gateway` |
| `version = 1` | `ike.version` |
| `aggressive = yes` | `ike.aggressive: true` |
| `encap = yes` | `ike.encap: true` |
| `proposals = a,b` | `ike.proposals: ["a", "b"]` |
| `rekey_time = 86400s` | `ike.rekey_time_secs: 86400` |
| `dpd_delay = 30s` | `ike.dpd_delay_secs: 30` |
| `vips = 0.0.0.0` | `network.request_vip: true` |
| `local-1 { auth = psk; id = X }` | `auth.local_id: "X"` |
| `local-2 { auth = xauth; xauth_id = ... }` | `auth.method: "psk-xauth"`. The username does **not** go in the profile: it is a credential |
| `remote { auth = psk; id = X }` | `auth.remote_id: "X"` (if there is an `id`) |
| `children { NAME { ... } }` | `child.name: "name"` (lowercase, digits and hyphens) |
| `esp_proposals = a,b` | `child.esp_proposals` |
| `rekey_time` (in the child) | `child.rekey_time_secs` |
| `remote_ts = X` | `child.remote_ts: ["X"]` |
| `dpd_action = restart` | `child.dpd_action: "restart"` |
| `close_action = start` | `child.close_action: "restart"` (in charon, `start` and `restart` are the same action) |
| `secrets { ... }` | **Nothing.** The PSK, the username and the password are entered in the app |
| DNS configured by hand (e.g. `resolvectl dns`/`domain`) | `network.dns.servers` and `network.dns.domains` |

Put the file in `presets.local/`, which never goes to git, and import it with
`ipsec-client-cli profiles import <file>`.

## Importing from other VPN clients

The app (**Profiles → Import**, or dragging the file) and the CLI
(`profiles import-legacy <file>`) also read the configuration of other
clients. The format is detected from the file name and content.

**Passwords are never read**: a group password, plain or "encrypted" (the
`.pcf` `enc_GroupPwd` uses a known, weak algorithm), an XAuth username or
password, the `[vpn-secrets]` section of NetworkManager. They are skipped
without being decrypted or copied; you type them when connecting. Paths
inside a file (a CA, a key) are not opened either.

| Format | Read | Becomes | Refused |
|---|---|---|---|
| Cisco VPN Client (`.pcf`) | `Host`, `GroupName`, `DHGroup`, `Description` | IKEv1 aggressive, `keyid:<group>`, group PSK + XAuth, AES/3DES with SHA-1 over the DH group, networks from the gateway (0.0.0.0/0) | certificates (`AuthType` 3, 5), IPsec over TCP |
| vpnc (`.conf`) | `IPSec gateway`, `IPSec ID`, `IKE DH Group`, `Perfect Forward Secrecy`, `NAT Traversal Mode`, `Domain` | the same as `.pcf`, with PFS and forced NAT-T when set | `IKE Authmode` cert/hybrid, `cisco-udp` |
| NetworkManager (`.nmconnection`) | `[vpn]` of the vpnc, strongswan (IKEv2, PSK) and libreswan plugins; `[ipv4]` DNS, search domains and routes (when it does not take the default route) | as the matching format | strongswan methods other than PSK; OpenVPN, OpenConnect, L2TP, PPTP, WireGuard |
| `ipsec.conf` (strongSwan 5, libreswan) | `conn` with `%default` and `also=`: `right`, `rightid`, `rightsubnet`, `leftid`, `authby`, `xauth`, `aggressive`/`aggrmode`, `keyexchange`/`ikev2`, `ike`, `esp`/`phase2alg`, `forceencaps`, `leftsourceip`, `dpddelay`, `dpdaction`, lifetimes | the client is `left`, the gateway `right`; the first `conn` (or `--conn`) | certificates (`rsasig`, `pubkey`, `leftcert`), EAP |
| FortiClient (XML backup) | the first IPsec connection: `server`, `mode`, `dhgroup`, `proposals`, `localid`, `xauth/enabled`, `mode_config`, `dpd`, `remote_networks`, lifetimes | IKEv1, main or aggressive, PSK + optional XAuth | certificate authentication |

Each import lists what to review (for example, a group id that Cisco or
FortiGate gateways usually expect as `keyid:<group>`). The FortiClient
importer follows FortiClient's documented XML; it was tested with files
written from that documentation, not with real exports. `test:modes`
connects through profiles imported from an `ipsec.conf` and a FortiClient
XML.

## Handing the profile to a colleague

1. Export the profile from the app or the CLI. The file contains no
   credentials.
2. Send the file over a private channel, never through a public repository.
3. Pass the group PSK over a separate, safe channel.
4. The colleague imports the file and enters their own credentials: their
   XAuth username and password, plus the group PSK.
