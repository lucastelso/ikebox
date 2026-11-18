#!/usr/bin/env bash
# Test of the network component installer (packaging/install-native.sh).
#
# Builds a real bundle with scripts/stage-network-component.sh, but with a FAKE
# engine and helper (fast, nothing to compile), and runs the installer as root
# in a throwaway Ubuntu container with a fake systemctl. Nothing is installed
# on the computer running the test.
#
# Scenarios: install, reinstall with another origin, remove, and refuse a
# tampered bundle, a symlink, an incomplete bundle, a tarball outside
# /opt/ipsec-client/engine or with a link inside, and running without root. In a second container
# set up like an image-based distribution (Fedora Silverblue/Kinoite): a
# read-only policy directory, where the policy goes to /etc with a new enough
# polkit and an old polkit is refused before anything is installed; and /opt
# as a link to /var/opt, which must stay a link.

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
# Same pinned base as the engine builder.
readonly IMAGE=ubuntu:22.04@sha256:9abcaffc92a5279fff3a6a41635c0800ea33e528425fb389658806476e7defde

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }
fail() {
    printf '  \033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

# --- Inside the container (root) ---------------------------------------------

# Runs the installer and requires a failure with the given message.
expect_refusal() {
    local why=$1 pattern=$2 bundle=$3 out
    if out=$(bash "$bundle/install-native.sh" install --bundle "$bundle" --origin app 2>&1); then
        fail "$why: the installer accepted it"
    fi
    grep -q -- "$pattern" <<<"$out" || fail "$why: unexpected message: $out"
    [[ ! -e /opt/ipsec-client ]] || fail "$why: installed anyway"
    ok "$why: refused"
}

# Writable copy of the bundle, to tamper with.
copy_bundle() {
    local dest
    dest=$(mktemp -d)
    cp -R /bundle/. "$dest"/
    printf '%s' "$dest"
}

resum() {
    # shellcheck disable=SC2094 # find excludes SHA256SUMS itself
    (cd "$1" && find . -type f ! -name SHA256SUMS -printf '%P\n' | LC_ALL=C sort |
        xargs -d '\n' sha256sum >SHA256SUMS)
}

inside() {
    # Fake systemctl: records the calls.
    cat >/usr/local/bin/systemctl <<'STUB'
#!/bin/sh
echo "$*" >>/tmp/systemctl.log
STUB
    chmod 0755 /usr/local/bin/systemctl
    mkdir -p /etc/systemd/system /usr/share/polkit-1/actions

    step "install without systemd running (image build, container)"
    bash /bundle/install-native.sh install --bundle /bundle --origin package >/dev/null
    grep -qx "enable ipsec-client-helper.socket" /tmp/systemctl.log ||
        fail "socket not enabled for the next boot"
    if grep -qE "daemon-reload|--now|^stop" /tmp/systemctl.log; then
        fail "talked to a systemd that is not running: $(tr '\n' ';' </tmp/systemctl.log)"
    fi
    bash /opt/ipsec-client/libexec/install-native.sh uninstall >/dev/null
    [[ ! -e /opt/ipsec-client ]] || fail "not removed without systemd"
    ok "files installed, socket enabled for the next boot, removed again"
    : >/tmp/systemctl.log
    # From here on, systemd manages the system.
    mkdir -p /run/systemd/system

    step "install (origin app)"
    bash /bundle/install-native.sh install --bundle /bundle --origin app
    local p=/opt/ipsec-client
    [[ -x $p/bin/ipsec-client-helper && -x $p/libexec/entrypoint.sh && -x $p/libexec/updown ]] ||
        fail "binaries/scripts missing"
    [[ -x $p/libexec/install-native.sh ]] || fail "installer not copied"
    [[ -f $p/engine/etc/strongswan.d/zz-ipsec-client.conf ]] || fail "configuration missing"
    [[ $(cat "$p/ORIGIN") == app ]] || fail "wrong ORIGIN"
    cmp -s /bundle/SHA256SUMS "$p/SHA256SUMS" || fail "SHA256SUMS not recorded"
    cmp -s /bundle/VERSION "$p/VERSION" || fail "VERSION not recorded"
    [[ $(stat -c %U "$p/bin/ipsec-client-helper") == root ]] || fail "wrong owner"
    [[ -z $(find "$p" -perm /022 -print -quit) ]] || fail "file writable by others"
    [[ -f /etc/systemd/system/ipsec-client-helper.socket ]] || fail "unit missing"
    [[ -f /usr/share/polkit-1/actions/io.github.lucastelso.ipsecclient.policy ]] ||
        fail "policy missing"
    grep -q "enable --now ipsec-client-helper.socket" /tmp/systemctl.log ||
        fail "socket not enabled"
    ok "files, owners, permissions, units, policy and version record"

    step "reinstall (origin manual)"
    bash /bundle/install-native.sh install --bundle /bundle --origin manual >/dev/null
    [[ $(cat "$p/ORIGIN") == manual ]] || fail "ORIGIN not updated"
    ok "reinstalling replaces the previous one"

    step "remove (with the copied installer)"
    bash "$p/libexec/install-native.sh" uninstall >/dev/null
    [[ ! -e $p && ! -e /etc/systemd/system/ipsec-client-helper.socket ]] ||
        fail "something was left behind"
    [[ ! -e /usr/share/polkit-1/actions/io.github.lucastelso.ipsecclient.policy ]] ||
        fail "policy not removed"
    ok "completely removed"

    step "refused bundles"
    local b
    b=$(copy_bundle)
    echo "# adulterado" >>"$b/engine/updown.sh"
    expect_refusal "tampered file" "does not match" "$b"

    b=$(copy_bundle)
    ln -sf /etc/shadow "$b/packaging/polkit/io.github.lucastelso.ipsecclient.policy"
    expect_refusal "symlink" "links" "$b"

    b=$(copy_bundle)
    rm "$b/packaging/systemd/ipsec-client-helper.service"
    resum "$b"
    expect_refusal "incomplete bundle" "missing packaging/systemd/ipsec-client-helper.service" "$b"

    b=$(copy_bundle)
    local evil
    evil=$(mktemp -d)
    mkdir -p "$evil/opt/ipsec-client/engine" "$evil/etc/cron.d"
    echo x >"$evil/etc/cron.d/evil"
    tar -czf "$b/engine.tar.gz" -C "$evil" opt etc
    resum "$b"
    expect_refusal "tarball outside the engine" "unexpected paths" "$b"

    b=$(copy_bundle)
    evil=$(mktemp -d)
    mkdir -p "$evil/opt/ipsec-client/engine"
    ln -s /etc "$evil/opt/ipsec-client/engine/etc-link"
    tar -czf "$b/engine.tar.gz" -C "$evil" opt
    resum "$b"
    expect_refusal "link inside the engine tarball" "links or special files" "$b"

    if bash /bundle/install-native.sh install --bundle /bundle --origin root >/dev/null 2>&1; then
        fail "invalid origin accepted"
    fi
    [[ ! -e /opt/ipsec-client ]] || fail "invalid origin: installed anyway"
    ok "invalid origin: refused"

    if setpriv --reuid=65534 --regid=65534 --clear-groups \
        bash /bundle/install-native.sh install --bundle /bundle >/dev/null 2>&1; then
        fail "ran without root"
    fi
    ok "without root: refused"
}

# Image-based system: read-only /usr/share/polkit-1/actions (mounted
# read-only by the host part) and /opt -> var/opt.
inside_readonly() {
    rm -rf /opt
    mkdir -p /var/opt
    ln -s var/opt /opt
    cat >/usr/local/bin/systemctl <<'STUB'
#!/bin/sh
echo "$*" >>/tmp/systemctl.log
STUB
    chmod 0755 /usr/local/bin/systemctl
    mkdir -p /etc/systemd/system
    local policy=io.github.lucastelso.ipsecclient.policy out version
    if touch /usr/share/polkit-1/actions/probe 2>/dev/null; then
        fail "the test needs a read-only /usr/share/polkit-1/actions"
    fi
    # Fake pkaction: prints the version in /tmp/polkit-version.
    cat >/usr/local/bin/pkaction <<'STUB'
#!/bin/sh
echo "pkaction version $(cat /tmp/polkit-version)"
STUB
    chmod 0755 /usr/local/bin/pkaction

    step "read-only policy directory, polkit that only reads /usr/share"
    for version in 0.105 122; do
        echo "$version" >/tmp/polkit-version
        if out=$(bash /bundle/install-native.sh install --bundle /bundle --origin app 2>&1); then
            fail "polkit $version: the installer went ahead"
        fi
        grep -q "polkit 124 or newer is needed" <<<"$out" || fail "polkit $version: unexpected message: $out"
        [[ ! -e /opt/ipsec-client && ! -e /etc/polkit-1/actions/$policy ]] ||
            fail "polkit $version: something was installed"
    done
    ok "polkit 0.105 and 122: refused with the reason, nothing installed"

    step "read-only policy directory, polkit 124 or newer"
    for version in 124 127; do
        echo "$version" >/tmp/polkit-version
        bash /bundle/install-native.sh install --bundle /bundle --origin app >/tmp/install.log 2>&1 || {
            cat /tmp/install.log
            fail "polkit $version: the installer failed"
        }
        [[ -f /etc/polkit-1/actions/$policy ]] || fail "polkit $version: policy not in /etc/polkit-1/actions"
        [[ -L /opt && $(readlink /opt) == var/opt ]] ||
            fail "polkit $version: the /opt link was replaced by the installation"
        [[ -x /var/opt/ipsec-client/bin/ipsec-client-helper &&
            -f /var/opt/ipsec-client/engine/libexec/ipsec/charon ]] ||
            fail "polkit $version: the component is not in /var/opt/ipsec-client"
        [[ $(stat -c '%a %U' "/etc/polkit-1/actions/$policy") == "644 root" ]] ||
            fail "polkit $version: wrong mode or owner of the policy"
        grep -q "goes to /etc/polkit-1/actions" /tmp/install.log || fail "polkit $version: the log does not say where the policy went"
        bash /opt/ipsec-client/libexec/install-native.sh uninstall >/dev/null
        [[ ! -e /opt/ipsec-client && ! -e /etc/polkit-1/actions/$policy ]] ||
            fail "polkit $version: something was left behind"
    done
    ok "polkit 124 and 127: policy installed in /etc/polkit-1/actions and removed with the component"
    ok "/opt stays a link to var/opt: the component lands in /var/opt/ipsec-client"
}

# --- On the host ---------------------------------------------------------------

main() {
    command -v docker >/dev/null || fail "Docker is required for this test"
    local work
    work=$(mktemp -d)
    # shellcheck disable=SC2064 # path fixed now
    trap "rm -rf '$work'" EXIT

    step "building a bundle with a fake engine and helper"
    mkdir -p "$work/root/opt/ipsec-client/engine/etc/strongswan.d" \
        "$work/root/opt/ipsec-client/engine/libexec/ipsec"
    # BUILDINFO declares the patches of this commit, like a really compiled
    # engine: without it, the production script refuses the engine (ADR-0019).
    local patches=() patch_file
    for patch_file in "$repo"/native-engine/patches/*.patch; do
        patches+=("$(basename "$patch_file"):$(sha256sum "$patch_file" | cut -c1-64)")
    done
    printf 'strongswan=0.0.0-teste\npatches=%s\n' "${patches[*]}" \
        >"$work/root/opt/ipsec-client/engine/BUILDINFO"
    printf '#!/bin/sh\nexit 0\n' >"$work/root/opt/ipsec-client/engine/libexec/ipsec/charon"
    chmod 0755 "$work/root/opt/ipsec-client/engine/libexec/ipsec/charon"
    tar -czf "$work/engine.tar.gz" -C "$work/root" opt
    printf '#!/bin/sh\necho "ipsec-client-helper 0.0.0-teste"\n' >"$work/helper"
    chmod 0755 "$work/helper"
    "$repo/scripts/stage-network-component.sh" --out "$work/bundle" \
        --engine "$work/engine.tar.gz" --helper "$work/helper" >/dev/null
    ok "bundle built by the production script"

    docker run --rm --network none \
        -v "$work/bundle:/bundle:ro" \
        -v "$repo/tests/packaging/install-test.sh:/test.sh:ro" \
        "$IMAGE" bash /test.sh --inside
    mkdir -p "$work/readonly-actions"
    docker run --rm --network none --security-opt label=disable \
        -v "$work/bundle:/bundle:ro" \
        -v "$work/readonly-actions:/usr/share/polkit-1/actions:ro" \
        -v "$repo/tests/packaging/install-test.sh:/test.sh:ro" \
        "$IMAGE" bash /test.sh --inside-readonly
    printf '\n\033[1;32m✔ network component installer OK\033[0m\n'
}

case ${1:-} in
--inside) inside ;;
--inside-readonly) inside_readonly ;;
*) main ;;
esac
