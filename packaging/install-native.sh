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
#   system user ipsec-client-engine  charon runs as it (ADR-0028)
#   /usr/share/polkit-1/actions/     io.github.lucastelso.ipsecclient.policy
#                                    (/etc/polkit-1/actions/ when /usr is
#                                    read-only: image-based distributions)

set -euo pipefail

readonly PREFIX=/opt/ipsec-client
readonly UNIT_DIR=/etc/systemd/system
readonly POLKIT_DIR=/usr/share/polkit-1/actions
# Where polkit 124 and newer also read actions from: used when /usr is
# read-only (Fedora Silverblue/Kinoite, openSUSE MicroOS and the like).
readonly POLKIT_ETC_DIR=/etc/polkit-1/actions
readonly POLKIT_ETC_SINCE=124
readonly POLICY=io.github.lucastelso.ipsecclient.policy
# System user charon runs as (ADR-0028). It is kept on removal, as system
# users usually are: its uid may still own something (it never does here).
readonly ENGINE_USER=ipsec-client-engine
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

# Whether systemd manages this system right now. It does not while a package
# is installed into an image being built, or in a container: the files and
# the socket's enablement are then put in place, and the socket starts on
# the next boot.
systemd_running() { [[ -d /run/systemd/system ]]; }

do_uninstall() {
    log "stopping and removing"
    if systemd_running; then
        systemctl stop ipsec-client-engine.service 2>/dev/null || true
        systemctl disable --now ipsec-client-helper.socket 2>/dev/null || true
        systemctl stop ipsec-client-helper.service 2>/dev/null || true
    else
        systemctl disable ipsec-client-helper.socket 2>/dev/null || true
    fi
    rm -f "$UNIT_DIR/ipsec-client-helper.socket" "$UNIT_DIR/ipsec-client-helper.service"
    rm -f "$POLKIT_DIR/$POLICY" "$POLKIT_ETC_DIR/$POLICY" 2>/dev/null || true
    rm -rf "${PREFIX:?}"
    if systemd_running; then
        systemctl daemon-reload
    fi
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
    # Only files and directories (the first column of `tar -tv` is the type):
    # a link inside it could point the extraction somewhere else.
    if tar -tvzf "$work/engine.tar.gz" | cut -c1 | grep -vq '[-d]'; then
        die "engine tarball with links or special files"
    fi
}

# Directory for the polkit policy (printed). The usual one if it can be
# written; on a read-only /usr, the one in /etc, which only polkit 124 or
# newer reads. Decided before anything is installed.
choose_polkit_dir() {
    if [[ -d $POLKIT_DIR && -w $POLKIT_DIR ]]; then
        printf '%s' "$POLKIT_DIR"
        return 0
    fi
    [[ -d $POLKIT_DIR ]] || die "$POLKIT_DIR does not exist; install polkit first"
    local version
    version=$(pkaction --version 2>/dev/null | grep -oE '[0-9]+' | head -n 1 || true)
    if [[ -n $version ]] && ((10#$version >= POLKIT_ETC_SINCE)); then
        printf '%s' "$POLKIT_ETC_DIR"
        return 0
    fi
    die "$POLKIT_DIR is read-only, and this polkit (version ${version:-unknown}) only reads policies from there; polkit $POLKIT_ETC_SINCE or newer is needed on a system with a read-only /usr"
}

# The user charon runs as: systemd-sysusers where it exists (every systemd
# distribution), useradd otherwise. No home, no shell, no password.
ensure_engine_user() {
    id -u "$ENGINE_USER" >/dev/null 2>&1 && return 0
    if command -v systemd-sysusers >/dev/null 2>&1; then
        systemd-sysusers --inline "u $ENGINE_USER - \"IPsec Client engine\" / /usr/sbin/nologin"
    else
        useradd --system --user-group --no-create-home --home-dir / \
            --shell /usr/sbin/nologin --comment "IPsec Client engine" "$ENGINE_USER"
    fi
    id -u "$ENGINE_USER" >/dev/null 2>&1 || die "could not create the system user $ENGINE_USER"
    log "system user $ENGINE_USER created"
}

do_install() {
    local bundle="" origin=manual work polkit_dir
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
    polkit_dir=$(choose_polkit_dir)
    command -v setpriv >/dev/null 2>&1 ||
        die "setpriv (util-linux) is missing: it starts the VPN engine without root"
    ensure_engine_user

    log "installing in $PREFIX"
    if systemd_running; then
        systemctl stop ipsec-client-engine.service 2>/dev/null || true
    fi
    rm -rf "$PREFIX/engine"
    install -d -m 0755 "$PREFIX" "$PREFIX/bin" "$PREFIX/libexec"
    # --keep-directory-symlink: on image-based systems /opt is a link to
    # var/opt (Fedora Silverblue/Kinoite). Without it, tar replaces the link
    # with a real directory: the files installed through the link vanish and
    # the root of the system is changed (or, read-only, the extraction fails).
    # The tarball was checked to hold only opt/ipsec-client/engine/...
    tar -xzf "$work/engine.tar.gz" -C / --no-same-owner --no-same-permissions \
        --keep-directory-symlink
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
    if [[ $polkit_dir != "$POLKIT_DIR" ]]; then
        log "$POLKIT_DIR is read-only: the polkit policy goes to $polkit_dir"
        install -d -m 0755 "$polkit_dir"
    fi
    install -m 0644 "$work/packaging/polkit/$POLICY" "$polkit_dir/$POLICY"

    # SELinux: default labels on the copied files (otherwise they may inherit
    # the label of the temporary directory they came from).
    if command -v restorecon >/dev/null 2>&1; then
        restorecon -RF "$PREFIX" "$UNIT_DIR/ipsec-client-helper.socket" \
            "$UNIT_DIR/ipsec-client-helper.service" "$polkit_dir/$POLICY" 2>/dev/null || true
    fi

    if systemd_running; then
        systemctl daemon-reload
        systemctl stop ipsec-client-helper.service 2>/dev/null || true
        systemctl enable --now ipsec-client-helper.socket
    else
        systemctl enable ipsec-client-helper.socket
        log "systemd is not running: the helper socket starts on the next boot"
    fi
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
