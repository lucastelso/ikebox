# Diário de desenvolvimento

Registro cronológico dos passos tomados, dos problemas encontrados e de como
foram resolvidos. As decisões formais estão nos [ADRs](adr/README.md). O
planejamento está no [roadmap](roadmap.md).

---

## 2026-11-15: ponto de partida e fundação

### De onde o projeto parte

Antes de existir este repositório, a VPN do trabalho do mantenedor (IPsec
legado em FortiGate: IKEv1 agressivo, PSK de grupo + XAuth) só funcionava no
Linux por um fluxo manual. Ele era composto por:

1. Uma imagem Docker `ubuntu:20.04` com strongSwan 5.8.2.
2. Um `swanctl.conf` montado no container, **com os segredos em texto puro**.
3. Um script bash, rodado com `sudo`, que:
   - subia o container com `--privileged`, `SYS_MODULE` e `/lib/modules`;
   - rodava `swanctl --load-all` e `--initiate` via `docker exec`;
   - extraía o IP virtual da saída de texto com `grep`;
   - adicionava o VIP e a rota **na interface física** (Wi-Fi), e o DNS também;
   - na saída, fazia `ip xfrm state flush` e `ip xfrm policy flush` **globais**.

O fluxo funciona e está validado. A análise mostrou o que precisava melhorar
para virar um app que outras pessoas possam usar:

| Problema no fluxo manual | Solução no app | ADR |
|---|---|---|
| Segredos em arquivo texto | VICI `load-shared` em memória + chaveiro opcional | 0003, 0007 |
| `--privileged` + `SYS_MODULE` | Só `NET_ADMIN` | 0005 |
| VIP/rota/DNS na interface física | Interface XFRM dedicada `ipsc0` | 0004 |
| Flush global do XFRM (derruba outros IPsec) | Limpeza atômica: apagar `ipsc0` | 0004 |
| Parse de texto (`grep`) do `swanctl` | Respostas e eventos estruturados via VICI | 0003 |
| Configuração fixa para uma VPN | Perfis genéricos em JSON | 0008 |
| Exige `sudo` a cada uso | Só o grupo `docker`; o root fica no container | 0005 |

### Passo 1: repositório e regra de privacidade

- `git init -b main`.
- O projeto vai ser publicado no GitHub, e por isso nasceu com a regra
  "nenhum dado real de VPN no repositório" (ADR-0010):
  - o material privado do mantenedor (plano original com dados do trabalho e a
    cópia do fluxo manual) foi listado em `.git/info/exclude`, que é local e
    nunca versionado;
  - foi criado o `scripts/check-private.sh`, que bloqueia commits que casem com
    padrões privados guardados em `.git/info/private-patterns`, também fora do
    git. A saída mostra só arquivo e linha, nunca o trecho.
- Testes manuais do guarda:
  - um arquivo com um IP e um domínio privados foi bloqueado;
  - palavras parecidas (`coisa`, `ssps`) não geraram falso positivo, graças às
    bordas de palavra (`\b`);
  - a mensagem de commit é verificada, e as linhas de comentário do git são
    ignoradas.
- Bug encontrado e corrigido antes do primeiro commit: com `set -o pipefail`, um
  `git grep` sem resultados (código 1) fazia o pipeline "falhar" e bloqueava
  commits limpos. A solução foi capturar o código de saída explicitamente. O
  mesmo cuidado valeu para `grep -q` em pipeline, que pode causar SIGPIPE.

### Passo 2: documentação de base

- README: o que é, para quem é e para quem não é, como funciona, segurança.
- Arquitetura, roadmap, `SECURITY.md` (modelo de ameaças e riscos aceitos),
  `CONTRIBUTING.md` (fluxo de branches, Conventional Commits em pt-BR, regra de
  privacidade).
- ADRs 0001–0010. As decisões 0001–0009 vêm do plano original, e a 0010
  (privacidade) foi acrescentada agora.

### Ambiente e mudança de ordem do roadmap

- O ambiente verificado:
  - Fedora 45, kernel 7.3, com o módulo `xfrm_interface` disponível e o `esp4`
    carregado;
  - systemd-resolved ativo, SELinux enforcing, daemon Docker sem SELinux;
  - Docker 29, Node 24, git 2.56.
- O que falta: **Rust**, as **bibliotecas de sistema do Tauri** (webkit2gtk etc.)
  e o **shellcheck**.
- A instalação do Rust no nível do usuário (rustup) foi barrada pela política de
  permissões da sessão automatizada, e as bibliotecas exigem `sudo`. Os dois
  ficam com o mantenedor (ver README, "Pré-requisitos").
