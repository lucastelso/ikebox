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

## O engine embutido (direção atual, ADRs 0013–0016)

O strongSwan **6.1.0** é compilado a partir do código-fonte oficial
(`native-engine/`) e roda no host, iniciado pelo helper com privilégio
(ADR-0014, fase D). Ele usa **os mesmos scripts e o mesmo contrato** do engine
em container descrito abaixo. A diferença é que o charon vem de
`/opt/ipsec-client/engine/libexec/ipsec/charon`, configurado por
`IPSEC_CLIENT_CHARON` e `IPSEC_CLIENT_STRONGSWAN_D`. Até a fase D, o engine
em container continua sendo o backend em uso e o dos testes.

### Componente de rede: `ipsec-client-helper` (ADR-0014)

| Peça | O que faz |
|---|---|
| `crates/proto` | Protocolo JSON por linha (`ping`, `start`, `stop`, `status`, `logs`) e as **regras de validação compartilhadas** com o `core`. Campos desconhecidos são recusados, então o cliente não consegue passar caminhos nem comandos |
| `crates/helper` | Root, ativado por socket (`/run/ipsec-client-helper.sock`, 0666). Faz `SO_PEERCRED` e consulta o polkit (`io.github.lucastelso.ipsecclient.connect`, `allow_active=yes`, sem interação). Cria `/run/ipsec-client/<uid>` (0711 root:root) e sobe o engine com `systemd-run --unit=ipsec-client-engine` e sandbox, passando o DNS do perfil (validado). Espera o socket VICI ser entregue ao usuário (0600, dono = uid) e responde com o caminho dele. Recusa se o engine for de outro usuário; `stop` e `logs` só para o dono. Atende até 8 conexões em paralelo (ADR-0019) |
| Sandbox do engine | Capabilities `NET_ADMIN`, `NET_BIND_SERVICE`, `NET_RAW` e `CHOWN`; `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `PrivateDevices`, `PrivateIPC`, `ProtectKernelModules`, `ProtectKernelTunables`, `ProtectKernelLogs`, `ProtectProc=invisible`, `SystemCallFilter=@system-service`, `RestrictAddressFamilies` (unix, inet, inet6, netlink), `MemoryDenyWriteExecute`, `UMask=0077`; gravável só `RuntimeDirectory` e o diretório do socket do usuário |
| `entrypoint.sh` (modo nativo) | `IPSEC_CLIENT_STATE_DIR`: a config de runtime vai para o diretório gravável e entra pelo `STRONGSWAN_CONF`, incluindo a base. `IPSEC_CLIENT_SOCKET_DIR`/`SOCKET_UID`: socket por usuário, `0600` do uid. `IPSEC_CLIENT_UPDOWN` + `DNS_*`: escreve `updown.args` (um argumento por linha) e as opções `force_updown`/`force_if_id` do patch do strongSwan, que fazem o charon ignorar o `updown` e o `if_id` vindos do VICI (ADR-0019) |
| `core::engine::Backend` | `Native` (helper) quando instalado, ou `Docker` como reserva. `IPSEC_CLIENT_BACKEND=native\|docker` força um deles. A sessão, o diagnóstico e o `logs` do CLI usam o `Backend` |
| `packaging/` | Units `ipsec-client-helper.{socket,service}` (o helper também tem hardening), policy do polkit e `install-native.sh`. O instalador recebe um pacote (`--bundle`), confere o `SHA256SUMS` numa cópia só do root, instala em `/opt/ipsec-client`, registra `VERSION`/`SHA256SUMS`/`ORIGIN` e aplica o `restorecon` por causa do SELinux |
| `core::component` (ADR-0018) | Pacote do componente (`scripts/stage-network-component.sh`, que vai em `<recursos>/network` no build de release; o helper vai compactado, porque o linuxdeploy do AppImage reescreve executáveis ELF), estado (não instalado, pronto, desatualizado ou com problema), instalação via `pkexec` a partir de uma cópia privada em `$XDG_RUNTIME_DIR` e remoção pelo instalador do root. Usado pelo app (Sistema, aviso de primeiro uso) e pelo `ipsec-client-cli component` |

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

## Módulo `core::engine` (Rust)

| Item | O que faz |
|---|---|
| `engine::context` | `FILES` com os arquivos de `engine/` (via `include_bytes!`); `content_hash()` (SHA-256 de caminho + tamanho + conteúdo); `image_tag()`; `tar_archive()` determinístico. Um teste falha se `engine/` ganhar um arquivo que não está em `FILES` |
| `EngineSpec` | Configuração pura de uma instância: `validate()` com as mesmas regras do entrypoint, `env()`, `host_config()` (rede do host, só `NET_ADMIN`, binds, `label=disable`/`apparmor=unconfined` conforme o host, privilegiado só como reserva), `create_body()` com labels |
| `DockerEngine` | `connect` (só pelo socket Unix: o bollard é compilado sem HTTP/TCP), `ping`, `ensure_image` (progresso sem códigos ANSI), `start` (diretório de runtime 0700 do usuário, sem symlink; stop gracioso do anterior; prontidão), `stop` (idempotente), `is_running` |
| `EngineError` | Mensagens em pt-BR para o usuário. `NotReady` leva as últimas linhas do log do entrypoint, que não contêm segredos |

## Módulo `core::vici` (Rust, ADR-0011)

| Item | O que faz |
|---|---|
| `Message`/`Value` | Mensagem VICI ordenada; `encode_into`/`decode`; valores sempre em buffers zerados no drop; `Debug` sem valores; profundidade máxima 32 |
| `ViciClient` | Conexão de comandos: `request` e `streamed_request`, que assina o evento, coleta os itens e cancela a assinatura (usado por `list-sas` e `list-conns`) |
| `ViciEvents` | Conexão só de eventos: `subscribe(["ike-updown", "child-updown", "log"])` + `next()` |
| `Charon` (trait) | `version`, `load_shared`, `unload_shared`, `load_conn`, `unload_conn`, `list_conns`, `initiate`, `terminate`, `list_sas`; `success = no` vira `ViciError::CommandFailed` com o `errmsg` do charon |
| `SharedSecret` | Segredo para o `load-shared` (`IKE`/`XAUTH`/`EAP`), com dados zerados e `Debug` sem dados nem donos |
| `IkeSa`/`ChildSa`/`CharonEvent` | Respostas e eventos tipados, **sem identidades**. As linhas de log (`CharonEvent::Log`) podem conter o usuário XAuth, e quem exibe deve mascarar |

## Módulo `core::profiles` (Rust)

| Item | O que faz |
|---|---|
| `Profile` | Modelo serde (`deny_unknown_fields`), sem credenciais. Formato em [perfis.md](perfis.md) |
| `validate` | Devolve **todos** os problemas, campo a campo (para a UI). Protege o `load-conn` e o `updown` |
| `Profile::to_load_conn` | Gera a mensagem VICI. O usuário XAuth vem de `ConnParams`, nunca do perfil |
| `Profile::updown_command` | Argumentos entre aspas simples. `shell_quote` recusa aspas e caracteres de controle, como segunda barreira |
| `Profile::if_id` | Derivado do id do perfil (SHA-256 → 1..=2³¹-1) |
| `Profile::required_credentials` | `psk`, ou `psk` + `xauth-user` + `xauth-password` |
| `ProfileStore` | `list`/`load`/`save` (atômico, 0600 em diretório 0700)/`delete`/`import` (limite de 1 MiB, recusa sobrescrever)/`export` |
| `parse_profile` | Confere `schema_version` antes do resto, que é o ponto de entrada de migrações futuras |

## Módulo `core::secrets` (Rust, ADR-0007)

| Item | O que faz |
|---|---|
| `Credentials` | PSK, usuário e senha XAuth em `SecretString` (zerada no drop). `Debug` mostra só presença/ausência e o usuário mascarado. `missing(perfil)`, `load`/`save`/`forget` num `SecretStore`, `shared_secrets(perfil)` → `SharedSecret` para o `load-shared` (PSK com dono = `local_id`, XAuth com dono = usuário) |
| `SecretStore` (trait) | `get`/`set`/`delete` por (perfil, campo) |
| `KeyringStore` | Secret Service (GNOME Keyring/KWallet) via **libdbus** (`keyring-core` + `dbus-secret-service-keyring-store`, cripto em Rust). Serviço `ipsec-client`, chave `<perfil>:<campo>`, rótulo "IPsec Client: <perfil> (<campo>)". Chamadas bloqueantes: usar `spawn_blocking` |
| `MemoryStore` | Só memória (testes e "não lembrar") |
| `Redactor`/`mask` | Mascara identidades conhecidas em linhas de log: usuário XAuth e id do grupo, com e sem prefixo `keyid:`. Exemplo: `usuario.exemplo` → `us***lo` |

## Módulos `core::diagnostics` e `core::session` (Rust)

| Item | O que faz |
|---|---|
| `diagnostics::preflight`/`doctor` | Verificações sem root: usuário, runtime dir, Docker, `xfrm_interface`, resolved, D-Bus, portas 500/4500 (por `/proc/net/udp*`), chaveiro e imagem. Cada item vem com dica de correção |
| `session::connect` | Credenciais → conexão já ativa? → preflight → imagem → engine (com a label do perfil) → eventos VICI → `establish` → `list-sas`. Em qualquer falha, derruba o engine |
| `session::establish` | Genérica sobre `Charon`: segredos antes da conexão, depois `load-conn` e `initiate` |
| `session::diagnose` | Causa provável da falha, a partir de linhas reais do charon 5.8.2 |
| `session::status`/`disconnect` | Funcionam de qualquer processo (o perfil ativo vem da label do container) |
| `SessionEvent` | Estado, log mascarado, progresso do build, queda e retorno do túnel, fim do engine |

## App desktop (`src-tauri` + `src`)

| Camada | O que faz |
|---|---|
| `src-tauri/src/commands.rs` | Comandos `async`, finos sobre o core. Disco e chaveiro vão para `spawn_blocking`, e o `ConnectionLock` impede dois `connect` simultâneos. As credenciais digitadas completam as guardadas e viram `SecretString` na hora |
| `src-tauri/src/dto.rs` | Contrato com a UI: `ProfileSummary`, `CredentialsStatus` (só presença e usuário mascarado), `CredentialsInput` (segredos só entram, `Debug` redigido), `Connection`, `SessionUpdate` (evento) e `UiError` (tipo + mensagem + detalhes) |
| `src-tauri/src/desktop.rs` | Bandeja (`tray-icon`), monitor de estado a cada 3 s com notificações nas transições (`transition_notice`, puro), notificações por `org.freedesktop.Notifications` via libdbus e instância única por socket em `$XDG_RUNTIME_DIR`. Sem zbus |
| `src/bindings.ts` | Gerado pelo tauri-specta (modo `Result`: erros tipados). Um teste Rust garante que está em dia |
| `src/features/connection/` | `connectionReducer` (fases, puro e testado), `useConnection` (eventos + comandos + status a cada 2 s), `useProfiles`, a `HomeScreen` e os componentes |

## Fluxo de conexão (`core::session`)

1. **Preflight:**
   - Docker acessível; se não estiver, dica de `usermod -aG docker`;
   - UDP 500/4500 livres no host;
   - systemd-resolved ativo;
   - credenciais completas, vindas do chaveiro ou do diálogo.
2. **Imagem e container** (`core::engine::DockerEngine`):
   - `ensure_image`: se a tag com o hash não existe, faz o build pela Docker API
     a partir do tar em memória;
   - o engine anterior, se houver, sai por `docker stop`, para que o entrypoint
     limpe o host, e é removido. O socket velho é apagado;
   - `create` + `start` com as variáveis da tabela acima;
   - espera a linha `[engine] pronto` e uma conexão bem-sucedida ao socket.
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
4. Verifica, na interface do teste (`ipsct0`, no lugar da `ipsc0`):
   - interface XFRM com MTU 1400 e o VIP;
   - a rota `192.0.2.0/24` na interface;
   - o DNS da interface no systemd-resolved: servidor fixo, servidor recebido
     do gateway e domínio `~ipsec-client.test`;
   - o socket VICI com dono e permissão corretos, e um `version` VICI feito
     pelo usuário comum;
   - `docker inspect` e logs do charon sem PSK nem senha;
   - um **ping através do túnel**.
5. Para o engine e confere que não sobrou interface, rota, DNS, política XFRM
   com o `if_id` do teste nem socket.
6. **Morte abrupta:** mata o engine com `SIGKILL`, sobe outro e confere que os
   restos foram limpos e que a reconexão funciona.
7. **Túnel completo** (`0.0.0.0/0`):
   - o engine roda no namespace de rede da rede de teste, não no do host, para
     não desviar o tráfego da máquina;
   - o socket D-Bus não é montado, porque o `resolvectl` resolveria o índice da
     interface no namespace errado;
   - verifica as rotas `/1` e a rota de host para o gateway, o ping sem loop e a
     remoção de tudo no *down*.

Para não colidir com uma sessão real, o teste usa a interface `ipsct0` e um
`if_id` próprio. Ele aborta se as portas 500/4500 já estiverem em uso. O IPv6
no `updown` ainda não tem teste automatizado.
