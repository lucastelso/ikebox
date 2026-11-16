#!/usr/bin/env bash
# Instala/remove o componente de rede do IPsec Client (engine embutido + helper
# com privilégio, ADRs 0013/0014). Roda como root.
#
# Uso:
#   sudo packaging/install-native.sh install --engine <tarball> --helper <binário> [--repo <dir>]
#   sudo packaging/install-native.sh uninstall
#
# Instala:
#   /opt/ipsec-client/engine/        strongSwan compilado (do tarball)
#   /opt/ipsec-client/libexec/       entrypoint.sh e updown
#   /opt/ipsec-client/bin/           ipsec-client-helper
#   /etc/systemd/system/             ipsec-client-helper.{socket,service}
#   /usr/share/polkit-1/actions/     io.github.lucastelso.ipsecclient.policy
#
# O mesmo script é usado pelo AppImage no primeiro uso (via pkexec).

set -euo pipefail

readonly PREFIX=/opt/ipsec-client
readonly UNIT_DIR=/etc/systemd/system
readonly POLKIT_DIR=/usr/share/polkit-1/actions
readonly POLICY=io.github.lucastelso.ipsecclient.policy

log() { printf '[componente de rede] %s\n' "$*"; }
die() {
    printf '[componente de rede] erro: %s\n' "$*" >&2
    exit 1
}

[[ $(id -u) -eq 0 ]] || die "rode como root (sudo/pkexec)"

uninstall() {
    log "parando e removendo"
    systemctl stop ipsec-client-engine.service 2>/dev/null || true
    systemctl disable --now ipsec-client-helper.socket 2>/dev/null || true
    systemctl stop ipsec-client-helper.service 2>/dev/null || true
    rm -f "$UNIT_DIR/ipsec-client-helper.socket" "$UNIT_DIR/ipsec-client-helper.service"
    rm -f "$POLKIT_DIR/$POLICY"
    rm -rf "${PREFIX:?}"
    systemctl daemon-reload
    log "removido"
}

install() {
    local engine="" helper="" repo
    repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
    while (($# > 0)); do
        case $1 in
        --engine) engine=${2:?}; shift 2 ;;
        --helper) helper=${2:?}; shift 2 ;;
        --repo) repo=${2:?}; shift 2 ;;
        *) die "argumento desconhecido: $1" ;;
        esac
    done
    [[ -f $engine ]] || die "tarball do engine não encontrado (--engine)"
    [[ -x $helper ]] || die "binário do helper não encontrado (--helper)"
    for f in engine/entrypoint.sh engine/updown.sh engine/strongswan.d/zz-ipsec-client.conf \
        packaging/systemd/ipsec-client-helper.socket packaging/systemd/ipsec-client-helper.service \
        "packaging/polkit/$POLICY"; do
        [[ -f $repo/$f ]] || die "arquivo ausente: $repo/$f"
    done
    # O tarball só pode conter opt/ipsec-client/engine/... (nada fora disso).
    if tar -tzf "$engine" | grep -vqE '^opt/(ipsec-client/(engine(/.*)?)?)?/?$'; then
        die "tarball do engine com caminhos inesperados"
    fi

    log "instalando em $PREFIX"
    systemctl stop ipsec-client-engine.service 2>/dev/null || true
    rm -rf "$PREFIX/engine"
    install -d -m 0755 "$PREFIX" "$PREFIX/bin" "$PREFIX/libexec"
    tar -xzf "$engine" -C / --no-same-owner
    install -m 0644 "$repo/engine/strongswan.d/zz-ipsec-client.conf" \
        "$PREFIX/engine/etc/strongswan.d/zz-ipsec-client.conf"
    install -m 0755 "$repo/engine/entrypoint.sh" "$PREFIX/libexec/entrypoint.sh"
    install -m 0755 "$repo/engine/updown.sh" "$PREFIX/libexec/updown"
    install -m 0755 "$helper" "$PREFIX/bin/ipsec-client-helper"
    chown -R root:root "$PREFIX"

    install -m 0644 "$repo/packaging/systemd/ipsec-client-helper.socket" "$UNIT_DIR/"
    install -m 0644 "$repo/packaging/systemd/ipsec-client-helper.service" "$UNIT_DIR/"
    install -m 0644 "$repo/packaging/polkit/$POLICY" "$POLKIT_DIR/$POLICY"

    # SELinux: rótulos padrão nos arquivos copiados (senão podem herdar o rótulo
    # do diretório temporário de onde vieram).
    if command -v restorecon >/dev/null 2>&1; then
        restorecon -RF "$PREFIX" "$UNIT_DIR/ipsec-client-helper.socket" \
            "$UNIT_DIR/ipsec-client-helper.service" "$POLKIT_DIR/$POLICY" 2>/dev/null || true
    fi

    systemctl daemon-reload
    systemctl stop ipsec-client-helper.service 2>/dev/null || true
    systemctl enable --now ipsec-client-helper.socket
    log "instalado: $("$PREFIX/bin/ipsec-client-helper" --version), strongSwan $(grep -m1 '^strongswan=' "$PREFIX/engine/BUILDINFO" | cut -d= -f2)"
}

case ${1:-} in
install)
    shift
    install "$@"
    ;;
uninstall) uninstall ;;
*) die "uso: $0 install --engine <tarball> --helper <binário> | uninstall" ;;
esac
