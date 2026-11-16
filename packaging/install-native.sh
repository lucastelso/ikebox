#!/usr/bin/env bash
# Instala/remove o componente de rede do IPsec Client (engine embutido + helper
# com privilégio, ADRs 0013, 0014 e 0018). Roda como root.
#
# Uso:
#   install-native.sh install --bundle <dir> [--origin app|package|manual]
#   install-native.sh uninstall
#
# <dir> é o pacote gerado por scripts/stage-network-component.sh (o app traz
# um dentro dele). Origem: quem instalou (o app só remove o que não veio de
# um pacote .rpm/.deb).
#
# Instala:
#   /opt/ipsec-client/engine/        strongSwan compilado (do tarball)
#   /opt/ipsec-client/libexec/       entrypoint.sh, updown e este instalador
#   /opt/ipsec-client/bin/           ipsec-client-helper
#   /opt/ipsec-client/{VERSION,SHA256SUMS,ORIGIN}
#   /etc/systemd/system/             ipsec-client-helper.{socket,service}
#   /usr/share/polkit-1/actions/     io.github.lucastelso.ipsecclient.policy

set -euo pipefail

readonly PREFIX=/opt/ipsec-client
readonly UNIT_DIR=/etc/systemd/system
readonly POLKIT_DIR=/usr/share/polkit-1/actions
readonly POLICY=io.github.lucastelso.ipsecclient.policy
# Arquivos obrigatórios do pacote (o app confere a mesma lista).
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

log() { printf '[componente de rede] %s\n' "$*"; }
die() {
    printf '[componente de rede] erro: %s\n' "$*" >&2
    exit 1
}

# Uma saída só (o app mostra o log na ordem em que aconteceu).
exec 2>&1

[[ $(id -u) -eq 0 ]] || die "rode como root (sudo/pkexec)"

do_uninstall() {
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

# Copia o pacote para um diretório só do root e confere os hashes ali: o que
# é instalado é exatamente o que foi conferido, mesmo que a origem mude
# depois.
verify_bundle() {
    local bundle=$1 work=$2 path
    cp -R --no-preserve=all "$bundle"/. "$work"/
    if [[ -n $(find "$work" ! -type f ! -type d -print -quit) ]]; then
        die "pacote com links ou arquivos especiais"
    fi
    [[ -f $work/SHA256SUMS ]] || die "pacote sem SHA256SUMS"
    (cd "$work" && sha256sum --check --strict --quiet SHA256SUMS) ||
        die "arquivos do pacote corrompidos (SHA256SUMS não confere)"
    local -A listed=()
    while read -r _ path; do
        listed[${path#\*}]=1
    done <"$work/SHA256SUMS"
    for path in "${REQUIRED[@]}"; do
        [[ -n ${listed[$path]:-} ]] || die "pacote incompleto: falta $path"
    done
    # O tarball só pode conter opt/ipsec-client/engine/... (nada fora disso).
    if tar -tzf "$work/engine.tar.gz" | grep -vqE '^opt/(ipsec-client/(engine(/.*)?)?)?/?$'; then
        die "tarball do engine com caminhos inesperados"
    fi
}

do_install() {
    local bundle="" origin=manual work
    while (($# > 0)); do
        case $1 in
        --bundle) bundle=${2:?}; shift 2 ;;
        --origin) origin=${2:?}; shift 2 ;;
        *) die "argumento desconhecido: $1" ;;
        esac
    done
    [[ -d $bundle ]] || die "pacote do componente não encontrado (--bundle)"
    case $origin in
    app | package | manual) ;;
    *) die "origem inválida: $origin" ;;
    esac

    work=$(mktemp -d)
    # shellcheck disable=SC2064 # o caminho é fixado agora, de propósito
    trap "rm -rf '$work'" EXIT
    verify_bundle "$bundle" "$work"

    log "instalando em $PREFIX"
    systemctl stop ipsec-client-engine.service 2>/dev/null || true
    rm -rf "$PREFIX/engine"
    install -d -m 0755 "$PREFIX" "$PREFIX/bin" "$PREFIX/libexec"
    tar -xzf "$work/engine.tar.gz" -C / --no-same-owner --no-same-permissions
    install -m 0644 "$work/engine/strongswan.d/zz-ipsec-client.conf" \
        "$PREFIX/engine/etc/strongswan.d/zz-ipsec-client.conf"
    install -m 0755 "$work/engine/entrypoint.sh" "$PREFIX/libexec/entrypoint.sh"
    install -m 0755 "$work/engine/updown.sh" "$PREFIX/libexec/updown"
    install -m 0755 "$work/install-native.sh" "$PREFIX/libexec/install-native.sh"
    # O helper vem compactado (ver scripts/stage-network-component.sh).
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

    # SELinux: rótulos padrão nos arquivos copiados (senão podem herdar o rótulo
    # do diretório temporário de onde vieram).
    if command -v restorecon >/dev/null 2>&1; then
        restorecon -RF "$PREFIX" "$UNIT_DIR/ipsec-client-helper.socket" \
            "$UNIT_DIR/ipsec-client-helper.service" "$POLKIT_DIR/$POLICY" 2>/dev/null || true
    fi

    systemctl daemon-reload
    systemctl stop ipsec-client-helper.service 2>/dev/null || true
    systemctl enable --now ipsec-client-helper.socket
    log "instalado: $("$PREFIX/bin/ipsec-client-helper" --version), strongSwan $(grep -m1 '^strongswan=' "$PREFIX/engine/BUILDINFO" | cut -d= -f2) (origem: $origin)"
}

case ${1:-} in
install)
    shift
    do_install "$@"
    ;;
uninstall) do_uninstall ;;
*) die "uso: $0 install --bundle <dir> [--origin app|package|manual] | uninstall" ;;
esac
