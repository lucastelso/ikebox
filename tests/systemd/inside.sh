#!/usr/bin/env bash
# Parte do tests/systemd/run.sh que roda DENTRO do "computador" de teste
# (container Fedora com systemd), como root. Credenciais e endereços são do
# gateway de teste (FALSOS, RFC 5737).
#
# Uso: inside.sh <IP do gateway de teste>

set -euo pipefail

readonly GW_IP=${1:?ip do gateway}
readonly CLI=/test/ipsec-client-cli
readonly IFACE=ipsc0
readonly RT_PROTO=241
readonly ALICE_SOCKET=/run/ipsec-client/1000/charon.vici
readonly SWANCTL=/opt/ipsec-client/engine/sbin/swanctl
readonly CREDS='psk=ipsec-client-test-psk
xauth-user=usuario.teste
xauth-password=senha-de-teste'

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }
fail() {
    printf '  \033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

# as USUARIO CMD...: roda como o usuário, com config própria e o engine
# embutido forçado.
as() {
    local user=$1
    shift
    runuser -u "$user" -- env XDG_CONFIG_HOME="/tmp/$user-config" \
        IPSEC_CLIENT_BACKEND=native "$@"
}
# helper USUARIO JSON: pedido cru ao helper.
helper() { runuser -u "$1" -- python3 -I /test/helper_client.py "$2"; }
# A saída do `ip xfrm state` tem chaves: só é contada, nunca impressa.
count_xfrm_state() { ip -o xfrm state 2>/dev/null | grep -c "if_id $1" || true; }

host_is_clean() {
    ip link show "$IFACE" >/dev/null 2>&1 && fail "a $IFACE sobrou"
    [[ -z $(ip -4 route show proto "$RT_PROTO") ]] || fail "sobraram rotas proto $RT_PROTO"
    [[ ! -e $ALICE_SOCKET ]] || fail "o socket VICI sobrou"
    ok "nada sobrou: interface, rotas e socket"
}

engine_prop() { systemctl show ipsec-client-engine -p "$1" --value; }

step "Instalação pelo instalador real"
bash /test/bundle/install-native.sh install --bundle /test/bundle --origin manual \
    >/tmp/install.log 2>&1 || {
    cat /tmp/install.log
    fail "o instalador falhou"
}
# Sem polkit no container: autorização por lista de uids. Só o root troca a
# linha de comando do helper.
install -d /etc/systemd/system/ipsec-client-helper.service.d
printf '[Service]\nExecStart=\nExecStart=/opt/ipsec-client/bin/ipsec-client-helper --allow-uid 1000 --allow-uid 1001\n' \
    >/etc/systemd/system/ipsec-client-helper.service.d/teste.conf
systemctl daemon-reload
systemctl stop ipsec-client-helper.service 2>/dev/null || true
useradd -m -u 1000 alice
useradd -m -u 1001 bob
ok "$(tail -n 1 /tmp/install.log)"

step "alice conecta pelo ipsec-client-cli (DNS do perfil vai pelo helper)"
sed "s/\"gw.ipsec-client.test\"/\"$GW_IP\"/" /test/perfil-gateway-teste.json >/tmp/perfil.json
chmod 0644 /tmp/perfil.json
as alice "$CLI" profiles import /tmp/perfil.json >/dev/null
out=$(as alice "$CLI" connect gateway-teste --detach --credentials-stdin <<<"$CREDS" 2>&1) || {
    printf '%s\n' "$out"
    fail "connect falhou"
}
[[ $out == *"Conectado: gateway-teste"* ]] || fail "saída inesperada do connect: $out"
ok "conectado"

[[ $(stat -c '%a %U' "$ALICE_SOCKET") == "600 alice" ]] || fail "socket VICI sem 0600/alice"
[[ $(stat -c '%a %U %G' "$(dirname "$ALICE_SOCKET")") == "711 root root" ]] ||
    fail "diretório do socket sem 0711 root:root"
ok "socket VICI 0600 de alice, diretório 0711 do root"

route=$(ip -4 route show dev "$IFACE" proto "$RT_PROTO")
[[ $route == *"192.0.2.0/24"* ]] || fail "rota ausente: '$route'"
[[ $(resolvectl domain "$IFACE") == *"~ipsec-client.test"* ]] || fail "domínio do perfil ausente"
[[ $(resolvectl dns "$IFACE") == *192.0.2.53* ]] || fail "DNS recebido do gateway ausente"
ping -c 2 -W 2 -q 192.0.2.1 >/dev/null || fail "ping pelo túnel falhou"
ok "rota 192.0.2.0/24, DNS ~ipsec-client.test + 192.0.2.53 (via helper) e ping pelo túnel"

step "Sandbox da unidade do engine (systemd-run)"
for prop in PrivateDevices=yes ProtectKernelTunables=yes ProtectKernelLogs=yes \
    ProtectProc=invisible PrivateIPC=yes NoNewPrivileges=yes ProtectSystem=strict; do
    [[ $(engine_prop "${prop%%=*}") == "${prop#*=}" ]] || fail "$prop não aplicado"
done
[[ -n $(engine_prop SystemCallFilter) ]] || fail "SystemCallFilter não aplicado"
pid=$(engine_prop MainPID)
[[ $(awk '$5 == "/proc/sys" { print $6 }' "/proc/$pid/mountinfo") == ro* ]] ||
    fail "/proc/sys gravável dentro do engine"
# has_disks DIR: há nós de disco (sdX, nvmeN, vdX) no /dev dado?
has_disks() { compgen -G "$1/sd[a-z]" >/dev/null || compgen -G "$1/nvme[0-9]*" >/dev/null ||
    compgen -G "$1/vd[a-z]" >/dev/null; }
if has_disks /dev && has_disks "/proc/$pid/root/dev"; then
    fail "o engine enxerga os discos do computador"
fi
ok "PrivateDevices, ProtectKernelTunables (/proc/sys só leitura), ProtectKernelLogs, ProtectProc, PrivateIPC, SystemCallFilter"

step "bob (outro usuário) tenta mexer na VPN de alice"
reply=$(helper bob '{"op":"stop"}')
[[ $reply == *'"result":"error"'* && $reply == *"outro usuário"* ]] || fail "bob parou a VPN de alice: $reply"
ip link show "$IFACE" >/dev/null 2>&1 || fail "a VPN de alice caiu"
reply=$(helper bob '{"op":"logs","tail":50}')
[[ $reply == '{"result":"logs","lines":[]}' ]] || fail "bob leu o log de alice"
reply=$(helper bob '{"op":"status"}')
[[ $reply == *'"running":true'* && $reply == *'"profile_id":null'* ]] || fail "bob viu o perfil de alice: $reply"
if runuser -u bob -- python3 -I -c "import socket; socket.socket(socket.AF_UNIX).connect('$ALICE_SOCKET')" 2>/dev/null; then
    fail "bob conectou no socket VICI de alice"
fi
ok "bob não para, não lê o log, não vê o perfil e não acessa o VICI de alice"

for _ in 1 2 3; do helper bob '{}' --hold 5 & done
sleep 0.3
timeout 2 runuser -u alice -- python3 -I /test/helper_client.py '{"op":"status"}' >/dev/null ||
    fail "clientes parados travaram o helper"
wait
ok "três conexões paradas de bob não travam o helper"

step "alice: status, log e desconexão"
out=$(as alice "$CLI" status) || fail "status deveria sair com 0 quando conectado"
[[ $out == *"IP virtual: 198.51.100."* ]] || fail "status sem VIP"
out=$(as alice "$CLI" logs -n 200)
[[ $out == *pronto* ]] || fail "log sem a linha de prontidão"
out=$(as alice "$CLI" disconnect)
[[ $out == *Desconectado* ]] || fail "disconnect falhou"
host_is_clean

step "Ataque (ADR-0019): cliente VICI escolhe o updown e o if_id"
sed -e "s|@GATEWAY@|$GW_IP|" -e "s|@IFACE@|$IFACE|" -e "s|@IF_ID@|999|" \
    -e "s|@REMOTE_TS@|192.0.2.0/24|" -e "s|@UPDOWN_EXTRA@||" \
    -e "s|updown = .*|updown = /bin/touch /run/ipsec-client/1000/PWNED|" \
    /test/initiator.conf.in >/tmp/ataque.conf
chmod 0644 /tmp/ataque.conf
reply=$(helper alice '{"op":"start","iface":"ipsc0","if_id":4242,"mtu":1400,"log_level":1,"profile_id":"ataque","dns":{"servers":["192.0.2.54"],"domains":["~ipsec-client.test"],"use_received":false,"default_route":false}}')
[[ $reply == *'"result":"started"'* ]] || fail "start falhou: $reply"
runuser -u alice -- "$SWANCTL" --load-all --uri "unix://$ALICE_SOCKET" --file /tmp/ataque.conf \
    >/tmp/load.log 2>&1 || {
    cat /tmp/load.log
    fail "swanctl --load-all falhou"
}
runuser -u alice -- "$SWANCTL" --initiate --uri "unix://$ALICE_SOCKET" --child rede-teste \
    --timeout 30 >/tmp/initiate.log 2>&1 || {
    cat /tmp/initiate.log
    fail "a negociação falhou"
}
[[ ! -e /run/ipsec-client/1000/PWNED ]] || fail "o charon executou o comando do cliente como root"
# Lido inteiro antes do grep: com pipefail, o `grep -q` sairia cedo e o
# journalctl morreria com SIGPIPE.
journal=$(journalctl -u ipsec-client-engine --no-pager)
grep -q "ignoring updown command supplied by client" <<<"$journal" ||
    fail "o charon não registrou que ignorou o updown do cliente"
ok "o comando do cliente não rodou (o charon ignorou o updown dele)"
route=$(ip -4 route show dev "$IFACE" proto "$RT_PROTO")
[[ $route == *"192.0.2.0/24"* ]] || fail "o updown forçado não rodou: '$route'"
[[ $(resolvectl dns "$IFACE") == *192.0.2.54* ]] || fail "o DNS do helper não foi aplicado"
forced=$(count_xfrm_state 0x1092)
evil=$(count_xfrm_state 0x3e7)
((forced >= 2 && evil == 0)) || fail "if_id do cliente usado ($forced com 0x1092, $evil com 0x3e7)"
ok "o updown forçado rodou (rota e DNS do helper) e as SAs usam o if_id do helper"
reply=$(helper alice '{"op":"stop"}')
[[ $reply == *'"was_running":true'* ]] || fail "stop falhou: $reply"
host_is_clean

step "Resultado"
ok "engine embutido com systemd: túnel, sandbox, isolamento entre usuários e ataque bloqueado"
