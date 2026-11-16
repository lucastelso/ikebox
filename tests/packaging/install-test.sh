#!/usr/bin/env bash
# Teste do instalador do componente de rede (packaging/install-native.sh).
#
# Monta um pacote de verdade com scripts/stage-network-component.sh, mas com
# engine e helper FALSOS (rápido, sem compilar nada), e roda o instalador como
# root num container Ubuntu descartável com um systemctl de mentira. Nada é
# instalado no computador de quem roda o teste.
#
# Cenários: instalar, reinstalar com outra origem, remover, e recusar pacote
# adulterado, com link simbólico, incompleto, com tarball fora de
# /opt/ipsec-client/engine e execução sem root.

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
# Mesma base fixada do builder do engine.
readonly IMAGE=ubuntu:22.04@sha256:9abcaffc92a5279fff3a6a41635c0800ea33e528425fb389658806476e7defde

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }
fail() {
    printf '  \033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

# --- Dentro do container (root) ----------------------------------------------

# Roda o instalador e exige falha com a mensagem indicada.
expect_refusal() {
    local why=$1 pattern=$2 bundle=$3 out
    if out=$(bash "$bundle/install-native.sh" install --bundle "$bundle" --origin app 2>&1); then
        fail "$why: o instalador aceitou"
    fi
    grep -q -- "$pattern" <<<"$out" || fail "$why: mensagem inesperada: $out"
    [[ ! -e /opt/ipsec-client ]] || fail "$why: instalou mesmo assim"
    ok "$why: recusado"
}

# Cópia gravável do pacote, para adulterar.
copy_bundle() {
    local dest
    dest=$(mktemp -d)
    cp -R /bundle/. "$dest"/
    printf '%s' "$dest"
}

resum() {
    # shellcheck disable=SC2094 # o find exclui o próprio SHA256SUMS
    (cd "$1" && find . -type f ! -name SHA256SUMS -printf '%P\n' | LC_ALL=C sort |
        xargs -d '\n' sha256sum >SHA256SUMS)
}

inside() {
    # systemctl de mentira: registra as chamadas.
    cat >/usr/local/bin/systemctl <<'STUB'
#!/bin/sh
echo "$*" >>/tmp/systemctl.log
STUB
    chmod 0755 /usr/local/bin/systemctl
    mkdir -p /etc/systemd/system /usr/share/polkit-1/actions

    step "instalar (origem app)"
    bash /bundle/install-native.sh install --bundle /bundle --origin app
    local p=/opt/ipsec-client
    [[ -x $p/bin/ipsec-client-helper && -x $p/libexec/entrypoint.sh && -x $p/libexec/updown ]] ||
        fail "binários/scripts ausentes"
    [[ -x $p/libexec/install-native.sh ]] || fail "instalador não copiado"
    [[ -f $p/engine/etc/strongswan.d/zz-ipsec-client.conf ]] || fail "configuração ausente"
    [[ $(cat "$p/ORIGIN") == app ]] || fail "ORIGIN errado"
    cmp -s /bundle/SHA256SUMS "$p/SHA256SUMS" || fail "SHA256SUMS não registrado"
    cmp -s /bundle/VERSION "$p/VERSION" || fail "VERSION não registrado"
    [[ $(stat -c %U "$p/bin/ipsec-client-helper") == root ]] || fail "dono errado"
    [[ -z $(find "$p" -perm /022 -print -quit) ]] || fail "arquivo gravável por outros"
    [[ -f /etc/systemd/system/ipsec-client-helper.socket ]] || fail "unit ausente"
    [[ -f /usr/share/polkit-1/actions/io.github.lucastelso.ipsecclient.policy ]] ||
        fail "policy ausente"
    grep -q "enable --now ipsec-client-helper.socket" /tmp/systemctl.log ||
        fail "socket não habilitado"
    ok "arquivos, donos, permissões, units, policy e registro da versão"

    step "reinstalar (origem manual)"
    bash /bundle/install-native.sh install --bundle /bundle --origin manual >/dev/null
    [[ $(cat "$p/ORIGIN") == manual ]] || fail "ORIGIN não atualizado"
    ok "reinstalação substitui a anterior"

    step "remover (pelo instalador copiado)"
    bash "$p/libexec/install-native.sh" uninstall >/dev/null
    [[ ! -e $p && ! -e /etc/systemd/system/ipsec-client-helper.socket ]] ||
        fail "sobrou algo"
    [[ ! -e /usr/share/polkit-1/actions/io.github.lucastelso.ipsecclient.policy ]] ||
        fail "policy não removida"
    ok "removido por completo"

    step "pacotes recusados"
    local b
    b=$(copy_bundle)
    echo "# adulterado" >>"$b/engine/updown.sh"
    expect_refusal "arquivo adulterado" "não confere" "$b"

    b=$(copy_bundle)
    ln -sf /etc/shadow "$b/packaging/polkit/io.github.lucastelso.ipsecclient.policy"
    expect_refusal "link simbólico" "links" "$b"

    b=$(copy_bundle)
    rm "$b/packaging/systemd/ipsec-client-helper.service"
    resum "$b"
    expect_refusal "pacote incompleto" "falta packaging/systemd/ipsec-client-helper.service" "$b"

    b=$(copy_bundle)
    local evil
    evil=$(mktemp -d)
    mkdir -p "$evil/opt/ipsec-client/engine" "$evil/etc/cron.d"
    echo x >"$evil/etc/cron.d/evil"
    tar -czf "$b/engine.tar.gz" -C "$evil" opt etc
    resum "$b"
    expect_refusal "tarball fora do engine" "caminhos inesperados" "$b"

    if bash /bundle/install-native.sh install --bundle /bundle --origin root >/dev/null 2>&1; then
        fail "origem inválida aceita"
    fi
    [[ ! -e /opt/ipsec-client ]] || fail "origem inválida: instalou mesmo assim"
    ok "origem inválida: recusada"

    if setpriv --reuid=65534 --regid=65534 --clear-groups \
        bash /bundle/install-native.sh install --bundle /bundle >/dev/null 2>&1; then
        fail "rodou sem root"
    fi
    ok "sem root: recusado"
}

# --- No host -----------------------------------------------------------------

main() {
    command -v docker >/dev/null || fail "Docker é necessário para este teste"
    local work
    work=$(mktemp -d)
    # shellcheck disable=SC2064 # caminho fixado agora
    trap "rm -rf '$work'" EXIT

    step "montando um pacote com engine e helper falsos"
    mkdir -p "$work/root/opt/ipsec-client/engine/etc/strongswan.d" \
        "$work/root/opt/ipsec-client/engine/libexec/ipsec"
    # O BUILDINFO declara os patches deste commit, como um engine compilado de
    # verdade: sem isso, o script de produção recusa o engine (ADR-0019).
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
    ok "pacote montado pelo script de produção"

    docker run --rm --network none \
        -v "$work/bundle:/bundle:ro" \
        -v "$repo/tests/packaging/install-test.sh:/test.sh:ro" \
        "$IMAGE" bash /test.sh --inside
    printf '\n\033[1;32m✔ instalador do componente de rede OK\033[0m\n'
}

if [[ ${1:-} == --inside ]]; then
    inside
else
    main
fi
