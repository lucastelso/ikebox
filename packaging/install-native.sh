#!/usr/bin/env bash
# Installs/removes the IPsec Client network component (embedded engine +
# privileged helper, ADRs 0013, 0014 and 0018). Runs as root.
#
# Usage:
#   install-native.sh install --bundle <dir> [--origin app|package|manual]
#   install-native.sh uninstall
#
# <dir> is the bundle built by scripts/stage-network-component.sh (the app
# ships one inside it). Origin: who installed it (the app only removes what
# did not come from a .rpm/.deb package).
#
# Installs:
#   /opt/ipsec-client/engine/        compiled strongSwan (from the tarball)
#   /opt/ipsec-client/libexec/       entrypoint.sh, updown and this installer
#   /opt/ipsec-client/bin/           ipsec-client-helper
#   /opt/ipsec-client/{VERSION,SHA256SUMS,ORIGIN}
#   /etc/systemd/system/             ipsec-client-helper.{socket,service}
#   /usr/share/polkit-1/actions/     io.github.lucastelso.ipsecclient.policy

set -euo pipefail

readonly PREFIX=/opt/ipsec-client
readonly UNIT_DIR=/etc/systemd/system
readonly POLKIT_DIR=/usr/share/polkit-1/actions
readonly POLICY=io.github.lucastelso.ipsecclient.policy
# Required bundle files (the app checks the same list).
readonly REQUIRED=(
    VERSION
    install-native.sh
    engine.tar.gz
    ipsec-client-helper.gz
    engine/entrypoint.sh
    engine/updown.sh
    engine/strongswan.d/zz-ipsec-client.conf
    packaging/systemd/ipsec-client-helper.socket
    packaging/systemd/ipsec-client-helper.service
    "packaging/polkit/$POLICY"
)

log() { printf '[network component] %s\n' "$*"; }
die() {
    printf '[network component] error: %s\n' "$*" >&2
    exit 1
}

# A single output stream (the app shows the log in the order it happened).
exec 2>&1

[[ $(id -u) -eq 0 ]] || die "run it as root (sudo/pkexec)"

do_uninstall() {
    log "stopping and removing"
    systemctl stop ipsec-client-engine.service 2>/dev/null || true
    systemctl disable --now ipsec-client-helper.socket 2>/dev/null || true
    systemctl stop ipsec-client-helper.service 2>/dev/null || true
    rm -f "$UNIT_DIR/ipsec-client-helper.socket" "$UNIT_DIR/ipsec-client-helper.service"
    rm -f "$POLKIT_DIR/$POLICY"
    rm -rf "${PREFIX:?}"
    systemctl daemon-reload
    log "removed"
}

# Copies the bundle to a root-only directory and checks the hashes there:
# what gets installed is exactly what was checked, even if the source
# changes afterwards.
verify_bundle() {
    local bundle=$1 work=$2 path
    cp -R --no-preserve=all "$bundle"/. "$work"/
    if [[ -n $(find "$work" ! -type f ! -type d -print -quit) ]]; then
        die "bundle with links or special files"
    fi
    [[ -f $work/SHA256SUMS ]] || die "bundle without SHA256SUMS"
    (cd "$work" && sha256sum --check --strict --quiet SHA256SUMS) ||
        die "corrupted bundle files (SHA256SUMS does not match)"
    local -A listed=()
    while read -r _ path; do
        listed[${path#\*}]=1
    done <"$work/SHA256SUMS"
    for path in "${REQUIRED[@]}"; do
        [[ -n ${listed[$path]:-} ]] || die "incomplete bundle: missing $path"
    done
    # The tarball may only contain opt/ipsec-client/engine/... (nothing else).
    if tar -tzf "$work/engine.tar.gz" | grep -vqE '^opt/(ipsec-client/(engine(/.*)?)?)?/?$'; then
        die "engine tarball with unexpected paths"
    fi
}

do_install() {
    local bundle="" origin=manual work
    while (($# > 0)); do
        case $1 in
        --bundle) bundle=${2:?}; shift 2 ;;
        --origin) origin=${2:?}; shift 2 ;;
        *) die "unknown argument: $1" ;;
        esac
    done
    [[ -d $bundle ]] || die "component bundle not found (--bundle)"
    case $origin in
    app | package | manual) ;;
    *) die "invalid origin: $origin" ;;
    esac

    work=$(mktemp -d)
    # shellcheck disable=SC2064 # the path is fixed now, on purpose
    trap "rm -rf '$work'" EXIT
    verify_bundle "$bundle" "$work"

    log "installing in $PREFIX"
    systemctl stop ipsec-client-engine.service 2>/dev/null || true
    rm -rf "$PREFIX/engine"
    install -d -m 0755 "$PREFIX" "$PREFIX/bin" "$PREFIX/libexec"
    tar -xzf "$work/engine.tar.gz" -C / --no-same-owner --no-same-permissions
    install -m 0644 "$work/engine/strongswan.d/zz-ipsec-client.conf" \
        "$PREFIX/engine/etc/strongswan.d/zz-ipsec-client.conf"
    install -m 0755 "$work/engine/entrypoint.sh" "$PREFIX/libexec/entrypoint.sh"
    install -m 0755 "$work/engine/updown.sh" "$PREFIX/libexec/updown"
    install -m 0755 "$work/install-native.sh" "$PREFIX/libexec/install-native.sh"
    # The helper comes compressed (see scripts/stage-network-component.sh).
    gzip -d -c "$work/ipsec-client-helper.gz" >"$work/ipsec-client-helper"
    install -m 0755 "$work/ipsec-client-helper" "$PREFIX/bin/ipsec-client-helper"
    install -m 0644 "$work/VERSION" "$PREFIX/VERSION"
    install -m 0644 "$work/SHA256SUMS" "$PREFIX/SHA256SUMS"
    printf '%s\n' "$origin" >"$PREFIX/ORIGIN"
    chmod 0644 "$PREFIX/ORIGIN"
    chown -R root:root "$PREFIX"
    chmod -R go-w "$PREFIX"

    install -m 0644 "$work/packaging/systemd/ipsec-client-helper.socket" "$UNIT_DIR/"
    install -m 0644 "$work/packaging/systemd/ipsec-client-helper.service" "$UNIT_DIR/"
    install -m 0644 "$work/packaging/polkit/$POLICY" "$POLKIT_DIR/$POLICY"

    # SELinux: default labels on the copied files (otherwise they may inherit
    # the label of the temporary directory they came from).
    if command -v restorecon >/dev/null 2>&1; then
        restorecon -RF "$PREFIX" "$UNIT_DIR/ipsec-client-helper.socket" \
            "$UNIT_DIR/ipsec-client-helper.service" "$POLKIT_DIR/$POLICY" 2>/dev/null || true
    fi

    systemctl daemon-reload
    systemctl stop ipsec-client-helper.service 2>/dev/null || true
    systemctl enable --now ipsec-client-helper.socket
    log "installed: $("$PREFIX/bin/ipsec-client-helper" --version), strongSwan $(grep -m1 '^strongswan=' "$PREFIX/engine/BUILDINFO" | cut -d= -f2) (origin: $origin)"
}

case ${1:-} in
install)
    shift
    do_install "$@"
    ;;
uninstall) do_uninstall ;;
*) die "usage: $0 install --bundle <dir> [--origin app|package|manual] | uninstall" ;;
esac
