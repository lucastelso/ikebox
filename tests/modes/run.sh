#!/usr/bin/env bash
# Certification of the IPsec modes that profile schema 1 accepts, on the
# production path: ipsec-client-cli → helper (root) → embedded strongSwan,
# in the test computer of tests/systemd (Fedora with real systemd).
#
# One test gateway per mode (tests/modes/gateways/*.conf, Debian's strongSwan
# in the engine test image); the scenarios are in tests/modes/inside.sh:
#
#   - IKEv1 main mode + group PSK + XAuth (split and full tunnel);
#   - IKEv1 main mode + PSK only (with and without a virtual IP);
#   - IKEv1 aggressive mode + group PSK only;
#   - IKEv2 + PSK (gateway identity, PFS on rekey, full tunnel, no VIP);
#
# each with or without forced NAT-T (ESP in UDP) and PFS. IKEv1 aggressive
# mode + group PSK + XAuth, the original mode, is in npm run test:systemd.
#
# Requirements: the same as tests/systemd/run.sh.

set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
IPSEC_CLIENT_TEST_SUITE=modes exec "$here/../systemd/run.sh"
