#!/usr/bin/env bash
# Compatibility matrix, packages: installs the .deb or the .rpm with the
# distribution's own package manager in a clean container of every
# distribution of tests/compat/distros.txt, and checks what a user gets.
#
#   tests/compat/packages.sh <dir with the packages> [--from <dir with older packages>] [distro ...]
#
# On each distribution:
#   - the package installs with only the dependencies it declares;
#   - with --from, the older package is installed first and then updated;
#   - no library of the app, the helper or charon is missing (ldd);
#   - the helper and charon start (--version);
#   - every tool the app, the installer and the engine scripts call exists.
#     A clean container has almost nothing: whatever the package needs and
#     does not declare shows up here as missing, and fails the test.
#     systemd-resolved is the exception: it is optional (VPN DNS) and only
#     reported.
#
# Nothing is installed on this computer. Requirements: Docker and network
# access to the distributions' repositories.

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
readonly LIST="$repo/tests/compat/distros.txt"

fail() {
    printf '\033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

# --- Inside the container (root) ---------------------------------------------
inside() {
    local kind=$1 missing=() tool out
    export DEBIAN_FRONTEND=noninteractive
    install_pkg() {
        case $kind in
        deb) apt-get install -y --no-install-recommends "$1" ;;
        dnf | el) dnf -y install --setopt=install_weak_deps=False "$1" ;;
        zypper) zypper -n --no-gpg-checks install --no-recommends "$1" ;;
        esac
    }
    case $kind in
    deb) apt-get update -qq ;;
    zypper) zypper -q -n ref ;;
    # RHEL family: WebKitGTK 4.1 and the tray library come from EPEL (+ CRB).
    el) dnf -y install epel-release && { dnf config-manager --set-enabled crb || true; } ;;
    esac >/dev/null 2>&1
    if [[ -n ${OLD_PKG:-} ]]; then
        install_pkg "/old/$OLD_PKG" >/tmp/old.log 2>&1 || {
            tail -n 15 /tmp/old.log
            echo "RESULT install-old=FAILED"
            exit 1
        }
    fi
    # A development build has the same version as the release it updates:
    # dnf and zypper would see nothing to do, so it is reinstalled (which
    # runs the new package's scripts, as a real update does).
    # Same format on both sides (newer rpm prints the epoch in %{NEVRA}).
    local fmt='%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}'
    if [[ $kind != deb && -n ${OLD_PKG:-} ]] &&
        [[ $(rpm -qp --qf "$fmt" "/pkg/$PKG" 2>/dev/null) == "$(rpm -q --qf "$fmt" ipsec-client 2>/dev/null)" ]]; then
        install_pkg() {
            case $kind in
            dnf | el) dnf -y reinstall "$1" ;;
            zypper) zypper -n --no-gpg-checks install --force --no-recommends "$1" ;;
            esac
        }
    fi
    install_pkg "/pkg/$PKG" >/tmp/install.log 2>&1 || {
        tail -n 15 /tmp/install.log
        echo "RESULT install=FAILED"
        exit 1
    }
    out=$(ldd /usr/bin/ipsec-client | grep "not found" || true)
    [[ -z $out ]] || { echo "$out"; echo "RESULT ldd-app=FAILED"; exit 1; }

    local net=/usr/lib/ipsec-client/network work
    work=$(mktemp -d)
    gzip -dc "$net/ipsec-client-helper.gz" >"$work/helper" && chmod +x "$work/helper"
    tar -xzf "$net/engine.tar.gz" -C "$work"
    out=$(ldd "$work/helper" "$work/opt/ipsec-client/engine/libexec/ipsec/charon" | grep "not found" || true)
    [[ -z $out ]] || { echo "$out"; echo "RESULT ldd-component=FAILED"; exit 1; }
    "$work/helper" --version >/dev/null || { echo "RESULT helper=FAILED"; exit 1; }
    STRONGSWAN_CONF="$work/opt/ipsec-client/engine/etc/strongswan.conf" \
        "$work/opt/ipsec-client/engine/libexec/ipsec/charon" --version >/dev/null ||
        { echo "RESULT charon=FAILED"; exit 1; }
    (cd "$net" && sha256sum --check --strict --quiet SHA256SUMS) ||
        { echo "RESULT bundle=FAILED"; exit 1; }

    # Called by the app (pkexec), the installer and the engine scripts.
    for tool in pkexec ip systemctl systemd-run journalctl bash tar gzip sha256sum install \
        mktemp find awk grep sed stat seq cp chmod chown id; do
        PATH=/usr/sbin:/usr/bin:/sbin:/bin command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
    done
    local resolved=present
    PATH=/usr/sbin:/usr/bin:/sbin:/bin command -v resolvectl >/dev/null 2>&1 || resolved=absent

    # The package installs the network component itself (origin "package"),
    # also as an update from a version that did not; systemd is not running
    # here, so the socket is only enabled for the next boot.
    local p=/opt/ipsec-client units=/etc/systemd/system version
    version=$(sed -n 's/^version=//p' "$net/VERSION")
    [[ $(cat "$p/ORIGIN" 2>/dev/null) == package ]] ||
        { tail -n 15 /tmp/install.log; echo "RESULT component-installed=FAILED"; exit 1; }
    [[ $(sha256sum <"$net/SHA256SUMS") == "$(sha256sum <"$p/SHA256SUMS" 2>/dev/null)" ]] ||
        { echo "RESULT component-version=FAILED"; exit 1; }
    [[ -L $units/sockets.target.wants/ipsec-client-helper.socket ]] ||
        { echo "RESULT socket-enabled=FAILED"; exit 1; }

    # Removing the package removes the component: no root helper, unit,
    # enabled socket or polkit policy may be left behind.
    case $kind in
    deb) apt-get remove -y ipsec-client ;;
    dnf | el) dnf -y remove ipsec-client ;;
    zypper) zypper -n rm ipsec-client ;;
    esac >/tmp/remove.log 2>&1 || { tail -n 15 /tmp/remove.log; echo "RESULT remove=FAILED"; exit 1; }
    local left
    left=$(ls -d "$p" "$units"/ipsec-client-helper.* "$units"/sockets.target.wants/ipsec-client-helper.socket \
        /usr/share/polkit-1/actions/io.github.lucastelso.ipsecclient.policy \
        /etc/polkit-1/actions/io.github.lucastelso.ipsecclient.policy 2>/dev/null || true)
    [[ -z $left ]] || { echo "left after removal: $left"; echo "RESULT removed-clean=FAILED"; exit 1; }
    # shellcheck source=/dev/null
    echo "RESULT ok os=\"$(. /etc/os-release && printf '%s' "$PRETTY_NAME")\" version=$version missing=\"${missing[*]:-}\" resolvectl=$resolved component=package-managed"
}