- Por isso o roadmap foi reordenado. Primeiro vem o que dá para validar já,
  só com Docker e Node:
  1. ferramentas do repositório (hooks, commitlint, shellcheck via Docker);
  2. o **engine** (container), que é a parte de maior risco técnico, porque a
     troca de "VIP na interface física" por "interface XFRM dedicada" é a maior
     mudança em relação ao fluxo validado. Ele vem com um teste ponta a ponta
     contra um gateway IPsec local.

  O núcleo em Rust e a GUI vêm em seguida.

### Passo 3: ferramentas do repositório (`chore/tooling`)

- `package.json` na raiz, com versões **exatas**: lefthook 2.2.2, commitlint
  21.2.4. O npm 11 bloqueia scripts de instalação de dependências por padrão.
  O `postinstall` do lefthook ficou bloqueado, e os hooks são instalados pelo
  `prepare` do próprio projeto, que é explícito e auditável.
- Hooks (`lefthook.yml`):

  | Hook | O que roda |
  |---|---|
  | pre-commit | guarda de dados privados + shellcheck nos `.sh` staged |
  | commit-msg | guarda na mensagem + commitlint |
  | pre-push | guarda sobre o histórico inteiro |

- `scripts/lint-sh.sh`: usa o shellcheck do sistema ou, na falta dele, a imagem
  `koalaman/shellcheck:v0.11.0`, fixada por digest, sem rede e com o
  repositório só leitura. Assim o lint funciona antes de o mantenedor instalar
  o shellcheck.
- CI (`.github/workflows/ci.yml`):
  - actions fixadas por **SHA de commit**, com a tag em comentário (proteção
    contra tags reescritas na cadeia de suprimentos);
  - permissões mínimas (`contents: read`) e `persist-credentials: false`;
  - jobs: `npm run check`, commitlint no intervalo do PR e gitleaks sobre o
    histórico completo (imagem fixada por digest).
- O gitleaks foi rodado localmente sobre o histórico e não achou nada.
- O dependabot atualiza actions, npm e o digest da imagem base. Ele **ignora**
  saltos de versão do Ubuntu, porque o 20.04 é fixado de propósito (ADR-0001).

### Passo 4: engine (`feat/engine-imagem`)

Objetivo: validar, antes de qualquer código Rust, a maior mudança em relação ao
fluxo manual. O VIP, as rotas e o DNS saem da interface física e vão para uma
**interface XFRM dedicada**. O container sai do `--privileged` e fica só com
`NET_ADMIN`.

**O que foi feito**

- `engine/Dockerfile`:
  - `ubuntu:20.04` fixado por digest;
  - mesma lista de pacotes do fluxo validado, com o strongSwan fixado em
    `5.8.2-1ubuntu3.6`;
  - `systemd` sem recommends, só pelo `resolvectl`.
- `engine/entrypoint.sh`:
  - valida as variáveis;
  - limpa os restos de uma sessão anterior;
  - cria a `ipsc0` (`type xfrm if_id N`, MTU 1400);
  - sobe o charon;
  - libera o socket VICI para o gid do usuário (0660);
  - no SIGTERM, encerra o charon e desfaz tudo.
- `engine/updown.sh`:
  - rotas na interface XFRM marcadas com `proto 241`;
  - túnel completo com rotas `/1` e uma rota de host para o gateway;
  - DNS split via `resolvectl`, pelo D-Bus do host;
  - argumentos validados.
- `tests/engine/e2e.sh`: um **gateway IPsec de teste** (a mesma imagem em
  modo responder) imita uma VPN legada: IKEv1 agressivo, PSK de grupo `keyid:`
  + XAuth, modp1024/768, pool de VIPs e DNS. O engine conecta nele exatamente
  como o app vai fazer. São dois cenários: o ciclo normal e a recuperação
  após `SIGKILL`.

**Resultado:** túnel estabelecido, VIP na interface dedicada, rota, DNS split e
**ping através do túnel**. Depois do `docker stop`, nada sobra no host. Também
não sobra nada depois de um `SIGKILL` seguido de reconexão. A rota padrão e a
interface física ficam intactas. O teste leva cerca de 10 s e foi repetido
para descartar flakiness.

**Problemas encontrados e como foram resolvidos**

1. **DNS recebido do gateway não chegava ao updown.** O log mostrava
   `handling INTERNAL_IP4_DNS attribute failed`. Com o plugin `resolve`
   desligado, ninguém tratava o atributo. O plugin `updown` só repassa os DNS
   (`PLUTO_DNS4_<n>`) com `charon.plugins.updown.dns_handler = yes`, que vem
   desligado por padrão. Solução: ligar a opção.
