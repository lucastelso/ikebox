# ADR-0029: Smartcards and tokens through a key agent in the person's process

- **Status:** Accepted. Phase E of the program to 1.0.0 (step 54): study,
  gate and first part of the implementation (engine, core, CLI, tests). The
  app's interface follows (E2). Amends ADR-0024 ("no smartcard/PKCS#11").
- **Date:** 2026-11-18

## Context

With a smartcard or a USB token (PKCS#11: OpenSC, YubiKey and vendor
modules), the private key never leaves the device. The maintainer put it in
the scope of 1.0 with a gate: it goes in only if it fits the static engine
(ADR-0020) without relevant new surface for root.

The study, in strongSwan 6.1.0's source:

- the `pkcs11` plugin loads the token's module with `dlopen`
  (`pkcs11_library.c`), which a fully static musl executable cannot do; and
  charon, which no longer runs as root nor in anyone's session (ADR-0028),
  would be refused by pcscd's polkit rules anyway;
- the `agent` plugin delegates signatures over the ssh-agent protocol and
  needs no library. Only `charon-cmd` and `charon-nm` use it: VICI cannot
  load such a key.

So the token stays with the **person's own process**, which can load a
PKCS#11 module, talk to pcscd and ask for the PIN.

## Decision

- **The app or the CLI holds the token.** It loads the PKCS#11 module
  (p11-kit's proxy by default, which offers every module the system set up;
  any module file can be chosen), logs in with the PIN, finds the key by the
  certificate's `CKA_ID`, and serves a minimal ssh-agent (`session::agent`:
  list identities, sign) on `/run/ipsec-client/<uid>/agent/agent.sock`
  while the VPN is up. The PIN is used for the login and dropped; it is
  never stored. Neither the key nor the PIN reaches charon.
- **The profile** marks the certificate round with `"token": true`. Which
  module and which certificate is a local choice, stored in the keyring like
  an imported certificate (`token-key`: module path and key id, public), so
  a shared profile works on any computer.
- **charon** (patch `0004`) accepts `load-key` of type `agent` with the
  public key as data, and builds the key on the socket set by root in
  `charon.plugins.vici.force_agent_socket`. The VICI client never names the
  socket, and without that option agent keys are refused. The patch also:
  - lets the `agent` plugin load without `CAP_DAC_OVERRIDE`, `CAP_SETUID`
    and `CAP_SETGID` in that mode (it wanted them to open other users'
    sockets as root; charon reaches ours through permissions);
  - signs ECDSA with the RFC 7427 schemes IKEv2 uses (DER, hash matching the
    curve as ssh-agents do), and reads the signature type of ECDSA responses
    (upstream took it for the signature: no agent ECDSA signature could be
    parsed);
  - for IKEv1 RSA (a signature over a hash charon computed, which no
    ssh-agent does), sets a private flag that only IPsec Client's agent
    understands: sign the data as it is (at most a SHA-512 hash).
- **Root** only creates the agent directory at the start of a session:
  owned by the session user, group `ipsec-client-engine`, `0750` (no setgid:
  the sandbox forbids it). The socket is `0666` inside it: only the user and
  charon's group can enter, and the agent checks `SO_PEERCRED` on every
  connection (this user, or charon's group).
- The engine builds the `agent` and `sshkey` plugins.
- The CLI: `secrets token-list`, `secrets token-use <profile>`,
  `connect --token-module/--token-id` for one connection, the PIN asked at
  every connection (or `token-pin=` on standard input); `--detach` is
  refused for token profiles, since the agent lives in that process.

## Consequences

- Smartcards and tokens work with RSA (IKEv2 and IKEv1 main mode, with
  XAuth after it) and ECDSA (IKEv2); Ed25519 is supported by the agent and
  the plugin. ECDSA in IKEv1 is not (it signs a precomputed hash too).
- **While connected, the app or the CLI must stay open:** a
  reauthentication (IKEv1 phase 1, which the gateway may start, phase D)
  needs a new signature. Phase F (close to the tray) keeps the app alive;
  the CLI stays in the foreground.
- **New surface:** charon can ask the token to sign while the session lasts
  (a signing oracle, inherent to any design where charon authenticates the
  person); a compromised charon is an unprivileged uid with `NET_ADMIN`
  (ADR-0028). The agent parses only two small messages, bounded in size.
  Root gained one `mkdir`/`chown`. The user controls the content of the
  agent directory: at worst charon connects to another socket and fails.
- The engine stays fully static; no module is ever loaded by root or by
  charon.
- Tested end to end in `test:modes` with an emulated token (SoftHSM) whose
  keys were imported from the test PKI: RSA and ECDSA in IKEv2, RSA with
  XAuth in IKEv1 main mode, a wrong PIN said as such, and the PIN searched
  for (and absent) in the journal and the files.

## Alternatives considered

- **A dynamic engine with glibc** to use the `pkcs11` plugin: undoes
  ADR-0020, and charon (not root, not in a session) still cannot reach
  pcscd without new polkit rules for a system user.
- **p11-kit's RPC** into charon: a PKCS#11 client compiled into charon,
  more code parsing input from the user's side than the two agent messages.
- **The person's own ssh-agent or gpg-agent:** their sockets live in
  `/run/user/<uid>`, which the sandbox hides, and they cannot sign a
  precomputed hash (IKEv1).
