# Command line: `ikebox-cli`

The CLI has the same logic as the desktop app, without a GUI. It is for using
the VPN from a terminal, in scripts, and for validating the core. It speaks
English.

## Installation

The `.rpm` and the `.deb` install it in `/usr/bin`, with a manual page
(`man ikebox-cli`) and completions for bash, zsh and fish. With the
AppImage, build it from source:

```bash
cargo build --release -p ikebox-cli
install -Dm755 target/release/ikebox-cli ~/.local/bin/ikebox-cli
# completions, for example for bash:
ikebox-cli completions bash > ~/.local/share/bash-completion/completions/ikebox-cli
```

### CLI only, without the desktop app

For servers, WSL or minimal setups: the command line and the network
component, with no Node, no WebKitGTK and no Docker
([ADR-0033](adr/0033-cli-only-and-servers.md)). It needs Rust (rustup reads
`rust-toolchain.toml`), `pkg-config`, the D-Bus headers and, for the
engine, a musl toolchain (the packages per distribution are in
[packaging.md](packaging.md#the-build)):

```bash
native-engine/build.sh --host        # the engine (or without --host, in Docker)
cargo build --release --locked -p ikebox-cli -p ikebox-helper
scripts/stage-network-component.sh \
    --engine native-engine/dist/ikebox-engine-6.1.0-x86_64.tar.gz \
    --helper target/release/ikebox-helper
install -Dm755 target/release/ikebox-cli ~/.local/bin/ikebox-cli
ikebox-cli component install --bundle target/network-component   # asks the administrator password once
ikebox-cli doctor
```

The engine version is the first line of `native-engine/versions.txt`.
Without a desktop, connecting from an SSH session needs the step in
[Servers and remote sessions](#servers-and-remote-sessions).

Usage prerequisites: the network component installed (by the app or with
`component install`), a kernel with `xfrm_interface`, systemd-resolved and,
optionally, the system keyring. Without the network component, the CLI falls
back to the Docker engine if Docker is available (development). To check
everything: `ikebox-cli doctor`.

> **Do not use `sudo`.** The CLI runs as your user. The network privileges
> live in the network component (ADR-0014).

## First use

```bash
ikebox-cli doctor                                   # is this computer ready?
ikebox-cli profiles import presets.local/vpn.json   # the profile (see docs/profiles.md)
# or, from a swanctl.conf that already works:
ikebox-cli profiles import-swanctl swanctl.conf --id vpn --dns 192.0.2.53 --domain '~corp.example.com'
ikebox-cli secrets set vpn                          # PSK, username and password → keyring
ikebox-cli connect vpn                              # Ctrl+C disconnects
```

## Commands

| Command | What it does |
|---|---|
| `doctor` | Checks the user, the network component (or Docker, as a fallback), `xfrm_interface`, resolved, D-Bus, ports 500/4500, keyring and image. Exits with 1 if something prevents connecting |
| `component status [--bundle DIR]` | Network component state: version, strongSwan and origin. With `--bundle`, says whether the bundle differs from the installed one. Exits with **3** if not installed |
| `component install --bundle DIR` | Installs or updates the component from a bundle (`npm run stage:component` builds one in `target/network-component`). The system asks for the administrator password once |
| `component uninstall` | Removes the component (asks for the administrator password). It does not remove what came from a .rpm/.deb package |
| `profiles list` | Lists the profiles, with their security level, and warns about invalid files |
| `profiles show <id>` | Shows the profile (JSON on stdout) and its security level with the reasons (stderr) |
| `profiles import <file> [--force]` | Imports a profile (validates it first; `--force` replaces an existing one) |
| `profiles export <id> <file>` | Exports the profile, without credentials, to hand it to a colleague |
| `profiles import-legacy <file> [--format pcf\|vpnc\|networkmanager\|ipsec-conf\|forticlient] [--conn] [--id] [--name] [--dns IP]... [--domain D]... [--use-received-dns] [--output file] [--force]` | Creates the profile from another client's file (Cisco `.pcf`, vpnc, NetworkManager, `ipsec.conf`, FortiClient XML). The format is detected when not given; passwords in the file are skipped without being read ([profiles.md](profiles.md#importing-from-other-vpn-clients)) |
| `profiles import-swanctl <swanctl.conf> [--id] [--name] [--dns IP]... [--domain D]... [--use-received-dns] [--output file] [--force]` | Creates the profile from an existing `swanctl.conf`. Skips the `secrets` section without reading it and does not copy the XAuth username |
| `profiles validate <file>` | Validates without importing and lists the problems field by field; a valid profile also gets its security level ([profiles.md](profiles.md#security-level)) |
| `profiles delete <id>` | Removes the profile (the credentials stay in the keyring) |
| `profiles path` | Shows the profiles directory |
| `secrets set <id> [--credentials-stdin]` | Asks for (passwords without echo) and stores the credentials in the keyring. Enter keeps the current value |
| `secrets import-p12 <id> <file> [--password-stdin]` | Imports your certificate (`.p12`/`.pfx`, modern or old) into the keyring: the private key and the certificate. The file's password is asked (or read from the first line of standard input) and never stored |
| `secrets token-list [--module <file>]` | Lists the certificates on the connected smartcards and tokens, with their key ids (no PIN needed). The module defaults to p11-kit's proxy, or OpenSC ([ADR-0029](adr/0029-smartcards-and-tokens.md)) |
| `secrets token-use <id> [--module <file>] [--id <hex>]` | For a profile whose certificate is on a smartcard or token (`"token": true`): chooses the certificate and stores the choice in the keyring (the certificate and where the key is; never the PIN) |
| `secrets status <id>` | Shows what is stored, without revealing values; the username is masked, and a client certificate shows its key type and how many days it is still valid |
| `secrets forget <id>` | Deletes the credentials from the keyring |
| `connect <id>` | Connects in the foreground. **Ctrl+C disconnects.** Tunnel drops and recoveries show up on screen |
| `connect <id> --detach` | Connects and returns to the terminal; the tunnel stays up |
| `connect <id> --remember` | Stores the credentials asked now in the keyring, once the connection is up (a wrong password is not stored) |
| `connect <id> --client-p12 <file>` | Uses that certificate for this connection only, in memory (with `--remember`, also in the keyring). The file's password is asked, or read from the `p12-password` line of `--credentials-stdin` |
| `connect <id> --token-module <file> [--token-id <hex>]` | Uses a certificate on a smartcard or token for this connection only. The PIN is asked at every connection (or read from the `token-pin` line of `--credentials-stdin`). The command must stay running while the VPN is up: it signs for it with the token (`--detach` is refused) |
| `connect <id> --timeout N` | Maximum negotiation time (default 30 s) |
| `connect <id> --privileged` | Fallback privileged container mode, Docker engine only (avoid it; see SECURITY.md) |
| `status` | Shows the active connection: gateway, virtual IP, networks, time and traffic. Exits with **3** when disconnected |
| `logs [-f] [-n N] [--anonymize]` | Engine log, with the XAuth username and the group id masked. With `--anonymize`, the gateway, IPs, DNS and domains also become placeholders (`<gateway>`, `<ip-1>`, `<domain-1>`): that is the format to send in a bug report |
| `disconnect` | Tells the gateway (`terminate`) and stops the engine. Works from any terminal |
| `-v` / `-vv` | Also shows the charon log after connecting / shows debugging |

## Credentials

Lookup order in `connect`:

1. the system keyring (`secrets set`, or `--remember`);
2. whatever is missing, it asks in the terminal;
3. with `--credentials-stdin`, it reads `key=value` lines from standard input.
   The keys are `psk`, `xauth-user` and `xauth-password`, for a profile
   with EAP `eap-identity` and `eap-password`, and `p12-password` with
   `--client-p12`. It is for
   automation; the keyring is not used in this mode.

Never pass secrets as arguments or environment variables: they show up in
`ps` and in the shell history. For automation, use a 0600 file with a
redirection:

```bash
ikebox-cli connect vpn --detach --credentials-stdin < ~/.config/my-vpn.cred
```

## Servers and remote sessions

By default only the user of the **active local session** (the person at
the computer's own screen) may connect and disconnect: an SSH session, a
WSL shell or a server without a desktop is refused with "not authorized:
only the user of the active local session can control the VPN". That
protects desktops from other users and remote logins.

On a server, an administrator can allow a group instead, with the
example rule that comes with Ikebox but is never installed
([ADR-0033](adr/0033-cli-only-and-servers.md)):

```bash
sudo groupadd --system ikebox
sudo usermod -aG ikebox <user>
# polkit 0.106 or newer (pkaction --version):
sudo install -m 0644 packaging/polkit/examples/50-ikebox-group.rules /etc/polkit-1/rules.d/
# polkit 0.105 (Ubuntu 22.04):
sudo install -m 0644 packaging/polkit/examples/50-ikebox-group.pkla /etc/polkit-1/localauthority/50-local.d/
```

Members of the group then control the VPN from any session, the same way
the local user does: each one starts their own VPN, and only the owner of
a session can stop it or read its log. Remove the file to go back to the
default.

Without a desktop there is usually no keyring: the CLI asks for the
credentials at every connection, or reads them with `--credentials-stdin`
(see [Credentials](#credentials)). WSL is not tested: it needs systemd
enabled and a kernel with `xfrm_interface`; `ikebox-cli doctor` says what is
missing.

## Common errors

`connect` reads the charon log and explains the likely cause:

| Message | What to check |
|---|---|
| "the gateway rejected phase 1 authentication" | The group PSK and `auth.local_id` (FortiGate/Cisco: `keyid:<group>`) |
| "wrong XAuth username or password" | `secrets set <id>` |
| "the gateway accepted none of the phase 1 proposals" | `ike.proposals` |
| "the gateway accepted none of the phase 2 proposals" | `child.esp_proposals` (algorithms and DH/PFS group) |
| "the gateway rejected the requested networks" | `child.remote_ts`/`local_ts` |
| "the gateway did not answer" | address, network, firewall or NAT (UDP 500/4500) |
| "UDP ports 500/4500 in use" | another IPsec VPN connected (e.g. an old container) |
| "the installed network component is from an older version" | update the component (`component install --bundle ...` or the app) |
| "not authorized: only the user of the active local session…" | an SSH, WSL or server session: see [Servers and remote sessions](#servers-and-remote-sessions) |

## Behavior

- The tunnel is independent of the terminal. Closing the terminal of a
  foreground `connect` does not bring the VPN down; use `disconnect`.
- Only one connection at a time. A second `connect` is refused and reports the
  active profile.
- The `status` exit code (0 = connected, 3 = disconnected) is meant for
  scripts.