2. **SAs órfãs não eram removidas.** O `ip -o xfrm state` separa os blocos com
   `\`, que ficava grudado nos campos (`dst 203.0.113.2\`). Solução: trocar
   `\` por espaço no awk antes de ler os campos. A saída do `ip xfrm state`
   contém chaves, por isso ela só é lida pelo awk e nunca impressa.
3. **Políticas órfãs com `if_id` não podem ser removidas pelo iproute2 5.5.**
   O problema foi investigado com uma política inofensiva (endereços de
   documentação e um `if_id` sem interface):
   - `delete` por seletor sem `if_id`: `ENOENT`;
   - `delete ... if_id`: o 5.5 não aceita o argumento;
   - `deleteall` com filtro: `ENOENT`;
   - `delete index N`: o kernel também exige o `if_id`.

   O iproute2 moderno (6.11/7.0) aceita `if_id` e remove a política.
   Análise: essas políticas só existem depois de um `SIGKILL` e são inertes,
   porque só casam com tráfego da interface XFRM com aquele `if_id`. O teste
   **provou** que elas não atrapalham a reconexão. Com o mesmo VIP, o charon as
   substitui (o seletor é o mesmo) e as remove no encerramento.

   Decisão: não embutir um segundo iproute2 na imagem agora. A limitação está
   documentada, o entrypoint avisa no log, e o item foi para o roadmap. A
   política de teste e as sobras das primeiras execuções foram removidas do
   kernel com um container descartável com iproute2 moderno.
4. **Condição de corrida no teste: socket velho tomado como "pronto".** Depois
   de um `SIGKILL`, o arquivo do socket continuava no diretório, com modo 0660,
   e o teste seguia antes de o engine novo terminar a limpeza. Solução: apagar
   o socket antes de subir o engine e esperar a linha `[engine] pronto`. Isso
   virou regra do **contrato do engine para o app** (ver arquitetura).
5. **O charon registra o usuário XAuth no nível 1.** Senha e PSK não aparecem,
   e há uma asserção automática para isso no E2E. Mas a linha
   `XAuth authentication of '<usuário>'` aparece, então o app vai mascarar as
   identidades conhecidas antes de exibir o log.

**Outros detalhes verificados**

- O `docker inspect` do engine não contém segredos (asserção no E2E).
- `version` via VICI funciona como **usuário comum** pelo bind mount do
  socket. O teste usa um cliente Python mínimo com o protocolo VICI. É a prova
  de que o app vai conseguir controlar o charon sem root.
- O kernel carrega o módulo `xfrm_interface` sozinho ao criar a interface,
  então não é preciso `SYS_MODULE` nem montar `/lib/modules`.

### Passo 5: túnel completo no teste (`test/engine-tunel-completo`)

- O caminho do `updown` para o túnel completo (`0.0.0.0/0`) ainda estava sem
  teste. É justamente o caminho que pode criar loop: se o tráfego IKE/ESP
  para o gateway entrar no próprio túnel, a conexão cai.
- Testar isso na rede do host desviaria todo o tráfego da máquina durante o
  teste. Solução: o cenário 3 roda o engine no namespace de rede da rede de
  teste.
- Cuidado identificado no caminho: **não montar o D-Bus do host nesse modo**.
  O `resolvectl` converte o nome da interface em índice no namespace dele e
  mandaria esse índice ao systemd-resolved do host, configurando o DNS de
  outra interface. Em produção isso não acontece, porque o engine sempre usa
  `--network host`.
- O gateway de teste ganhou um child `tudo` (`local_ts = 0.0.0.0/0`). O
  strongSwan escolhe o child com o TS mais exato, então o cenário 1 continua
  usando `rede-teste`.
- Resultado: rotas `0.0.0.0/1` + `128.0.0.0/1` na interface XFRM, rota de host
  para o gateway pelo caminho original, ping sem loop. No `terminate`, o
  *down* remove tudo, e a rota padrão fica intacta.

---

## 2026-11-16: núcleo em Rust

### Passo 6: pré-requisitos instalados pelo mantenedor

- O mantenedor instalou o Rust (rustup, toolchain estável 1.100.0), as libs do
  Tauri (webkit2gtk 4.1 etc.) e o shellcheck. O `npm run check` passou a usar o
  shellcheck nativo, e o fallback via Docker continua disponível.
- O mantenedor acrescentou ao `.git/info/private-patterns` o login XAuth e o IP
  da máquina do trabalho acessada por RDP. O histórico inteiro foi revarrido
  com os padrões novos, sem achados.
- O repositório privado foi criado no GitHub, e `main` foi enviado.
  Observação: os commits levam o e-mail pessoal do autor. Isso virou pendência
  para decidir antes de tornar o repositório público (`docs/retomar.md`).

### Passo 7: workspace Rust (`chore/scaffold-rust`)

- `Cargo.toml` do workspace:
  - edition 2024, resolver 3, `publish = false`;
  - lints de workspace: `unsafe_code = forbid`, `unwrap_used`/`expect_used`/
    `print_stdout` como aviso, e o clippy roda com `-D warnings`;
  - profile de release com LTO thin e strip.
- `rust-toolchain.toml` fixa o Rust em 1.100.0. Atualizar a versão exige um PR
  próprio.
- `deny.toml`:
  - licenças permissivas na lista de permitidas;
  - vulnerabilidades do RustSec e crates "yanked" negados;
  - só o registry do crates.io é aceito como origem;
  - os crates próprios ficam fora da checagem de licença enquanto ela não for
    definida.
- Crates:
  - `ipsec-client-core`, por enquanto só o esqueleto e a lista de módulos
    planejados;
  - `ipsec-client-cli`, com clap e um teste que valida a definição da CLI.
- `scripts/check-rust.sh` roda fmt → clippy → testes → cargo-deny e entrou no
  `npm run check`. O pre-commit roda `cargo fmt --check` quando há `.rs`
  staged.
- CI: um job Rust próprio, com a toolchain lida do `rust-toolchain.toml`, o
  cache `Swatinem/rust-cache` e a `cargo-deny-action`, todas fixadas por SHA.
  O dependabot também passou a cobrir o cargo.
- Novo `docs/retomar.md`: estado atual, próximo passo e como preparar o
  ambiente. Ele é atualizado a cada merge, para que o trabalho possa ser
  retomado de qualquer ponto.

### Passo 8: `core::engine` em Rust (`feat/engine-rust`)

- Dependências:
  - `bollard` 0.21, compilado **sem** os recursos HTTP/TCP, só o socket Unix
    local. É menos superfície e menos dependências;
  - `tokio`, `thiserror`, `tracing`, `sha2`/`hex`, `tar` e `rustix` (uid/gid
    sem `unsafe`).
- A API do bollard mudou bastante na 0.21 (builders em `query_parameters`,
  `ContainerCreateBody`, `body_full`). As assinaturas foram conferidas no
  código-fonte baixado, em vez de supostas.
- Módulos:
  - `context`: arquivos de `engine/` embutidos, hash, tag e tar determinístico
    (mtime 0, dono root). Um teste compara `FILES` com o conteúdo real de
    `engine/`, para que o hash nunca deixe de cobrir um arquivo novo;
  - `spec`: tradução pura para a Docker API, com as mesmas validações do
    entrypoint e testes de privilégio mínimo (rede do host, só `NET_ADMIN`,
    `privileged` desligado, `security_opt` conforme SELinux/AppArmor);
  - `docker`: `ensure_image`, `start`, `stop` e `is_running`.
- Decisões de implementação:
  - **Recriação graciosa.** O ADR-0002 fala em `rm -f` do container anterior,
    mas `rm -f` manda SIGKILL e deixaria restos no host se uma sessão
    estivesse ativa. O `start` faz `docker stop` (10 s, o entrypoint limpa) e
    depois remove. O teste de integração cobre esse caminho, chamando `start`
    duas vezes;
  - **Diretório de runtime.** É criado com 0700 e rejeitado se for symlink, se
    não for um diretório ou se pertencer a outro usuário. Isso evita que outro
    usuário local prepare o caminho do socket;
  - **Prontidão.** Linha `[engine] pronto` + `connect()` no socket, com timeout
    de 20 s. Em falha, o erro leva as últimas 40 linhas do log do entrypoint;
  - `clippy.toml` libera `unwrap`/`expect` só em testes.
- Achado: a saída do build pela API traz códigos de cor ANSI. Agora eles são
  removidos (`strip_ansi`, com teste) antes de chegar à UI.
- O build pela API usa o builder clássico, que ainda funciona no Docker 29. O
  primeiro build leva cerca de 35 s. Com a imagem em cache, o teste de
  integração completo (subir, recriar, derrubar) leva cerca de 1 s.
- Verificado depois do teste: nenhuma interface, container ou socket sobrou.

### Passo 9: cliente VICI em Rust (`feat/vici`)

- **Decisão (ADR-0011): implementação própria em vez do `rsvici`.**
  - O `rsvici` 0.1.5 é pré-1.0, tem 4 dependências e serializa as mensagens
    via serde em buffers que **não são zerados**. O `load-shared` leva a PSK e
    a senha.
  - O protocolo é pequeno e já tinha uma referência em Python no repositório.
- Implementado em três camadas:
  - `message`: formato de fio, com valores em `Zeroizing`, `Debug` sem valores
    e limite de profundidade;
  - `client`: pacotes, conexão de comandos com suporte a comandos streamed e
    conexão de eventos;
  - `charon`: trait `Charon` e tipos sem identidades.
- Testes unitários (28 no core):
  - formato de fio byte a byte;
  - ida e volta (roundtrip);
  - 7 entradas malformadas;
  - aninhamento excessivo;
  - campos grandes demais;
  - `Debug` sem segredos;
  - parse de `list-sa` e dos eventos;
  - um servidor VICI de mentira (socketpair) para o fluxo de
    `streamed_request`.
- **Teste de integração `vici_docker.rs`.** O gateway de teste sobe pela própria
  Docker API (`tests/support`). Depois, sem nenhum `swanctl`:
  1. `version` (5.8.2);
  2. `load-shared` da PSK e da senha XAuth;
  3. `load-conn` e `list-conns`;
  4. `initiate`, com o evento `child-updown` (up) recebido;
  5. `list-sas`: ESTABLISHED, VIP 198.51.100.x, child INSTALLED com o TS
     certo;
  6. ping pelo túnel, com os contadores de bytes subindo;
  7. logs sem PSK nem senha;
  8. `terminate`, com o evento de down e as rotas removidas;
  9. `unload-conn`.

  O teste leva cerca de 2 s. A limpeza roda sempre, mesmo se uma asserção
  falhar, porque o cenário roda numa task separada.
- O clippy passou a rodar com `--all-features`, para analisar também o código
  dos testes Docker.

### Passo 10: perfis (`feat/perfis`)

- **Modelo** (schema 1, serde com `deny_unknown_fields`):
  - gateway;
  - IKE: versão, agressivo, encap, propostas, rekey, DPD;
  - autenticação: método `psk` ou `psk-xauth`, `local_id`, `remote_id`;
  - child: nome, propostas ESP, rekey, TS, ações de DPD e de fechamento;
  - rede: VIP, MTU e DNS (servidores, domínios, DNS recebidos, rota padrão).
- **Decisões:**
  - O **usuário XAuth não fica no perfil.** Ele é credencial, como a senha.
    Assim o perfil do trabalho pode ser passado a um colega sem a matrícula de
    quem o criou.
  - A autenticação é um `method` enumerado, extensível para `eap`/`pubkey` num
    schema 2, sem quebrar perfis existentes. `parse_profile` lê o
    `schema_version` antes do resto, que é onde as migrações vão entrar.
  - **ADR-0012: `presets.local/` não é embutido no binário**, como previa o
    plano. Um build local publicado por engano levaria os dados do trabalho.
    A distribuição é feita por exportação e importação de JSON.
  - `close_action`: o fluxo validado usa `start`, e o perfil gera `restart`.
    No parser de configuração do charon, os dois são a mesma ação, e o teste de
    integração confirma que o charon aceita `restart`.
- **Validação** campo a campo, que devolve todos os problemas para a UI:
  - slug do id;
  - host do gateway;
  - IKEv1 obrigatório para agressivo e XAuth;
  - formato das propostas;
  - CIDRs;
  - domínios no formato do resolved;
  - MTU;
  - durações;
  - identidades sem aspas nem caracteres de controle.

  As regras de domínio e IP são as mesmas do `updown.sh`.
- **Geração:** `to_load_conn` e `updown_command`. Os argumentos vão entre aspas
  simples, e `shell_quote` recusa qualquer aspa ou controle como segunda
  barreira.
- **Store:** gravação atômica (temporário + `rename`), arquivos 0600,
  diretório 0700, importação limitada a 1 MiB e recusa a sobrescrever sem
  `overwrite`. Ids com `../` não escapam do diretório.
- **Testes** (43 no core):
  - snapshots com insta do `load-conn` dos dois presets. **Revisão manual:** o
    snapshot do preset legado reproduz exatamente as propostas, os tempos e as
    rodadas do fluxo validado;
  - todos os presets válidos e canônicos (ida e volta);
  - erros reportados em todos os campos;
  - JSON com `"psk"` recusado;
  - versão desconhecida recusada;
  - store com permissões, export/import e path traversal.
- **Integração:** o `vici_docker.rs` deixou de usar uma conexão escrita à mão e
  passou a gerar o `load-conn` a partir de `tests/fixtures/perfil-gateway-teste.json`.
  A cadeia inteira (JSON → validação → `load-conn` → charon real → túnel →
  ping) passa em cerca de 2,4 s.
- Docs: `docs/perfis.md` (formato, tabela de correspondência swanctl → perfil,
  como passar a um colega) e `presets/README.md`.

### Passo 11: segredos (`feat/segredos`)

- **Chaveiro:** o crate `keyring` 4 foi dividido em `keyring-core` + backends.
  A primeira escolha foi o backend em Rust puro (zbus), mas o `cargo-deny`
  **barrou**: o RustSec marcou `zvariant`, `zbus_names` e `zvariant_derive`
  como *unmaintained* (RUSTSEC-2026-0370/0371/0372), porque foram incorporados
  ao `zbus` 6 e o backend ainda usa o `zbus` 5. Em vez de abrir exceção no
  `deny.toml`, a troca foi para o backend **libdbus**
  (`dbus-secret-service-keyring-store`, cripto em Rust), que passa limpo. A
  libdbus existe em qualquer desktop Linux. Para compilar, é preciso o
  `dbus-devel`/`libdbus-1-dev`, que foi acrescentado ao README e à CI.
- **API do backend** conferida no código-fonte: `Store::new()`,
  `build(serviço, usuário, modificadores)` com rótulo amigável (aparece assim
  no Seahorse: "IPsec Client: <perfil> (<campo>)").
- **`Credentials`:**
  - `SecretString` em tudo, inclusive no usuário XAuth, que não é segredo mas
    é pessoal;
  - `Debug` só com presença/ausência e o usuário mascarado;
  - valores vazios contam como ausentes;
  - `shared_secrets` gera os segredos do `load-shared`: a PSK com dono = id do
    grupo e o XAuth com dono = usuário.
- **`Redactor`:** mascara o usuário XAuth e o id do grupo nas linhas de log,
  inclusive o id sem o prefixo `keyid:`, que é como o charon registra
  (`[grupo-teste]`). Identidades com menos de 3 caracteres não são mascaradas,
  para não estragar o texto.
- **Testes** (53 unitários no core):
  - `Debug` sem segredos;
  - ausências e valores vazios;
  - donos dos segredos;
  - store em memória;
  - chaves `<perfil>:<campo>`;
  - máscaras;
  - linhas reais do charon mascaradas.
- **Chaveiro real:** o teste `keyring_real_roundtrip` (opcional, `--ignored`)
  criou, leu e apagou um item no GNOME Keyring da sessão do mantenedor e
  passou.
- **Integração:** o `vici_docker.rs` passou a carregar os segredos via
  `Credentials::shared_secrets`, e confere que os logs, depois do `Redactor`,
  não trazem nem o usuário XAuth nem o id do grupo. O charon, por si só,
  registra os dois, e o teste confirma isso também.

### Passo 12a: sessão no core (`feat/sessao-cli`, primeira parte)

- **Assinaturas de falha reais.** Cada falha típica foi provocada de propósito
  contra o gateway de teste, com `swanctl`, e as linhas do charon 5.8.2 foram
  capturadas:

  | Falha provocada | Linha capturada |
  |---|---|
  | PSK errada | `calculated HASH does not match HASH payload` |
  | Senha errada | `XAuth authentication of '…' (myself) failed` |
  | Proposta IKE errada | `received NO_PROPOSAL_CHOSEN` antes do IKE_SA |
  | Proposta ESP errada | o mesmo aviso, depois do IKE_SA estabelecido |
  | Gateway inexistente | `retransmit N of request` + `not established after` |

  Essas linhas viraram os testes de `session::diagnose`.
- **`core::diagnostics`:**
  - root;
  - `XDG_RUNTIME_DIR`;
  - Docker;
  - `xfrm_interface` (por `/sys/module` ou `/lib/modules`);
  - systemd-resolved;
  - D-Bus do sistema;
  - portas 500/4500, por parse de `/proc/net/udp*`, sem root.

  O `doctor` acrescenta o chaveiro e a imagem.
- **`core::session`:**
  - `establish`, genérica sobre `Charon` e testada com um mock: carrega os
    segredos antes da conexão e, se faltar credencial, não toca no charon;
  - `connect` (preflight → imagem → engine → eventos → negociação), que
    derruba o engine em qualquer falha;
  - `status` e `disconnect`, que funcionam de qualquer processo, porque o
    container leva a label do perfil;
  - eventos de estado, log mascarado, queda e retorno do túnel.
- Um detalhe de concorrência: os logs chegam por outra conexão VICI. Antes de
  diagnosticar uma falha, a sessão espera 300 ms para os últimos chegarem.
- **Integração `session_docker.rs`:**
  - senha errada → `XauthCredentials`, sem identidade no detalhe e sem nada de
    pé depois;
  - conexão com eventos;
  - ping;
  - segunda conexão recusada (`AlreadyConnected`);
  - `status`;
  - `disconnect`;
  - idempotência.

  Leva cerca de 4 s.

### Passo 12b: CLI (`feat/sessao-cli`, segunda parte)

- **`ipsec-client-cli`** (clap, pt-BR):
  - `doctor`;
  - `profiles` (list/show/import/export/validate/delete/path);
  - `secrets` (set/status/forget);
  - `connect`;
  - `status`;
  - `logs`;
  - `disconnect`.

  Guia em `docs/cli.md`.
- **Credenciais no `connect`:**
  - primeiro o chaveiro; o que faltar é pedido no terminal, com as senhas
    lidas do TTY sem eco (`rpassword`);
  - `--remember` guarda no chaveiro;
  - `--credentials-stdin` lê linhas `chave=valor` da entrada padrão, como o
    `docker login --password-stdin`, para automação e para o E2E. Segredos por
    argumento ou variável de ambiente foram descartados de propósito: aparecem
    em `ps` e no histórico.
- **Primeiro plano:** o progresso e o log aparecem durante a negociação; depois
  de conectar, `Ctrl+C` desconecta, e quedas e retornos do túnel aparecem na
  tela. `--detach` devolve o terminal, e o túnel é um container independente.
- **Scripts:** o `status` sai com 3 quando está desconectado, e o `doctor` sai
  com 1 quando algo impede a conexão.
- **`logs`:** mascara o id do grupo e o usuário XAuth do perfil ativo. O perfil
  vem da label do container, e o usuário, do chaveiro.
- **`tests/cli/e2e.sh`** (`npm run test:cli`): o fluxo de um usuário real contra
  o gateway de teste, com um `XDG_CONFIG_HOME` temporário e credenciais falsas
  via stdin, sem tocar no chaveiro nem nos perfis do mantenedor:
  1. doctor;
  2. import, list e validate, com o perfil gravado em 0600;
  3. senha errada → "usuário ou senha XAuth incorretos", sem identidade
     exposta e com o host limpo;
  4. connect `--detach`, com a saída sem segredos;
  5. status com VIP;
  6. ping;
  7. logs sem segredos;
  8. disconnect, com status = 3 e o host limpo.

  O teste passou e entrou na CI.

### Passo 13: importar `swanctl.conf` (`feat/import-swanctl`, adiantado)

- **Por que adiantar:** é o caminho mais curto para o mantenedor gerar o perfil
  do trabalho a partir do arquivo que já funciona, sem digitar valores e sem
  que eles passem por esta sessão, e para passar a configuração ao colega.
- **Parser** do formato do strongswan.conf: seções, `chave = valor`, `#` fora de
  aspas e avisos para `include`.
