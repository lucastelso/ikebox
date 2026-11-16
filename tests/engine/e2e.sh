#!/usr/bin/env bash
# Teste ponta a ponta do engine, SEM nenhuma VPN real.
#
# Sobe um gateway IPsec de teste (IKEv1 agressivo, PSK de grupo + XAuth,
# modp1024/768) numa rede Docker isolada e conecta o engine a ele do mesmo jeito
# que o app vai fazer (--network host, só NET_ADMIN). Verifica:
#
#   cenário 1, ciclo normal:
#     interface XFRM + MTU, socket VICI (dono/permissão, `version` como usuário
#     comum), docker inspect sem segredos, VIP, rota, DNS split, ping pelo
#     túnel; depois do `docker stop`, que não sobrou nada (interface, rotas,
#     políticas/SAs XFRM com o if_id, socket).
#   cenário 2, recuperação após SIGKILL:
#     o engine morre sem limpar e o próximo engine remove os restos.
#   cenário 3, túnel completo (0.0.0.0/0) em rede isolada.
#   cenário 4, PFS só com modp768 (DH1) na fase 2.
#   cenário 5 (só no engine embutido, IPSEC_CLIENT_E2E_FORCED=1): um cliente
#     VICI malicioso tenta escolher o updown (que roda como root) e o if_id;
#     o charon com o patch do IPsec Client ignora os dois (ADR-0019).
#
# Altera o host só temporariamente: cria a interface ipsct0 (nome e if_id
# próprios do teste, para não colidir com uma sessão real), rotas para
# 192.0.2.0/24 (RFC 5737) e DNS de ~ipsec-client.test nessa interface.
#
# Requisitos: Docker (usuário no grupo docker), python3, iproute2, ping,
# systemd-resolved e as portas UDP 500/4500 livres.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
here="$repo_root/tests/engine"

