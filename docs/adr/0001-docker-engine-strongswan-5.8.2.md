# ADR-0001: Engine in a Docker container with strongSwan 5.8.2

- **Status:** Superseded by ADR-0013 for end users. Docker is still used in the tests.
- **Date:** 2026-11-15

## Context

Legacy IPsec VPNs (IKEv1, aggressive mode, group PSK + XAuth, DH
modp768/modp1024, SHA1) are still common on corporate concentrators. On
current Linux distributions, connecting to them usually fails: the system
crypto policies block the old algorithms, and the NetworkManager plugins do
not handle this authentication combination well.

A manual flow was already validated in production by the maintainer: the
strongSwan 5.8.2 of Ubuntu 20.04 runs in a container with `--network host`.
The IKE daemon (charon) negotiates inside the container and installs the SAs
and policies in the **host** kernel, via XFRM netlink.

## Decision

- The "engine" is a Docker image based on `ubuntu:20.04`, **pinned by
  digest**, with the strongSwan packages pinned to `5.8.2-1ubuntu3.6`.
- The package list is the same as in the validated flow. The only addition is
  `systemd` (without recommends), to get `resolvectl` (see ADR-0004).
- The container uses `--network host`. IPsec itself (ESP) is done by the host
  kernel.

## Consequences

- **Positive.**
  - It works with legacy VPNs without weakening the host crypto.
  - The behavior is reproducible: the same strongSwan version runs on any
    distribution.
  - The host does not get IPsec packages installed.
- **Negative.**
  - It requires Docker, and the user must be in the `docker` group, which is
    equivalent to root privileges (see SECURITY.md).
  - Ubuntu 20.04 is out of standard support. New strongSwan security fixes
    may not reach the public archive, so the exposure is limited in time
    (ADR-0002) and in surface (ADR-0005).
  - When the base digest or the package versions change, the image build
    fails on purpose instead of silently changing.

## Alternatives considered

- **The host's strongSwan/libreswan.** It requires loosening the crypto
  policies of the whole system, and versions vary between distributions.
- **NetworkManager-strongswan/l2tp.** It does not handle group PSK + XAuth in
  aggressive mode well.
- **Compiling a static strongSwan 5.x on the host.** More maintenance and
  more build surface. It loses the validation already done with the Ubuntu
  package.
- **Rootless Podman.** It cannot configure XFRM in the host network namespace
  without equivalent privileges. To be evaluated in the future.