- **Segurança:**
  - a seção `secrets` é **pulada sem ser armazenada**: o parser só conta as
    chaves de abertura e fechamento;
  - uma chave `secret` fora de `secrets` faz a importação ser recusada;
  - o `xauth_id` não é copiado, porque o usuário é credencial;
  - os testes conferem que nem o JSON do perfil nem o `Debug` contêm os
    segredos da fixture.
- **Mapeamento:**
  - campos da conexão, rodadas `local*`/`remote*` (psk, xauth) e o primeiro
    child;
  - ações `start`/`restart` → `restart`, `route`/`trap`/`hold` → `trap`,
    `none`/`clear` → `none`;
  - durações com sufixo s/m/h/d;
  - nomes convertidos em ids (`rede_interna` → `rede-interna`);
  - certificados e EAP são recusados com uma mensagem clara.
- **Fixture:** a mesma estrutura do `swanctl.conf` real do mantenedor (tabs,
  comentários, `#proposals`, `local-1`/`local-2`, `secrets` com aspas e `#`
  dentro), com valores falsos.
- **CLI:** `profiles import-swanctl`, com `--dns`/`--domain`/`--use-received-dns`,
  porque o DNS não existe no swanctl.conf, e `--output` para gerar um arquivo
  0600.
- **E2E do CLI:** importa o `swanctl.conf` do gateway de teste (que tem
  `secrets` com credenciais falsas), confere que o perfil não tem segredos nem
  o usuário, e **conecta com esse perfil**, com ping pelo túnel. Passou.

