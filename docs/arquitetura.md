# Arquitetura

Este documento descreve **como** o IPsec Client funciona. O **porquê** de cada
escolha está nos [ADRs](adr/README.md).

## Visão geral

```
React UI ──(tauri-specta: comandos/eventos tipados)──► src-tauri (comandos finos, estado, tray)
                                                          │
                                               crates/core (ipsec-client-core)
   profiles    modelo, validação, presets, import/export, geração de load-conn/updown
   secrets     keyring (Secret Service) + memória (secrecy/zeroize) + redação
   engine      imagem embutida (hash) + ciclo do container (bollard)
   session     máquina de estados, VICI (trait Charon), eventos, erros
   diagnostics docker/grupo, portas UDP 500/4500, resolved, keyring, xfrm_interface
                 │ Docker API                                   │ VICI (socket Unix)
                 ▼                                               ▼
  container engine (ubuntu:20.04 + strongSwan 5.8.2), --network host, recriado a cada conexão
    charon → VIP e SAs/políticas XFRM (if_id) no kernel do host, via ipsc0
    updown.sh → rotas na ipsc0 + DNS split (resolvectl → systemd-resolved do host)

crates/cli (ipsec-client-cli): a mesma lógica, sem GUI (scripts, testes E2E headless)
```

## Nomes e locais

| Item | Valor |
|---|---|
| Imagem | `ipsec-client-engine:5.8.2-<hash8>` (label `io.github.ipsec-client.engine.hash`) |
| Container | `ipsec-client-engine` |
| Interface XFRM | `ipsc0`, MTU 1400, `if_id` derivado do perfil (≠ 0) |
| Socket VICI (host) | `$XDG_RUNTIME_DIR/ipsec-client/charon.vici` (diretório 0700, socket 0660 `root:<gid do usuário>`) |
| Socket VICI (container) | `/run/ipsec-client/charon.vici` |
| Perfis | `~/.config/ipsec-client/profiles/<id>.json` |
| Chaveiro | serviço `ipsec-client`, chave `<perfil>:<campo>` |

## O engine (container)

Os arquivos ficam em [`engine/`](../engine).

| Arquivo | Função |
|---|---|
| `Dockerfile` | `ubuntu:20.04` fixado por digest; strongSwan fixado em `5.8.2-1ubuntu3.6`; `systemd` (só pelo `resolvectl`) |
| `entrypoint.sh` | Valida o ambiente; recria `ipsc0`; escreve a config de runtime; sobe o charon; ajusta a permissão do socket; no `SIGTERM`, encerra o charon e limpa a interface e o DNS |
| `updown.sh` | Chamado pelo charon a cada TS que sobe ou desce; instala/remove rotas na `ipsc0` e configura o DNS split |
| `strongswan.d/zz-ipsec-client.conf` | Socket VICI no diretório compartilhado, `install_routes = no`, plugin `resolve` desligado, `updown.dns_handler = yes` (repassa os DNS recebidos ao updown), log no stderr (nível 1) |

### Variáveis de ambiente do container

Todas são **não sensíveis**: os segredos nunca passam por aqui.

| Variável | Exemplo | Validação no entrypoint |
|---|---|---|
| `IPSEC_CLIENT_IFACE` | `ipsc0` | `^[a-z][a-z0-9_-]{0,14}$` |
| `IPSEC_CLIENT_IF_ID` | `4096` | inteiro 1–4294967295 |
| `IPSEC_CLIENT_MTU` | `1400` | inteiro 576–9000 |
| `IPSEC_CLIENT_SOCKET_GID` | `1000` | inteiro ≥ 0 |
| `IPSEC_CLIENT_LOG_LEVEL` | `1` | inteiro 0–2 (nunca 3+, que pode mostrar material sensível) |

### Argumentos do `updown`

O `updown` é configurado por child SA na mensagem `load-conn`. O charon o
executa via `/bin/sh -c`, por isso cada argumento vai entre aspas simples e é
validado duas vezes: no Rust e no script.

```
/usr/local/libexec/ipsec-client/updown --iface 'ipsc0' \
    [--dns '192.0.2.53']... [--domain '~corp.example.com']... \
    [--use-received-dns] [--dns-default-route]
```

As rotas **não** vêm de argumentos. O script lê o TS negociado
(`PLUTO_PEER_CLIENT`) e o VIP (`PLUTO_MY_SOURCEIP`) do ambiente que o charon
fornece.

Todas as rotas da sessão recebem `proto 241`. Isso permite remover só elas,
inclusive a rota de host para o gateway no modo túnel completo, que fica fora
da `ipsc0`.

### Contrato do engine para o app

