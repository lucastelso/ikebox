#!/usr/bin/env bash
# Teste ponta a ponta do ipsec-client-cli contra o gateway IPsec de teste.
#
# Roda o fluxo de um usuário real: doctor → profiles import → connect
# (senha errada, depois certa) → status → logs → disconnect. Usa um
# XDG_CONFIG_HOME temporário (não toca nos perfis do usuário) e credenciais
# FALSAS via --credentials-stdin (não toca no chaveiro).
#
# Usa os nomes de produção (container ipsec-client-engine, interface ipsc0):
# aborta se já houver uma conexão ativa ou se UDP 500/4500 estiver ocupado.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
readonly NET=ipsec-client-cli-e2e
readonly GW=ipsec-client-cli-e2e-gw
readonly IMAGE_LOCAL=ipsec-client-engine:e2e
readonly TEST_SECRETS_RE='ipsec-client-test-psk|senha-de-teste'

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }
fail() {
    printf '  \033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

workdir=""
cli=""

cleanup() {
    local rc=$?
    set +e
    [[ -n $cli ]] && "$cli" disconnect >/dev/null 2>&1
    docker rm -f "$GW" >/dev/null 2>&1
    docker network rm "$NET" >/dev/null 2>&1
    [[ -n $workdir ]] && rm -rf "$workdir"
    exit "$rc"
}

main() {
    step "Pré-requisitos"
    if [[ -f "$HOME/.cargo/env" ]] && ! command -v cargo >/dev/null 2>&1; then
        # shellcheck source=/dev/null
        . "$HOME/.cargo/env"
    fi
    command -v cargo >/dev/null || fail "cargo não encontrado"
    docker info >/dev/null 2>&1 || fail "Docker inacessível"
    if [[ -n $(ss -Hlun '( sport = :500 or sport = :4500 )') ]]; then
        fail "UDP 500/4500 em uso (outra VPN IPsec conectada?)"
    fi
    if docker ps -q --filter name=^ipsec-client-engine$ | grep -q .; then
        fail "já existe uma conexão do ipsec-client ativa"
    fi
    trap cleanup EXIT
    workdir=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/ipsec-client-cli-e2e.XXXXXX")
    export XDG_CONFIG_HOME="$workdir/config"
    ok "XDG_CONFIG_HOME temporário: $XDG_CONFIG_HOME"

    step "Build do CLI"
    cargo build -q -p ipsec-client-cli --manifest-path "$repo_root/Cargo.toml"
    cli="$repo_root/target/debug/ipsec-client-cli"
    ok "$("$cli" --version)"

    step "Gateway IPsec de teste"
    docker build -q -t "$IMAGE_LOCAL" "$repo_root/engine" >/dev/null
    docker network rm "$NET" >/dev/null 2>&1 || true
    docker network create "$NET" >/dev/null
    docker run -d --name "$GW" --network "$NET" --cap-add NET_ADMIN \
        --security-opt label=disable \
        -v "$repo_root/tests/engine/gateway:/test:ro" --entrypoint /test/gateway.sh \
        "$IMAGE_LOCAL" >/dev/null
    local _ gw_ip
    for _ in $(seq 1 150); do
        docker logs "$GW" 2>&1 | grep -q '\[gateway-teste\] pronto' && break
        sleep 0.1
    done
    gw_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$GW")
    [[ $gw_ip =~ ^[0-9]+(\.[0-9]+){3}$ ]] || fail "gateway de teste não subiu"
    ok "gateway em $gw_ip"

    step "doctor"
    "$cli" doctor >"$workdir/doctor.txt" 2>&1 || {
        cat "$workdir/doctor.txt"
        fail "doctor reprovou o ambiente"
    }
    ok "$(tail -n 1 "$workdir/doctor.txt")"

    step "profiles import / list / validate"
    sed "s/\"gw.ipsec-client.test\"/\"$gw_ip\"/" \
        "$repo_root/crates/core/tests/fixtures/perfil-gateway-teste.json" >"$workdir/perfil.json"
    "$cli" profiles validate "$workdir/perfil.json" >/dev/null || fail "perfil inválido"
    "$cli" profiles import "$workdir/perfil.json" >/dev/null
    "$cli" profiles list | grep -q '^gateway-teste ' || fail "perfil não listado"
    [[ $(stat -c '%a' "$XDG_CONFIG_HOME/ipsec-client/profiles/gateway-teste.json") == 600 ]] ||
        fail "perfil sem permissão 0600"
    ok "perfil importado (0600) e listado"

    step "connect com senha errada → mensagem clara, nada de pé"
    local out rc=0
    out=$(printf 'psk=ipsec-client-test-psk\nxauth-user=usuario.teste\nxauth-password=errada\n' |
        "$cli" connect gateway-teste --detach --credentials-stdin 2>&1) || rc=$?
    ((rc != 0)) || fail "connect com senha errada deveria falhar"
    [[ $out == *"usuário ou senha XAuth incorretos"* ]] || {
        printf '%s\n' "$out"
        fail "mensagem de erro inesperada"
    }
    [[ $out != *usuario.teste* ]] || fail "usuário XAuth sem máscara na saída"
    ip link show ipsc0 >/dev/null 2>&1 && fail "ipsc0 sobrou após a falha"
    ok "\"usuário ou senha XAuth incorretos\", sem identidade exposta, host limpo"

    step "connect --detach"
    out=$(printf 'psk=ipsec-client-test-psk\nxauth-user=usuario.teste\nxauth-password=senha-de-teste\n' |
        "$cli" connect gateway-teste --detach --credentials-stdin 2>&1) || {
        printf '%s\n' "$out"
        fail "connect falhou"
    }
    [[ $out == *"Conectado: gateway-teste"* ]] || fail "saída do connect inesperada"
    if grep -qE "$TEST_SECRETS_RE|usuario\.teste" <<<"$out"; then
        fail "segredo ou identidade na saída do connect"
    fi
    ok "conectado (saída sem segredos nem identidades)"

    step "status / ping / logs"
    out=$("$cli" status) || fail "status deveria sair com 0 quando conectado"
    [[ $out == *"IP virtual: 198.51.100."* ]] || fail "status sem VIP: $out"
    ping -c 2 -W 2 -q 192.0.2.1 >/dev/null || fail "ping pelo túnel falhou"
    out=$("$cli" logs -n 200)
    [[ $out == *"pronto"* ]] || fail "logs sem a linha de prontidão"
    if grep -qE "$TEST_SECRETS_RE" <<<"$out"; then fail "segredo nos logs"; fi
    ok "status com VIP, ping pelo túnel, logs sem segredos"

    step "disconnect"
    "$cli" disconnect | grep -q "Desconectado" || fail "disconnect"
    rc=0
    "$cli" status >/dev/null || rc=$?
    ((rc == 3)) || fail "status deveria sair com 3 quando desconectado (saiu $rc)"
    ip link show ipsc0 >/dev/null 2>&1 && fail "ipsc0 sobrou"
    ok "desconectado, status = 3, host limpo"

    step "Resultado"
    ok "fluxo do CLI completo"
}

main "$@"