### Passo 14: primeiro uso real e guia rápido

- O mantenedor instalou o CLI e importou o perfil do trabalho com
  `profiles import-swanctl`, passando o DNS e o domínio do script antigo. Depois
  guardou as credenciais com `secrets set`, sem nenhum valor passar pelo
  repositório.
- Dois tropeços viraram documentação:
  - `cargo: command not found` (falta o `~/.cargo/env` no shell);
  - os marcadores `COLE_O_...` copiados literalmente.
- Novo `docs/guia-rapido.md`: passo a passo simples para um colega que
  recebeu o perfil exportado (instalar, importar, `secrets set`, `connect`,
  tabela de erros).

---

## 2026-11-16: nova direção, app desktop sem Docker

O mantenedor definiu o objetivo final: um app **bonito, simples e seguro** que
substitua o FortiClient, distribuído como **AppImage**, em que o usuário não
instala nada além do próprio app.

**Pontos levantados no planejamento**

- **npm não chega ao usuário.** Node e npm são ferramentas de build. O Tauri
  compila o frontend para dentro do binário. Colocar o frontend num container
  foi descartado: não resolve nada e traz o Docker de volta.
- **O Docker era o verdadeiro "instalar coisas"**, além de pôr o usuário no
  grupo `docker`, o que equivale a root.
