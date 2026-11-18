# Architecture decision records (ADRs)

Each ADR records **one** relevant decision: the context, what was decided, the
consequences and the discarded alternatives. ADRs are not edited after they
are accepted. If a decision changes, a new ADR supersedes it and the old one
becomes `Superseded by ADR-XXXX`.

The ADRs were written in Portuguese up to version 0.1.1 and translated to
English in 0.2.0.

| # | Decision | Status |
|---|---|---|
| [0001](0001-docker-engine-strongswan-5.8.2.md) | Engine in a Docker container with strongSwan 5.8.2 | Superseded by 0013 (tests only) |
| [0002](0002-container-always-recreated.md) | Container recreated on every connection | Accepted |
| [0003](0003-control-via-vici.md) | charon controlled via VICI | Accepted |
| [0004](0004-routing-by-xfrm-interface.md) | Routing and DNS on a dedicated XFRM interface | Accepted |
| [0005](0005-least-privilege.md) | Least privilege in the container | Accepted |
| [0006](0006-image-versioned-by-hash.md) | Engine image embedded and versioned by hash | Accepted |
| [0007](0007-secrets.md) | Secrets only in memory or in the system keyring | Accepted |
| [0008](0008-generic-profiles.md) | Generic profiles in versioned JSON | Accepted |
| [0009](0009-typed-ipc-tauri-specta.md) | Typed IPC between the UI and Rust with tauri-specta | Accepted |
| [0010](0010-private-data-outside-the-repository.md) | No real VPN data in the repository | Accepted |
| [0011](0011-own-vici-client.md) | Our own VICI client instead of `rsvici` | Accepted |
| [0012](0012-local-presets-not-embedded.md) | Local presets are not embedded in the binary | Accepted |
| [0013](0013-embedded-engine-without-docker.md) | Engine embedded in the app, no Docker for the user | Accepted |
| [0014](0014-privileged-helper-polkit.md) | Privileged helper, authorized once (polkit + systemd) | Accepted |
| [0015](0015-distribution-appimage-rpm-deb.md) | Distribution as AppImage, rpm and deb (no Flatpak) | Accepted |
| [0016](0016-compiled-strongswan-6.1.md) | strongSwan 6.1.0 in the embedded engine, with 5.8.2 as a fallback | Accepted |
| [0017](0017-gpl-3-license.md) | GPL-3.0-or-later license | Accepted |
| [0018](0018-component-installed-by-the-app.md) | The app installs the network component (embedded bundle + pkexec) | Accepted |
| [0019](0019-forced-updown-and-if-id.md) | Whoever talks VICI does not choose the updown or the if_id (embedded engine) | Accepted |
| [0020](0020-static-engine-and-lighter-builds.md) | Static engine with musl, no Docker engine in release builds | Accepted |
| [0021](0021-interface-state-color.md) | Interface colored by the connection state (direction B) | Accepted |
| [0022](0022-profile-schema-2.md) | Profile schema 2: authentication rounds, trust anchors, new credentials | Accepted (design) |
| [0023](0023-ikev2-eap-mschapv2.md) | IKEv2 + EAP-MSCHAPv2, with the gateway certificate always validated | Accepted (design) |
| [0024](0024-client-certificates.md) | Client certificates: PKCS#12 import, key only in the keyring, sent as data | Accepted (design) |
| [0025](0025-deferred-xauth-otp-l2tp.md) | Deferred: XAuth with one-time codes, and L2TP/IPsec | Accepted (deferral) |
| [0026](0026-strongswan-maintenance-policy.md) | strongSwan maintenance policy, with IKEv1 kept working | Accepted |
| [0027](0027-openssl-in-the-engine.md) | OpenSSL in the embedded engine (ECDSA and ECP groups), replacing GMP | Accepted |
| [0028](0028-charon-without-root.md) | charon runs without root, with the network capabilities only | Accepted |
| [0029](0029-smartcards-and-tokens.md) | Smartcards and tokens through a key agent in the person's process | Accepted |

Template for new ADRs: copy an existing one and keep the sections
**Context · Decision · Consequences · Alternatives considered**.
