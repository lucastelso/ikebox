# Example presets

**Generic** profiles, with addresses and names reserved for documentation
only (RFC 5737 and RFC 2606). They are a starting point and are validated by
the tests.

| File | Scenario |
|---|---|
| `exemplo-ikev1-agressivo-xauth.json` | Typical legacy VPN (e.g. FortiGate): IKEv1 aggressive, group PSK (`keyid:`) + XAuth, modp1024/768, split tunnel |
| `exemplo-ikev2-psk.json` | IKEv2 with PSK, modern proposals, IPv4 + IPv6 |

To create yours, copy an example to `presets.local/`, which git ignores, and
adjust it. The format and the mapping from `swanctl.conf` are in
[docs/profiles.md](../docs/profiles.md). Do **not** put credentials in the
file: they are entered in the app.