- **Flatpak foi descartado.** O sandbox impede netlink/XFRM e serviços com
  privilégio, e contorná-lo anula o sandbox. Os formatos ficam AppImage + rpm
  + deb.

**Decisões do mantenedor**

- engine **embutido**: strongSwan compilado por nós e um helper com privilégio
  instalado **uma vez**, com a senha de administrador;
- o **strongSwan mais novo que funcionar**, com a 5.8.2 como reserva;
- a ordem **prova → GUI → engine → release**.

**O que muda e o que fica**

- O núcleo (`vici`, `profiles`, `secrets`, `session`, `diagnostics`) é
  reaproveitado.
- O `core::engine` ganha a trait `EngineBackend`.
- A lógica do `entrypoint.sh` vai para o helper.
- O roadmap foi reescrito com as fases A a E.

### Passo 15: prova do engine embutido (`spike/engine-nativo`)

**Objetivo:** provar que dá para tirar o Docker do usuário compilando o próprio
strongSwan, e escolher a versão.

**Build** (`native-engine/`):

- tarballs oficiais de 6.1.0, 6.0.7, 5.9.14 e 5.8.2, conferidos por **SHA-256
  fixado** e **assinatura GPG** (chave `948F…BA77`, versionada em
  `native-engine/keys/`);
