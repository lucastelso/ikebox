# ADR-0019: Whoever talks VICI does not choose the updown or the if_id (embedded engine)

- **Status:** Accepted. Implemented in the `fix/engine-updown-forcado` branch
  (released in 0.2.0). Fixes a local privilege escalation in 0.1.0 found in
  the security audit (step 32 of the development log).
- **Date:** 2026-11-16
- **Complements:** ADR-0003 (control via VICI) and ADR-0014 (helper with polkit).

## Context

In the embedded engine, charon runs **as root**, in a transient systemd unit.
The session user talks to it directly through the VICI socket: that is how
secrets go from the app to charon without going through the helper
(ADR-0007).

VICI lets the client set, on each CHILD_SA, the **`updown`**. charon runs that
command via `/bin/sh -c` whenever a tunnel goes up or down, as root. In 0.1.0,
the app sent that command, that is, the unprivileged side. The result is a
privilege escalation to root, without a password:

1. a malicious program, running as the user of the active session, asks the
   helper to start the engine (polkit is `allow_active`);
2. it loads a connection with `updown = "<any command>"`;
3. it completes a tunnel with any gateway, possibly its own.

The unit sandbox did not stop the worst: `/proc/sys` was writable
(`kernel.core_pattern`, `kernel.modprobe`), the disks showed up in `/dev`, and
uid 0 reads root's files without any capability.

The same way, the client also chose the `if_id`. With it, it could install SAs
and policies outside the session interface.

It is not exploitable from the network or by the gateway: it needs code
running as the user. The `tests/engine/e2e.sh` test, scenario 5, reproduces
the attack. With the 0.1.0 strongSwan, the client's command runs.

## Decision

1. **A minimal strongSwan patch**
   (`native-engine/patches/0001-vici-forced-updown-if-id.patch`).
   - With `charon.plugins.vici.force_updown` set, the VICI parser replaces the
     `updown` of every CHILD_SA with the configured value and logs "ignoring
     updown command supplied by client".
   - With `force_if_id` set, it forces `if_id_in` and `if_id_out` on the
     CHILD_SA and on the connection.
   - Without the options, the behavior is upstream's.
   - The build applies the patch **after** checking the source SHA-256 + GPG
     and records its hash in `BUILDINFO`. Staging, the release build and
     `verify-release.sh` refuse an engine without the patches of the commit.
2. **Root sets the command, with arguments that never go through a shell.**
   - The entrypoint writes the options into the runtime config, which lives in
     the unit's runtime directory (0700, root):
     - `force_updown = /opt/ipsec-client/libexec/updown --args-file <file>`;
     - `force_if_id = <session if_id>`.
   - The arguments (interface and DNS) go **one per line** in a root-owned
     file. `updown` reads that file as data and validates everything again.
3. **The DNS goes through the helper.** `StartParams` gains `dns` (servers,
   domains, flags), validated in `ipsec-client-proto` with the same rules as
   the profile. In the embedded engine, `load-conn` goes without `updown`.
   - An old helper refuses the new field. The app then shows "update the
     network component", with the "Update network component" button.
4. **Defense in depth.**
   - **Engine sandbox:** `PrivateDevices`, `ProtectKernelTunables`
     (read-only `/proc/sys`), `ProtectKernelLogs`, `ProtectProc=invisible`,
     `PrivateIPC`, `UMask=0077` and `SystemCallFilter=@system-service`.
     - charon tried to set `xfrm_acq_expires`, which is global to the host, and
       now cannot. It is optional and only affects trap policies, which the app
       does not use.
   - **VICI socket:** `0600` owned by the session uid, in a `0711` directory
     owned by root. Before, it was `root:<effective gid>` `0660`, and other
     members of that group could reach it too.
   - **Helper:**
     - `Stop` and `Logs` only for the session owner (or root). The log has the
       gateway, the group id and the login;
     - `Status` hides another user's profile;
     - connections served in parallel (up to 8), with 3 s for the request to
       arrive. Before, an idle client blocked the helper.

## Consequences

- **The user no longer controls any text run as root.** What they still
  control through VICI (connections, secrets, initiate and terminate) only
  affects their own session, on its interface.
- **We maintain a patch on strongSwan.** When updating the version, the build
  fails if the patch does not apply: it must be ported. The 5.x fallback
  versions (ADR-0016) do not have the patch ported and therefore cannot be
  packaged.
- **Residual risk:**
  - charon is still root inside the sandbox. The next hardening step is to run
    it as its own user with ambient capabilities (`NET_ADMIN`,
    `NET_BIND_SERVICE`, `NET_RAW`), which depends on validating `resolvectl`
    without root;
  - a memory bug in charon exploitable by whoever talks VICI would still be
    worth root, limited by the sandbox.
- **Tests:**
  - `npm run test:native`, scenario 5: the attack, which passes with the patch
    and fails with the 0.1.0 engine;
  - `npm run test:systemd`: real systemd in a container. It covers the real
    installer, the helper, the applied sandbox, two users (bob cannot stop it,
    read its log or reach alice's VICI) and the attack.
- **Users of 0.1.0** must update the app **and** click "Update" on the network
  component. The fix lives in the component.

## Alternatives considered

- **A filtering VICI proxy in Rust.** The user would talk to a proxy, which
  would refuse `updown` and `if_id` before forwarding to charon. Discarded for
  two reasons:
  - more privileged code parsing untrusted input all the time (the event
    stream included);
  - the filter would have to follow every protocol version.

  The patch is a few lines and sits exactly where the value is read.
- **Only running charon without root.** It reduces the damage, but the
  client's `updown` would still run with `NET_ADMIN` and `NET_RAW`: capturing
  and redirecting the whole computer's traffic. It stays as the next step, on
  top of the patch.
- **Validating the `updown` text in charon (fixed prefix + allowed
  characters).** The shell grammar has pitfalls (`~` expansion, newlines).
  Forcing the value and passing the arguments as data is simpler to verify.
