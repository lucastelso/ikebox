# Example presets

**Generic** profiles, with addresses and names reserved for documentation
only (RFC 5737 and RFC 2606). They are a starting point and are validated by
the tests.

| File | Scenario |
|---|---|
| `example-ikev1-aggressive-xauth.json` | Typical legacy VPN (e.g. FortiGate): IKEv1 aggressive, group PSK (`keyid:`) + XAuth, modp1024/768, split tunnel |
| `example-ikev2-psk.json` | IKEv2 with PSK, modern proposals, IPv4 + IPv6 |
| `example-ikev2-eap.json` | IKEv2 with username and password (EAP-MSCHAPv2), gateway proven by its certificate (schema 2). Its CA is a fake test CA: replace it with your gateway's |

To create yours, copy an example to `presets.local/`, which git ignores, and
adjust it. The format and the mapping from `swanctl.conf` are in
[docs/profiles.md](../docs/profiles.md). Do **not** put credentials in the
file: they are entered in the app.