- builder Ubuntu 22.04 (glibc 2.35, a mesma base do AppImage), com o tarball
  de saída determinístico.

Três tropeços, todos resolvidos:

1. **Plugins ausentes no executável estático.** O charon subia com
   `aes_plugin_create not found`. Num build estático, os plugins só entram com
   `--enable-monolithic`, que gera uma tabela de construtores. Para gerá-la, o
   strongSwan precisa do Python **no build**, que foi acrescentado ao builder.
2. **O 5.8.2 não linkava** no gcc 11. É o código antigo diante do
   `-fno-common`, padrão desde o gcc 10. Solução: `-fcommon` só para a 5.8.x.
3. **Pacote de 12 MB** por causa das bibliotecas `.a` de build. Depois de
   removê-las, ficam cerca de 1,8 MB compactados e 4 MB instalados, com o
   charon dependendo **só da glibc** (o gmp ficou estático).

**Teste:**

- `native-engine/spike/` monta uma imagem **Fedora 45** (a mesma glibc 2.44 e o
  mesmo iproute2 moderno do host do mantenedor) só como substituto do root,
  porque o modelo real de privilégio é a fase D;
- o `tests/engine/e2e.sh` ganhou `IPSEC_CLIENT_E2E_ENGINE_IMAGE` e
  `IPSEC_CLIENT_E2E_EXPECT_VERSION`. O gateway de teste continua sendo o
  strongSwan 5.8.2 do Ubuntu, imitando as VPNs legadas.

