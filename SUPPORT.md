# Getting help

**Português:** as issues podem ser escritas em português.

1. **Read first:** the [installation guide](docs/installation.md) (its last
   section, "Common problems"), the [quick start](docs/quick-start.md) and
   **System → Diagnostics** in the app (or `ikebox-cli doctor`): most
   problems show up there, with a hint.
2. **A connection fails:** the app explains the likely cause. If it does not
   help, open a [bug report](https://github.com/lucastelso/ikebox/issues/new/choose)
   with the **anonymized** log (**Copy anonymized** in the app, or
   `ikebox-cli logs --anonymize`): it replaces your gateway, addresses
   and domains with placeholders. Never paste a PSK, a password, a PIN or
   your organization's gateway.
3. **A security problem:** never in a public issue; see
   [SECURITY.md](SECURITY.md).
4. **Ideas and new VPN modes:** a feature request, saying which gateway
   (vendor and model, not its address) and which mode it uses.

This is a volunteer project: answers may take a few days. Questions about
your organization's VPN settings (the PSK, the group, which proposals) are
for your IT team: only they can see the gateway.