# IMAGE: gateway de teste (strongSwan 5.8.2 validado) e utilitários.
# ENGINE_IMAGE: o engine testado (padrão: a mesma imagem; o spike do engine
# embutido passa outra, com o strongSwan compilado por nós).
readonly IMAGE=${IPSEC_CLIENT_E2E_IMAGE:-ipsec-client-engine:e2e}
readonly ENGINE_IMAGE=${IPSEC_CLIENT_E2E_ENGINE_IMAGE:-$IMAGE}
readonly EXPECT_VERSION=${IPSEC_CLIENT_E2E_EXPECT_VERSION:-5.8.2}
# 1 = engine embutido: updown/if_id forçados pelo root e socket do uid (0600).
readonly FORCED=${IPSEC_CLIENT_E2E_FORCED:-0}
readonly UPDOWN=/usr/local/libexec/ipsec-client/updown
readonly NET=ipsec-client-e2e
readonly GW=ipsec-client-e2e-gateway
readonly ENGINE=ipsec-client-e2e-engine
readonly IFACE=ipsct0
readonly IF_ID=7341
readonly RT_PROTO=241
readonly VICI_URI=unix:///run/ipsec-client/charon.vici
readonly TEST_SECRETS_RE='ipsec-client-test-psk|senha-de-teste'
HEX_ID=$(printf '0x%x' "$IF_ID")
readonly HEX_ID

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }
fail() {
    printf '  \033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}
need() { command -v "$1" >/dev/null 2>&1 || fail "comando necessário não encontrado: $1"; }

workdir=""
rundir=""
confdir=""
gw_ip=""

# Roda `ip` com privilégio, num container descartável na rede do host.
# A saída de `ip xfrm state` contém chaves: só é usada para contar, nunca impressa.
host_ip() {
    docker run --rm --network host --cap-add NET_ADMIN --security-opt label=disable \
        --entrypoint ip "$IMAGE" "$@"
}
count_xfrm() { # count_xfrm state|policy
    host_ip -o xfrm "$1" | grep -c "if_id $HEX_ID" || true
}

cleanup() {
    local rc=$?
    set +e
    if ((rc != 0)); then
        printf '\n--- últimas linhas do engine ---\n'
        docker logs "$ENGINE" 2>&1 | tail -n 60
        printf '\n--- últimas linhas do gateway ---\n'
        docker logs "$GW" 2>&1 | tail -n 30
    fi
    # stop (gracioso) antes do rm, para o entrypoint limpar o host.
    docker stop -t 10 "$ENGINE" >/dev/null 2>&1
    docker rm -f "$ENGINE" "$GW" >/dev/null 2>&1
    docker network rm "$NET" >/dev/null 2>&1
    if ip link show "$IFACE" >/dev/null 2>&1; then
        host_ip link del "$IFACE" >/dev/null 2>&1
    fi
    [[ -n $workdir ]] && rm -rf "$workdir"
    exit "$rc"
}

# start_engine [host|isolado]
#   host:    como o app sobe (--network host + D-Bus do host).
#   isolado: no namespace de rede da rede de teste e SEM o D-Bus do host. O
#            resolvectl resolve o índice da interface no namespace dele e
#            poderia configurar a interface errada no systemd-resolved do host.
start_engine() {
    local mode=${1:-host} net_args
    if [[ $mode == host ]]; then
        net_args=(--network host -v /run/dbus/system_bus_socket:/run/dbus/system_bus_socket)
    else
        net_args=(--network "$NET")
    fi
    # Um socket velho (de um engine morto com SIGKILL) não indica prontidão:
    # remove antes e espera o entrypoint anunciar "pronto".
    rm -f "$rundir/charon.vici"
    local env_args
    if ((FORCED)); then
        # Como o helper sobe o engine embutido: o DNS vem do root, não do VICI.
        env_args=(
            -e IPSEC_CLIENT_SOCKET_UID="$(id -u)"
            -e IPSEC_CLIENT_UPDOWN="$UPDOWN"
            -e IPSEC_CLIENT_DNS_SERVERS=192.0.2.54
            -e IPSEC_CLIENT_DNS_DOMAINS="~ipsec-client.test"
            -e IPSEC_CLIENT_DNS_FLAGS=use-received
        )
    else
        env_args=(-e IPSEC_CLIENT_SOCKET_GID="$(id -g)")
    fi
    docker run -d --name "$ENGINE" \
        "${net_args[@]}" \
        --cap-add NET_ADMIN \
        --security-opt label=disable \
        -v "$rundir:/run/ipsec-client" \
        -v "$confdir:/test:ro" \
        -e IPSEC_CLIENT_IFACE="$IFACE" \
        -e IPSEC_CLIENT_IF_ID="$IF_ID" \
        "${env_args[@]}" \
        "$ENGINE_IMAGE" >/dev/null

    local sock="$rundir/charon.vici" _
    for _ in $(seq 1 150); do
        if docker logs "$ENGINE" 2>&1 | grep -q '^\[engine\] pronto'; then
            [[ -S $sock ]] || fail "engine pronto, mas sem socket VICI"
            return 0
        fi
        [[ $(docker inspect -f '{{.State.Running}}' "$ENGINE") == true ]] || fail "o engine parou durante a inicialização"
        sleep 0.1
    done
    fail "o engine não ficou pronto em 15 s"
}

# render_conf ARQUIVO TS_REMOTO ARGS_EXTRAS_DO_UPDOWN
render_conf() {
    sed -e "s|@GATEWAY@|$gw_ip|" -e "s|@IFACE@|$IFACE|" -e "s|@IF_ID@|$IF_ID|" \
        -e "s|@REMOTE_TS@|$2|" -e "s|@UPDOWN_EXTRA@|$3|" \
        "$here/initiator.conf.in" >"$confdir/$1"
}

# connect_tunnel [ARQUIVO]
connect_tunnel() {
    local conf=${1:-initiator.conf}
    docker exec "$ENGINE" swanctl --load-all --uri "$VICI_URI" --file "/test/$conf" \
        >"$workdir/load.log" 2>&1 || {
        cat "$workdir/load.log"
        fail "swanctl --load-all falhou"
    }
    docker exec "$ENGINE" swanctl --initiate --uri "$VICI_URI" --child rede-teste --timeout 30 \
        >"$workdir/initiate.log" 2>&1 || {
        cat "$workdir/initiate.log"
        fail "a negociação IKE falhou"
    }
}

main() {
    step "Pré-requisitos"
    for cmd in docker python3 ip ss ping resolvectl stat; do need "$cmd"; done
    docker info >/dev/null 2>&1 || fail "o Docker não está acessível (usuário no grupo docker?)"
    [[ -S /run/dbus/system_bus_socket ]] || fail "socket D-Bus do sistema ausente"
    if [[ -n $(ss -Hlun '( sport = :500 or sport = :4500 )') ]]; then
        fail "UDP 500/4500 em uso no host (outra VPN IPsec conectada?)"
    fi
    ip link show "$IFACE" >/dev/null 2>&1 && fail "a interface $IFACE já existe"
    docker rm -f "$ENGINE" "$GW" >/dev/null 2>&1 || true
    docker network rm "$NET" >/dev/null 2>&1 || true
    trap cleanup EXIT

    workdir=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/ipsec-client-e2e.XXXXXX")
    rundir="$workdir/run"
    confdir="$workdir/conf"
    mkdir -m 0700 "$rundir" "$confdir"
    ok "ambiente pronto"

    step "Build da imagem do engine ($IMAGE)"
    docker build -q -t "$IMAGE" "$repo_root/engine" >/dev/null
    ok "imagem construída"

    step "Gateway IPsec de teste"
    docker network create "$NET" >/dev/null
    docker run -d --name "$GW" --network "$NET" --cap-add NET_ADMIN \
        --security-opt label=disable \
        -v "$here/gateway:/test:ro" --entrypoint /test/gateway.sh "$IMAGE" >/dev/null
    local _
    for _ in $(seq 1 150); do
        docker logs "$GW" 2>&1 | grep -q '\[gateway-teste\] pronto' && break
        sleep 0.1
    done
    docker logs "$GW" 2>&1 | grep -q '\[gateway-teste\] pronto' || fail "o gateway de teste não subiu"
    gw_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$GW")
    [[ $gw_ip =~ ^[0-9]+(\.[0-9]+){3}$ ]] || fail "IP do gateway inválido: $gw_ip"
    render_conf initiator.conf 192.0.2.0/24 \
        " --dns '192.0.2.54' --domain '~ipsec-client.test' --use-received-dns"
    render_conf initiator-full.conf 0.0.0.0/0 ""
    render_conf initiator-modp768.conf 203.0.113.0/24 ""
    # Cliente malicioso: tenta rodar um comando como root pelo updown e tirar
    # as SAs da interface da sessão com outro if_id.
    render_conf initiator-evil.conf 192.0.2.0/24 ""
    sed -i -e "s|updown = .*|updown = /bin/touch /run/ipsec-client/PWNED|" \
        -e "s|if_id_in = .*|if_id_in = 999|" -e "s|if_id_out = .*|if_id_out = 999|" \
        "$confdir/initiator-evil.conf"
    # No IKEv1, a fase 2 só leva um grupo DH (o da primeira proposta): para
    # oferecer modp768, ele precisa vir primeiro.
    sed -i 's/esp_proposals = .*/esp_proposals = aes256-sha1-modp768/' "$confdir/initiator-modp768.conf"
    ok "gateway em $gw_ip (rede protegida 192.0.2.0/24)"

    # ------------------------------------------------------------------ 1 --
    step "Cenário 1: engine sobe como o app sobe"
    start_engine
    local link
    link=$(ip -d link show "$IFACE")
    [[ $link == *"mtu 1400"* ]] || fail "MTU da $IFACE não é 1400"
    [[ $link == *xfrm* && $link == *"if_id $HEX_ID"* ]] || fail "$IFACE não é XFRM com if_id $HEX_ID"
    ok "$IFACE: interface XFRM, if_id $HEX_ID, MTU 1400"

    if ((FORCED)); then
        [[ $(stat -c '%a %u' "$rundir/charon.vici") == "600 $(id -u)" ]] ||
            fail "socket VICI sem 0600/uid do usuário"
        ok "socket VICI 0600, dono $(id -u)"
    else
        [[ $(stat -c '%a %g' "$rundir/charon.vici") == "660 $(id -g)" ]] ||
            fail "socket VICI sem 0660/gid do usuário"
        ok "socket VICI 0660, grupo $(id -g)"
    fi

    local version
    version=$(python3 -I "$here/vici_version.py" "$rundir/charon.vici") ||
        fail "VICI 'version' falhou como usuário comum"
    [[ $version == *"version=$EXPECT_VERSION"* ]] || fail "versão inesperada do charon: $version"
    ok "VICI 'version' como usuário comum: $(grep '^version=' <<<"$version")"

    docker inspect "$ENGINE" | grep -qE "$TEST_SECRETS_RE" && fail "segredo visível no docker inspect"
    ok "docker inspect sem segredos"

    step "Cenário 1: negociação (IKEv1 agressivo + PSK + XAuth)"
    connect_tunnel
    ok "túnel estabelecido"

    if docker logs "$ENGINE" 2>&1 | grep -qE "$TEST_SECRETS_RE" ||
        grep -qE "$TEST_SECRETS_RE" "$workdir/load.log" "$workdir/initiate.log"; then
        fail "PSK ou senha XAuth apareceu nos logs do charon"
    fi
    ok "logs do charon (nível 1) sem PSK nem senha XAuth"

    local vip
    vip=$(ip -4 -o addr show dev "$IFACE" | awk '{ print $4 }' | cut -d/ -f1 | head -n 1)
    [[ $vip == 198.51.100.* ]] || fail "VIP não instalado na $IFACE (achei: '${vip:-nada}')"
    ok "VIP $vip na $IFACE"

    local route
    route=$(ip -4 route show dev "$IFACE" proto "$RT_PROTO")
    [[ $route == *"192.0.2.0/24"* && $route == *"src $vip"* ]] || fail "rota 192.0.2.0/24 ausente: '$route'"
    ok "rota 192.0.2.0/24 dev $IFACE src $vip (proto $RT_PROTO)"

    local dns domains default_route
    dns=$(resolvectl dns "$IFACE")
    domains=$(resolvectl domain "$IFACE")
    default_route=$(resolvectl default-route "$IFACE")
    [[ $dns == *192.0.2.54* ]] || fail "DNS fixo ausente: $dns"
    [[ $dns == *192.0.2.53* ]] || fail "DNS recebido do gateway ausente: $dns"
    [[ $domains == *"~ipsec-client.test"* ]] || fail "domínio ausente: $domains"
    [[ $default_route == *no ]] || fail "default-route deveria ser 'no': $default_route"
    ok "DNS split: 192.0.2.54 (fixo) + 192.0.2.53 (recebido), ~ipsec-client.test, sem default-route"

    ping -c 3 -W 2 -q 192.0.2.1 >/dev/null || fail "ping 192.0.2.1 pelo túnel falhou"
    ok "ping 192.0.2.1 pelo túnel"

    local policies states
    policies=$(count_xfrm policy)
    states=$(count_xfrm state)
    ((policies >= 2 && states >= 2)) || fail "XFRM sem políticas/SAs com if_id ($policies/$states)"
    ok "XFRM: $policies políticas e $states SAs com if_id $HEX_ID"

    ip -4 route show default | grep -q "dev $IFACE" &&
        fail "rota padrão foi alterada (split tunnel esperado)"
    ok "rota padrão do host intacta"

    step "Cenário 1: desconexão (docker stop)"
    docker stop -t 10 "$ENGINE" >/dev/null
    [[ $(docker inspect -f '{{.State.ExitCode}}' "$ENGINE") == 0 ]] || fail "o entrypoint não saiu com 0"
    ip link show "$IFACE" >/dev/null 2>&1 && fail "a $IFACE sobrou"
    [[ -z $(ip -4 route show proto "$RT_PROTO")$(ip -6 route show proto "$RT_PROTO") ]] || fail "sobraram rotas proto $RT_PROTO"
    [[ ! -e "$rundir/charon.vici" ]] || fail "o socket VICI sobrou"
    policies=$(count_xfrm policy)
    states=$(count_xfrm state)
    ((policies == 0 && states == 0)) || fail "sobrou XFRM com if_id ($policies políticas, $states SAs)"
    ok "nada sobrou: interface, rotas, DNS, XFRM e socket"
    docker rm "$ENGINE" >/dev/null

    # ------------------------------------------------------------------ 2 --
    step "Cenário 2: engine morto com SIGKILL (sem limpeza)"
    start_engine
    connect_tunnel
    docker kill -s KILL "$ENGINE" >/dev/null
    docker rm "$ENGINE" >/dev/null
    ip link show "$IFACE" >/dev/null 2>&1 || fail "esperava restos da sessão morta"
    ok "restos presentes: $IFACE, $(count_xfrm policy) políticas, $(count_xfrm state) SAs"

    step "Cenário 2: o próximo engine limpa os restos"
    start_engine
    route=$(ip -4 route show dev "$IFACE")
    [[ -z $route ]] || fail "rotas antigas na $IFACE: $route"
    [[ $(resolvectl dns "$IFACE") != *192.0.2.* ]] || fail "DNS antigo na $IFACE"
    states=$(count_xfrm state)
    ((states == 0)) || fail "sobraram $states SAs com if_id"
    ok "restos removidos: interface recriada, rotas, DNS e SAs"
    # Limitação conhecida: o iproute2 5.5 não remove políticas com if_id.
    policies=$(count_xfrm policy)
    ok "políticas órfãs restantes: $policies (só sobram com o iproute2 5.5 da imagem Docker; são inertes)"

    step "Cenário 2: reconexão por cima dos restos"
    # Simula o gateway real descartando a sessão morta (timeout de DPD): o
    # mesmo usuário recebe de volta o mesmo VIP, e o charon substitui as
    # políticas órfãs (que têm o mesmo seletor) e as remove ao sair. Sem isso,
    # elas continuariam inertes no kernel de quem roda o teste.
    docker exec "$GW" swanctl --terminate --ike gateway-teste --force --timeout 2 >/dev/null 2>&1 || true
    connect_tunnel
    [[ $(ip -4 -o addr show dev "$IFACE") == *" $vip/"* ]] || fail "esperava o mesmo VIP $vip na reconexão"
    ping -c 3 -W 2 -q 192.0.2.1 >/dev/null || fail "ping pelo túnel novo falhou"
    ok "túnel novo funciona (ping 192.0.2.1)"
    docker stop -t 10 "$ENGINE" >/dev/null
    ip link show "$IFACE" >/dev/null 2>&1 && fail "a $IFACE sobrou"
    policies=$(count_xfrm policy)
    states=$(count_xfrm state)
    ((policies == 0 && states == 0)) || fail "sobrou XFRM com if_id ($policies políticas, $states SAs)"
    ok "o charon substituiu as políticas órfãs e removeu tudo ao sair"

    # ------------------------------------------------------------------ 3 --
    step "Cenário 3: túnel completo (0.0.0.0/0), engine em rede isolada"
    docker rm "$ENGINE" >/dev/null
    start_engine isolado
    connect_tunnel initiator-full.conf
    local routes default_before default_after
    routes=$(docker exec "$ENGINE" ip -4 route show proto "$RT_PROTO")
    [[ $routes == *"0.0.0.0/1 dev $IFACE"* && $routes == *"128.0.0.0/1 dev $IFACE"* ]] ||
        fail "rotas /1 do túnel completo ausentes: $routes"
    [[ $routes == *"$gw_ip "*"dev eth0"* ]] || fail "rota de host para o gateway ausente: $routes"
    ok "rotas 0.0.0.0/1 + 128.0.0.0/1 via $IFACE e rota de host para $gw_ip via eth0"
    default_before=$(docker exec "$ENGINE" ip -4 route show default)
    docker exec "$ENGINE" ping -c 3 -W 2 -q 192.0.2.1 >/dev/null || fail "ping pelo túnel completo falhou"
    ok "ping 192.0.2.1 pelo túnel completo (o IKE/ESP não entrou em loop)"
    docker exec "$ENGINE" swanctl --terminate --uri "$VICI_URI" --ike teste --timeout 10 >/dev/null 2>&1 ||
        fail "terminate falhou"
    routes=$(docker exec "$ENGINE" ip -4 route show proto "$RT_PROTO")
    [[ -z $routes ]] || fail "rotas do túnel completo sobraram após o down: $routes"
    default_after=$(docker exec "$ENGINE" ip -4 route show default)
    [[ $default_before == "$default_after" ]] || fail "rota padrão alterada"
    ok "updown down removeu as rotas /1 e a rota de host; rota padrão intacta"
    docker stop -t 10 "$ENGINE" >/dev/null

    # ------------------------------------------------------------------ 4 --
    step "Cenário 4: PFS só com modp768 (DH1) na fase 2, como VPNs legadas"
    docker rm "$ENGINE" >/dev/null
    start_engine isolado
    connect_tunnel initiator-modp768.conf
    docker exec "$ENGINE" swanctl --list-sas --uri "$VICI_URI" 2>/dev/null | grep -q "MODP_768" ||
        fail "a fase 2 não negociou MODP_768"
    docker exec "$ENGINE" ping -c 2 -W 2 -q 203.0.113.1 >/dev/null || fail "ping com PFS modp768 falhou"
    ok "fase 2 com PFS MODP_768 negociada e ping 203.0.113.1 pelo túnel"
    docker stop -t 10 "$ENGINE" >/dev/null

    # ------------------------------------------------------------------ 5 --
    if ((FORCED)); then
        step "Cenário 5: cliente VICI malicioso (updown e if_id próprios)"
        docker rm "$ENGINE" >/dev/null
        start_engine
        connect_tunnel initiator-evil.conf
        [[ ! -e "$rundir/PWNED" ]] || fail "o charon executou o updown do cliente (como root)"
        docker logs "$ENGINE" 2>&1 | grep -q "ignoring updown command supplied by client" ||
            fail "o charon não registrou que ignorou o updown do cliente"
        ok "updown do cliente ignorado: nenhum comando dele rodou"
        route=$(ip -4 route show dev "$IFACE" proto "$RT_PROTO")
        [[ $route == *"192.0.2.0/24"* ]] || fail "o updown forçado não instalou a rota: '$route'"
        [[ $(resolvectl domain "$IFACE") == *"~ipsec-client.test"* ]] ||
            fail "o updown forçado não aplicou o DNS do root"
        ok "o updown forçado rodou: rota 192.0.2.0/24 e DNS ~ipsec-client.test"
        states=$(count_xfrm state)
        local evil
        evil=$(host_ip -o xfrm state | grep -c "if_id 0x3e7" || true)
        ((states >= 2 && evil == 0)) || fail "if_id do cliente usado ($states SAs com $HEX_ID, $evil com 0x3e7)"
        ok "SAs com o if_id forçado $HEX_ID (nenhuma com o 999 do cliente)"
        docker stop -t 10 "$ENGINE" >/dev/null
        ip link show "$IFACE" >/dev/null 2>&1 && fail "a $IFACE sobrou"
        ok "limpeza ok"
    fi

    step "Resultado"
    ok "todos os cenários passaram"
}

main "$@"
