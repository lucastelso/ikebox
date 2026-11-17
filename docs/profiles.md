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

## Format (schema 1)

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

Unknown fields are refused. That way, a typo, or an attempt to put the
`"psk"` in the file, does not go unnoticed.

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

## Handing the profile to a colleague

1. Export the profile from the app or the CLI. The file contains no
   credentials.
2. Send the file over a private channel, never through a public repository.
3. Pass the group PSK over a separate, safe channel.
4. The colleague imports the file and enters their own credentials: their
   XAuth username and password, plus the group PSK.