if [[ ${1:-} == --inside ]]; then
    inside "$2"
    exit 0
fi

# --- On this computer --------------------------------------------------------

# first_package DIR EXT: name of the first *.EXT file in DIR (empty if none).
first_package() {
    local -a found=()
    local file
    for file in "$1"/*."$2"; do
        [[ -f $file ]] && found+=("${file##*/}")
    done
    printf '%s' "${found[0]:-}"
}

main() {
    local dir=${1:-} old="" name base kind pkg old_pkg line out
    [[ -d $dir ]] || fail "usage: $0 <dir with the packages> [--from <dir>] [distro ...]"
    dir=$(cd "$dir" && pwd)
    shift
    if [[ ${1:-} == --from ]]; then
        old=$(cd "${2:?}" && pwd)
        shift 2
    fi
    local -a wanted=("$@") failed=()
    docker info >/dev/null 2>&1 || fail "Docker is not reachable"

    while read -r name base; do
        [[ -n $name && $name != \#* ]] || continue
        ((${#wanted[@]} == 0)) || [[ " ${wanted[*]} " == *" $name "* ]] || continue
        case $name in
        ubuntu-* | debian-*) kind=deb ;;
        fedora-*) kind=dnf ;;
        almalinux-* | rocky-*) kind=el ;;
        opensuse-*) kind=zypper ;;
        *) fail "unknown distribution family: $name" ;;
        esac
        local ext=rpm
        [[ $kind == deb ]] && ext=deb
        pkg=$(first_package "$dir" "$ext")
        [[ -n $pkg ]] || fail "no .$ext package in $dir"
        old_pkg=""
        [[ -z $old ]] || old_pkg=$(first_package "$old" "$ext")
        printf '\n\033[1;35m#### %s\033[0m (%s%s)\n' "$name" "${old_pkg:+$old_pkg → }" "$pkg"
        # The RESULT line decides; the rest is only shown on failure.
        out=$(docker run --rm --security-opt label=disable \
            -v "$dir:/pkg:ro" ${old:+-v "$old:/old:ro"} \
            -v "$repo/tests/compat/packages.sh:/packages.sh:ro" \
            -e "PKG=$pkg" -e "OLD_PKG=$old_pkg" \
            "$base" bash /packages.sh --inside "$kind" 2>&1) || true
        line=$(grep '^RESULT ' <<<"$out" | tail -n 1 || true)
        [[ $line == "RESULT ok "* ]] || grep -v '^RESULT ' <<<"$out" | tail -n 20
        if [[ $line == "RESULT ok "* && $line == *'missing=""'* ]]; then
            printf '  \033[0;32m✔\033[0m %s\n' "${line#RESULT ok }"
        elif [[ $line == "RESULT ok "* ]]; then
            printf '  \033[0;31m✘\033[0m tools not brought by the package: %s\n' "${line#RESULT ok }"
            failed+=("$name")
        else
            printf '  \033[0;31m✘\033[0m %s\n' "${line:-no result}"
            failed+=("$name")
        fi
    done <"$LIST"
    ((${#failed[@]} == 0)) || fail "failed on: ${failed[*]}"
}

main "$@"
