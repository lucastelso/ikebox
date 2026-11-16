# ADR-0013: Engine embedded in the app, no Docker for the user

- **Status:** Accepted. Supersedes ADR-0001 for end users; Docker stays only in
  the tests. Adapts ADRs 0002, 0005 and 0006 (see below).
- **Date:** 2026-11-16

## Context

The project's goal is a "download and run" desktop app that replaces
FortiClient. The Docker engine (ADR-0001) has three problems for that:

- the user must install Docker;
- the user must join the `docker` group, which is equivalent to root
  (SECURITY.md);
- the image depends on Ubuntu 20.04, which is out of standard support, and the
  first build alone takes about 1 minute.

The original reason for the container was to use strongSwan 5.8.2 **with its
own crypto plugins**, without the host crypto policies. That can also be done
by **compiling strongSwan ourselves**.

The `spike/engine-nativo` spike (development log, 2026-11-16) showed that it
works:

- strongSwan 6.1.0, 6.0.7, 5.9.14 and 5.8.2 compile statically (embedded
  gmp), with their own plugins and without OpenSSL;
- charon depends **only on glibc** and the whole engine takes about 4 MB
  installed (1.8 MB compressed);
- on a simulated Fedora 45 host, all four versions passed the 26 E2E checks
  against the test gateway: IKEv1 aggressive, PSK + XAuth, modp1024 and
  **modp768 PFS**, XFRM interface, split DNS, full tunnel and recovery after
  `SIGKILL`.

## Decision

- The app ships the strongSwan we compile (`native-engine/`):
  - official tarball checked by **SHA-256 and GPG signature**;
  - Ubuntu 22.04 builder (glibc 2.35, the same base as the Tauri AppImage);
  - `--disable-shared --enable-static --enable-monolithic`;
  - explicit plugin list, no OpenSSL;
  - deterministic tarball.
- charon runs on the host, started by the privileged helper (ADR-0014) and
  with the same scripts and contract as the current engine: `entrypoint.sh`,
  `updown.sh`, VICI socket (`0600` of the uid since ADR-0019), readiness,
  `proto 241`, `if_id`.
- In `core`, the engine becomes an `EngineBackend` trait:
  - `NativeBackend` is the default;
  - `DockerBackend` is the current one, for tests and development.

### What changes in the earlier ADRs

| ADR | In the embedded engine |
|---|---|
| 0001 (Docker + 5.8.2) | Applies only to the tests (test gateway and the Docker backend) |
| 0002 (container recreated) | Becomes "process recreated": each connection starts a new charon, and the previous one exits gracefully |
| 0005 (least privilege) | The sandbox comes from systemd (`systemd-run` with minimal capabilities), see ADR-0014 |
| 0006 (image by hash) | The engine is identified by the hash of the embedded tarball |

## Consequences

- **Positive.**
  - The user only installs the app.
  - No `docker` group.
  - A 4 MB engine.
  - A current strongSwan, with its security fixes.
  - **The orphan XFRM policies limitation goes away**, because the host's
    iproute2 accepts `if_id` on delete (E2E scenario 2: 0 policies left).
- **Negative.**
  - It requires a privileged helper (ADR-0014) and maintaining the strongSwan
    build.
  - Without OpenSSL, **there are no ECP curves** (`ecp256`/`ecp384`). There
    are modp, curve25519 and AES-GCM/ChaCha20. If a gateway requires ECP, a
    static OpenSSL or wolfSSL plugin can be evaluated.

## Alternatives considered

- **Keeping Docker.** It goes against the "download and run" goal and the
  security model.
- **Using the host's strongSwan.** It runs into the crypto policies and the
  differences between distributions, which were the reason for the container.
- **Rootless Podman.** It has no `CAP_NET_ADMIN` in the host network
  namespace.