**Novo cenário 4: PFS só com modp768 na fase 2.** O `vpn-legada.conf` do
mantenedor diz que o gateway "pediu DH1". O primeiro teste falhou e revelou
uma regra do IKEv1: **a fase 2 só leva um grupo DH, o da primeira proposta**.
O strongSwan descarta as propostas com outros grupos. Como a lista do
mantenedor começa com modp1024, a VPN real negocia hoje PFS modp1024. O
cenário passou a oferecer modp768 primeiro, e agora passa.

**Resultado:** **as quatro versões passaram nos 26 testes** (4 cenários). A
6.1.0 não registrou nenhum aviso de algoritmo fraco. Bônus: com o iproute2
moderno, o `entrypoint.sh` agora **remove as políticas XFRM órfãs** (0
restantes no cenário 2). A limitação antiga só existe na imagem Docker.

**Decisões (ADRs):**

- 0013: engine embutido, sem Docker para o usuário;
- 0014: helper com privilégio, autorizado uma vez (polkit + `systemd-run` com
  sandbox);
- 0015: AppImage + rpm + deb, sem Flatpak;
- 0016: strongSwan 6.1.0, com a 5.8.2 de reserva.

**Limitação registrada:** sem OpenSSL, não há curvas ECP. O preset IKEv2 de
exemplo passou a usar `curve25519`.

**CI:** novo job `native-engine` (`npm run test:native`).

**Pendente:** o push e a primeira execução da CI, que dependem do mantenedor,
porque o ambiente automatizado não tem as credenciais do GitHub.
