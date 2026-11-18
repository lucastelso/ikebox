# Security

**[Português](docs/pt-BR/seguranca.md)** · English

## Reporting vulnerabilities

Once the repository is public, use GitHub's **Security Advisories** ("Report
a vulnerability"). Do not open public issues for vulnerabilities.

## Threat model (summary)

### What the app protects

| Asset | Protection |
|---|---|
| PSK, XAuth username and password | Kept in memory (`secrecy`/`zeroize`) or in the system keyring. They go from the app straight to charon through the VICI socket; the network component (root) never receives them. They never go to disk, environment, arguments or logs (ADR-0007) |
| VICI control socket | Unix socket `0600`, owned by the session uid, in `/run/ipsec-client/<uid>` (`0711`, owned by root). Only the session owner talks to charon (ADRs 0003, 0014 and 0019) |
| Commands run as root | Whoever talks VICI **does not choose** the `updown` or the `if_id`: the embedded charon carries a patch that forces them to the values set by root, and the `updown` arguments go in a file, never through a shell (ADR-0019) |
| Network component (root) | Minimal socket API: accepts only validated numbers, the profile id and the DNS settings, never paths, commands or secrets. Every request is authorized by polkit (`allow_active`: only the active local session) and the uid comes from the kernel (`SO_PEERCRED`). The only interface it creates or removes is the session's (`ipsc<N>`, and only if it is an XFRM interface). Stopping the VPN and reading its log is limited to the session owner, and the log has only the lines of that session; connections are served in parallel, with a limit per user and a deadline (ADRs 0014 and 0019) |
| Host during the connection | charon runs **without root**, as the `ipsec-client-engine` system user with only `NET_ADMIN` (and no way to gain other capabilities), inside a sandboxed transient systemd service: read-only system, read-only `/proc/sys` and `/sys`, no real devices, no view of other processes, a syscall filter and no new privileges. The VPN DNS is applied by the root part of the engine, and only to the session's interface (ADRs 0014, 0019 and 0028) |
| Component installation | The installer checks `SHA256SUMS` on a root-only copy and refuses links and out-of-place paths. With the `.rpm`/`.deb`, it runs from the app's own root-owned directory: no file the user can change is executed as root. Removal only runs the root-owned installer (ADR-0018) |
| Host network | Routes, VIP and DNS only on the dedicated `ipsc0` interface, no global XFRM flush and the physical interface is not touched (ADR-0004) |
| The user's organization data | Never enters the repository; an automatic guard checks commits, the history and the inside of release packages (ADR-0010) |
| Shared logs | Identities (XAuth username, group id, gateway identity) are masked wherever charon prints them, whether or not the app knows them. "Copy anonymized" also replaces the gateway, IPs and domains with placeholders before the text leaves the app. Control characters in the log (the gateway can put text there) are replaced before it is shown or printed |
| Memory of a crashed process | No core dumps: the app, the CLI, the network component and charon run with the core limit at zero, so a crash does not write credentials or session keys to disk |

### Known and accepted risks

1. **The network component runs as root.** It is the minimum needed to
   create the tunnel (XFRM, routes, ports 500/4500). The root code lives in a
   small, tested binary that only starts strongSwan in a sandbox. Once it is
   installed, any user in the active local session can connect without a
   password, as with NetworkManager. Since 1.0, charon, the part that
   parses what comes from the network, runs as its own user with
   `NET_ADMIN` only; a small root script around it sets up the interface,
   applies the DNS and cleans up
   ([ADR-0028](docs/adr/0028-charon-without-root.md)).
2. **We compile strongSwan, with two patches of our own.** The source code
   is verified by SHA-256 and by the authors' GPG signature (ADR-0016). Both
   patches are small and security-related, and are applied after that check:
   the VICI client cannot choose the `updown` or the `if_id` (ADR-0019), nor
   make charon open a certificate file or token by path (ADR-0020). It links
   OpenSSL statically for RSA, ECDSA and the Diffie-Hellman groups
   (ADR-0027). Security fixes of strongSwan and OpenSSL arrive when the
   project publishes a new version, not through the distribution's package
   manager ([ADR-0026](docs/adr/0026-strongswan-maintenance-policy.md)). The daemon is exposed (UDP 500/4500)
   **only while the VPN is connected**.