- **Prontidão.** O engine está pronto quando escreve `[engine] pronto` no log
  **e** o socket VICI aceita conexão. Um arquivo de socket sozinho **não**
  indica prontidão: um engine morto com `SIGKILL` deixa o socket velho no
  diretório. O app apaga o socket antes de criar o container. Essa condição
  de corrida foi encontrada pelo teste E2E.
- **Encerramento.** `docker stop` (SIGTERM). O entrypoint encerra o charon, que
  envia DELETE ao gateway, e remove interface, rotas `proto 241`, DNS, SAs
  XFRM com o `if_id` e o socket. Termina com código 0.
- **Morte abrupta (SIGKILL).** O próximo engine remove a interface velha, as
  rotas, o DNS e as SAs com o `if_id`. **Limitação conhecida:** as
  *políticas* XFRM com `if_id` não podem ser removidas pelo iproute2 5.5 do
  Ubuntu 20.04 (`delete`/`deleteall` não aceitam `if_id`, e o kernel o exige).
  - Elas são inertes: só casam com tráfego de uma interface XFRM com aquele
    `if_id`.
  - Uma sessão nova com o mesmo VIP as substitui, e elas são removidas no
    encerramento.
  - Com outro VIP, ficam até o reboot.
  - O entrypoint registra um aviso com a contagem.
- **Logs.** O charon roda no nível 1 e não registra PSK nem senha (há uma
  asserção no E2E). Ele **registra o usuário XAuth**
  (`XAuth authentication of '<usuário>'`), então o app mascara as identidades
  conhecidas antes de exibir o log (ADR-0007).

## Fluxo de conexão (`core::session`)

1. **Preflight:**
   - Docker acessível; se não estiver, dica de `usermod -aG docker`;
   - UDP 500/4500 livres no host;
   - systemd-resolved ativo;
   - credenciais completas, vindas do chaveiro ou do diálogo.
2. **Imagem e container:**
   - `ensure_image`;
   - `rm -f` do container anterior;
   - `create` + `start` com as variáveis da tabela acima.
3. **Entrypoint:**
   - recria a `ipsc0`;
   - sobe o charon;
   - ajusta o socket VICI;
   - fica em `wait`.
4. **Carga** via VICI: o app espera o socket, assina os eventos e envia
   `load-shared` (PSK, XAuth) → `load-conn` → `initiate`. O log de controle
   aparece ao vivo.
5. **Conectado:**
   - o evento `child-updown up` leva ao estado *Conectado*, com VIP, TS e tempo
     de conexão;
   - um `list-sas` periódico atualiza os bytes de entrada e saída.
6. **Desconectar:** `terminate` → `stop` (10 s) → `rm`. Ao abrir, o app se
   reanexa a um engine que já esteja rodando.
7. **Erros com mensagem clara:**

   | Erro | O que a mensagem aponta |
   |---|---|
   | `AUTHENTICATION_FAILED` na fase 1 | PSK ou id do grupo; sugere o tipo `keyid:` |
   | Falha de XAuth | usuário ou senha |
   | `NO_PROPOSAL_CHOSEN` | propostas ou PFS |
   | Timeout | gateway, rede ou NAT |

### Máquina de estados

```
Desconectado ─connect─► Preparando (preflight, imagem, container)
     ▲                       │ ok
     │                       ▼
     │                  Negociando (load-shared/load-conn/initiate)
     │                       │ child-updown up
     │                       ▼
     ├────disconnect──── Conectado ◄──── (rekey / DPD restart)
     │                       │ falha / child-updown down sem retorno
     └──────────────────  Erro (mensagem + log)
```

## Teste ponta a ponta do engine

O script [`tests/engine/e2e.sh`](../tests/engine/e2e.sh) roda sem nenhuma VPN
real:

1. Cria uma rede Docker isolada.
2. Sobe um **gateway de teste** com a mesma imagem do engine em modo
   responder. Ele usa IKEv1 agressivo, PSK + XAuth e modp1024/modp768, e
   anuncia a rede `192.0.2.0/24`, distribui VIPs de `198.51.100.0/24` e o DNS
   `192.0.2.53`.
3. Sobe o engine como o app sobe (`--network host`, só `NET_ADMIN`). Carrega
   uma conexão de teste com credenciais falsas e inicia o túnel.
4. Verifica:
   - `ipsc0` com MTU 1400 e o VIP;
   - a rota `192.0.2.0/24` na `ipsc0`;
   - o DNS da `ipsc0` no systemd-resolved;
   - o socket VICI com dono e permissão corretos, e um `version` VICI feito
     pelo usuário comum;
   - um **ping através do túnel**.
5. Para o engine e confere que não sobrou interface, rota, DNS, política XFRM
   com o `if_id` do teste nem socket.

Para não colidir com uma sessão real, o teste usa a interface `ipsct0` e um
`if_id` próprio. Ele aborta se as portas 500/4500 já estiverem em uso.