3. **Legacy cryptography.** IKEv1 aggressive mode with PSK exposes the PSK
   hash to whoever observes the handshake, which is subject to an offline
   dictionary attack. modp768/modp1024 and SHA1 are also weak. This is a
   limitation **of the legacy VPN**, not of the app: the app can only use what
   the gateway accepts. Recommend that your organization's IT team move to
   IKEv2 with certificates or EAP.

   **A group PSK does not prove who the gateway is.** Everyone in the group
   has the same PSK, and whoever has it can pose as the gateway to another
   member: receive the XAuth username and password, and push routes and DNS
   servers. This is how IKEv1 with a group PSK and XAuth works, in any
   client. Treat the group PSK as a secret, and be wary of connecting from
   networks where someone could intercept the traffic to the gateway. The
   app says so on these profiles: their security badge is at most "legacy",
   and the profile details explain why
   ([security level](docs/profiles.md#security-level)).
4. **AppImage.** The downloaded file is the root of trust. A malicious
   program running as the user could swap files during the component
   installation (ADR-0018): root cannot read the AppImage, so the installer
   runs from a copy in the user's runtime directory. The `.rpm` and the
   `.deb` do not have that window. The AppImage also carries its own
   libraries (such
   as WebKitGTK), which do not get the distribution's security updates. Prefer
   the `.rpm` or the `.deb`.
5. **Docker engine (development and tests only).** Release builds of the
   app have no Docker engine. Development builds (`npm run dev:app`) fall
   back to it when the network component is not installed: being in the
   `docker` group is equivalent to root, and that engine uses Debian's
   strongSwan, with every plugin of the distribution. The fallback privileged
   mode (`--privileged`) is off.
6. **What other users of the same computer can see.** That a VPN is up, its
   virtual IP, routes, DNS servers and domains, and the profile id: Linux
   shows network state to every local user. They cannot read the connection
   log, talk to its strongSwan or disconnect it. The system journal keeps the
   unmasked log for administrators (root and the journal groups).
7. **IKEv1 is being retired upstream.** strongSwan 6.1.0 disables IKEv1 by
   default (the embedded build enables it) and announced its removal in a
   future release. Most of the VPNs this app was made for are IKEv1 (IKEv2
   with a PSK is supported and tested too). When upstream removes IKEv1,
   the engine stays on the last release that has it and the project applies
   upstream's security fixes to it
   ([ADR-0026](docs/adr/0026-strongswan-maintenance-policy.md)).
8. **IKEv2 with a username and password (EAP).** The gateway always proves
   itself with its certificate, checked against the CA in the profile and
   the gateway name; there is no way to skip it. Two limits, decided for
   0.7.0: **revocation is not checked** (no CRL or OCSP: that would make root
   charon fetch URLs from the internet), so a revoked gateway certificate
   still works until it expires. RSA, ECDSA and Ed25519 certificates are
   verified ([ADR-0023](docs/adr/0023-ikev2-eap-mschapv2.md),
   [ADR-0027](docs/adr/0027-openssl-in-the-engine.md)). MSCHAPv2 is only safe
   inside that checked tunnel: the app never sends it to an unverified
   gateway.
9. **Your certificate (client certificates, 0.8.0).** The private key is the
   most sensitive credential the app holds: whoever has it is you on the
   VPN until the certificate expires. It is read from the `.p12` in memory,
   stored only in the system keyring (as protected as your stored
   passwords: unlocked with your session), and sent to charon as data, never
   as a file. The `.p12` password is not stored; the app no longer needs the
   file, and you can delete it. Its names (yours, your e-mail, the
   organization) are masked in the log. RSA, ECDSA and Ed25519 keys are
   accepted; no smartcards or tokens (PKCS#11) yet
   ([ADR-0024](docs/adr/0024-client-certificates.md)).

### Out of scope

- Protecting the user's data from an already compromised host (local root)
  or from a malicious program running as the user. **In scope:** the network
  component must not be a stepping stone from that program to root
  (ADR-0019).
- Kill switch / blocking leaks outside the tunnel. The app does split
  tunneling by default.

## Development practices

- No real VPN data in the repository. Use RFC 5737/2606 values in examples
  and set up `.git/info/private-patterns` (see [CONTRIBUTING.md](CONTRIBUTING.md)).
- Pinned dependencies: Rust via `Cargo.lock` + `cargo deny`, npm via
  `package-lock.json`, base images by digest, the embedded strongSwan source by
  SHA-256 + GPG, the Docker engine strongSwan packages by exact version, and
  everything the release build downloads (rustup-init, Node.js, the AppImage
  tools) by SHA-256.
- CI runs a secret scanner (gitleaks) on every push.
- Release packages are checked before publishing: network component
  integrity, strongSwan security patches, glibc limit and private data
  (`scripts/verify-release.sh`, `scripts/check-private.sh --artifacts`).
