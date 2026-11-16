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

### Passo 16: scaffold da GUI (`chore/scaffold-gui`)

**Versões**, todas fixadas exatamente:

- Tauri **2.12** estável (o crate `tauri` já tem 3.0-alpha, que foi evitado de
  propósito);
- `tauri-specta` 2.0.0-rc.29, com `specta` rc.28 e `specta-typescript` 0.0.15;
- React 19.3, Vite 8, Tailwind 4.3, TypeScript 7, biome 2.6, vitest 5.

O npm 11 bloqueia os scripts de instalação das dependências. Nenhuma das
dependências novas precisa deles; só o lefthook tem um, e ele continua
bloqueado.

**`src-tauri/`:**

- comandos finos sobre o `core` (ADR-0009);
- **DTOs próprios** (`dto.rs`): o `core` não depende de specta, e só o que a UI
  precisa atravessa para o webview;
- um teste Rust (`bindings_are_up_to_date`) compara o `src/bindings.ts`
  versionado com o gerado. Para regenerar: `UPDATE_BINDINGS=1 cargo test -p
  ipsec-client-app bindings`.

**Segurança:**

- CSP estrita: `script-src 'self'`, sem conteúdo remoto. O `style-src`
  aceita inline por causa dos estilos de posicionamento dos componentes
  Radix;
- `freezePrototype`;
- capability só com `core:default`;
- nenhum plugin de shell ou fs.

**`src/`:**

- tokens de cor do shadcn/ui em OKLCH, com tema claro e escuro seguindo o
  sistema ou forçado por `data-theme`;
- componentes `Button` e `Card`;
- tela provisória que já usa o IPC tipado;
- testes com `mockIPC`, imports explícitos do vitest e limpeza de DOM e mocks
  entre testes.

**Identidade provisória:** `assets/logo.svg` (escudo com fechadura nas cores do
tema), de onde o `tauri icon` gera os ícones.

**cargo-deny:** a árvore do Tauri trouxe dois avisos *unmaintained*, `paste`
(via specta) e `proc-macro-error` (via glib-macros, da pilha GTK3 do Tauri 2).
Os dois são **macros de compilação**, sem código no binário, e sem
alternativa. Ficaram como exceções justificadas no `deny.toml`. Diferente do
caso do zbus (passo 11), em que havia alternativa.

**CI e hooks:**

- o job Rust ganhou as bibliotecas do Tauri;
- o job de verificações passou a rodar biome, tsc, vitest e build;
- o pre-commit roda o biome nos arquivos web staged;
- o `npm run check` cobre tudo.

### Passo 17: tela de conexão (`feat/ui-conexao`)

**IPC (`src-tauri`):**

- comandos `list_profiles`, `credentials_status`, `connect`, `disconnect`,
  `status` e `forget_credentials`, mais o evento tipado `SessionUpdate`;
- erros no modo `Result` do tauri-specta: o TypeScript recebe
  `{status: "error", error: UiError}`, com tipo, mensagem e detalhes, em vez
  de exceções sem tipo;
- `disable_serde_phases()` deixa um tipo TypeScript por DTO. As versões
  `_Serialize`/`_Deserialize` duplicadas eram idênticas nos nossos tipos.

**Decisões de segurança e robustez:**

- a UI nunca recebe segredos: `credentials_status` devolve só o que está
  guardado e o usuário mascarado. `CredentialsInput` só entra e tem `Debug`
  redigido;
- os comandos são `async` (comandos síncronos do Tauri rodam na thread da UI),
  e disco e chaveiro vão para `spawn_blocking`;
- o `ConnectionLock` recusa um segundo `connect` em andamento, por exemplo
  num clique duplo;
- o `CredentialsDialog` apaga os valores digitados ao fechar e depois do
  envio.

**Ajustes de tipos:**

- o specta mapeia `f64` para `number | null` (NaN vira `null` em JSON), e a UI
  trata isso;
- `Option<String>` virava campo obrigatório. Agora usa `#[specta(optional)]`,
  porque "vazio" quer dizer "usar o guardado".

**UI:**

- seletor de perfil (lembrado entre execuções, com localStorage protegido por
  try/catch);
- botão central com anel de estado (cinza, âmbar pulsando, verde);
- cartão com IP virtual, gateway, redes, tempo e tráfego;
- o diálogo de credenciais pede **só o que falta** e marca "lembrar" quando há
  chaveiro;
- os erros mostram a causa provável, detalhes recolhíveis e "Informar
  credenciais";
- painel de log mascarado e copiável;
- o status consultado a cada 2 s também **adota conexões abertas pelo CLI**.

**Testes:** 12 no frontend, contando os do `connectionReducer` e da
`HomeScreen` com `mockIPC`:

- conectar com credenciais guardadas;
- pedir só o que falta, com "lembrar";
- erro de senha com a ação de correção;
- tela vazia.

O jsdom precisou de um stub de `ResizeObserver` por causa do Radix. O Rust
ganhou testes dos DTOs e do `apply_input`.

### Passo 18: gerenciador de presets (`feat/ui-perfis`)

Importar e exportar presets foi um pedido explícito do mantenedor: o
`trabalho.json` e o swanctl.conf.

**Invariante de segurança: o webview nunca manipula caminhos de arquivo.**

- O **diálogo nativo** (`tauri-plugin-dialog`, chamado do Rust; sem permissão
  de diálogo para o JS na capability) e o **arrastar-e-soltar**
  (`WindowEvent::DragDrop`, tratado no Rust) produzem uma `ImportPreview` com
  um **token opaco de uso único**.
- A UI confirma com `confirm_import(token, opções)`. Um token já usado ou
  inexistente é recusado.
- A prévia lê no máximo 1 MiB e detecta o tipo pela extensão ou pelo
  conteúdo (JSON ou swanctl.conf).

**Dependência:** o plugin de diálogo usa por padrão o seletor do GTK3, que o
Tauri já carrega. O backend de portal XDG puxaria o zbus 5, barrado pelo
cargo-deny no passo 11. O `cargo tree` confirma que não há zbus.

**Comandos:**

- `choose_import_file`;
- `confirm_import` (id/nome, DNS para swanctl.conf, `overwrite`);
- `profile_details`;
- `export_profile` (diálogo de salvar, arquivo 0600, sem credenciais);
- `delete_profile` (opcionalmente apagando as credenciais).

**UI:**

- navegação Conexão | Perfis;
- tela de perfis com cartões que expandem os detalhes (protocolo, autenticação,
  id do grupo, redes, propostas, DNS);
- diálogo de importação com prévia, DNS para swanctl.conf, aviso de conflito
  de id (exige "substituir" ou outro id) e avisos da conversão recolhíveis;
- confirmação de exclusão;
- toasts (sonner). Ao exportar, o toast lembra que a PSK vai por outro canal;
- a tela vazia da Conexão ganhou o botão "Importar perfil".

**Testes:** 16 no frontend:

- importação de swanctl.conf, com os argumentos exatos que chegam ao Rust;
- conflito de id;
- arquivo inválido;
- listar, exportar e excluir.

O Rust testa a detecção de tipo, o token de uso único e a validação do DNS.

**Fica para depois:** o editor de perfis (criar, editar, duplicar), que vira
`feat/ui-editor-perfis` no roadmap.

### Passo 19: o app abre de verdade e a aba Sistema (`feat/ui-diagnostico`)

- **Teste de fumaça do app real.** Um `tauri build --debug --no-bundle` (com o
  frontend embutido) e a execução por alguns segundos, repetida algumas vezes:
  o app abre e roda estável na sessão Wayland do mantenedor, sem erros de
  capability, de plugin ou de CSP. Esse tipo de problema escapa dos testes
  com mock.
- **Aba Sistema:**
  - comando `run_diagnostics`, que reaproveita `core::diagnostics::doctor`, e a
    lista com ícone por nível e a dica de correção;
  - tema sistema/claro/escuro em `data-theme` no `<html>`, aplicado antes da
    primeira renderização e lembrado (localStorage protegido por try/catch);
  - versão do app e do engine.
- **Acessibilidade:** o biome recusou `role="radio"` em botão, e o seletor de
  tema passou a usar `<input type="radio">` de verdade, visualmente oculto,
  com foco visível no rótulo.
- 18 testes no frontend.

### Passo 20: primeira conexão real pelo app

O mantenedor rodou o app (`npm run tauri dev`, ainda com o engine em Docker)
contra a VPN do trabalho, um FortiGate com IKEv1 agressivo e PSK de grupo +
XAuth. Resultado, lido no painel de log do app:

- **fase 1:** estabelecida. O gateway escolheu a **segunda** proposta da lista
  (AES-128/SHA1/modp1024);
- **XAuth:** autenticado. O usuário e o id do grupo aparecem **mascarados** no
  log, o que confirma o `Redactor` em uso real (um id curto vira `***`);
- **IP virtual:** recebido e instalado na interface dedicada;
- **fase 2:** AES-256/SHA1 com **PFS modp1024**. Isso confirma o achado do
  passo 15: no IKEv1, só o grupo da primeira proposta é oferecido, então o
  modp768 nunca chega a ser negociado;
- a rota e o DNS split foram para a interface dedicada;
- **desconexão:** DELETE da fase 2 e da fase 1 enviados ao gateway, rota
  removida e DNS revertido.

O gateway também mandou o aviso `RESPONDER-LIFETIME` (notify 24576), que é
informativo.

Pendências e ideias que saíram daqui:

- um teste de longa duração (horas, com RDP), passando por rekey e DPD;
- o log copiado ainda mostra gateway, DNS e domínios internos. Ideia
  registrada no roadmap: um "copiar log anonimizado" para relatórios de bug.

### Passo 21: licença (GPL-3.0-or-later, ADR-0017)

- **Fatos levantados:**
  - o strongSwan é GPL-2.0-or-later, com exceção para bibliotecas LGPL e
    OpenSSL;
  - o GMP, estático no charon, é LGPLv3/GPLv2;
  - todas as dependências Rust e do frontend que vão no app são permissivas e
    compatíveis com a GPLv3;
  - o app conversa com o charon por socket, então não é obra combinada com o
    strongSwan.
- **Escolha:** GPL-3.0-or-later. Para uma ferramenta de segurança que
  substitui um cliente proprietário, garante que derivados redistribuídos
  continuem abertos e auditáveis, e é da mesma família do strongSwan embutido.
  A alternativa permissiva (MIT/Apache-2.0) ficou registrada no ADR.
- **Aplicação:**
  - `LICENSE` com o texto oficial;
  - SPDX nos `Cargo.toml`, no `package.json` e nos metadados dos pacotes do
    Tauri;
  - `deny.toml` com exceções só para os nossos crates. As dependências
    continuam na lista permissiva;
  - contribuições *inbound = outbound* (CONTRIBUTING.md).
- **Conformidade do que é distribuído:**
  - `THIRD_PARTY_NOTICES.md` aponta o código-fonte exato do strongSwan
    (tarball + SHA-256 + GPG) e do GMP (pacote do Ubuntu 22.04, com a versão
    no `BUILDINFO`);
  - o build do engine passou a empacotar os textos de licença dos dois em
    `share/licenses/`.

### Passo 22: bandeja, notificações e instância única (`feat/tray`)

- **Problema:** os plugins oficiais de notificação e de instância única usam o
  **zbus** no Linux (o primeiro via `notify-rust`), e não há opção de troca. O
  cargo-deny barra o zbus 5 (passo 11).
- **Solução sem dependência nova:**
  - **notificações:** chamada direta a `org.freedesktop.Notifications.Notify`
    pela libdbus (crate `dbus`, que já vinha pelo chaveiro). É *best effort*:
    sem servidor de notificações, não faz nada;
  - **instância única:** socket Unix `app.sock` em
    `$XDG_RUNTIME_DIR/ipsec-client/`. A segunda execução manda `show` e sai, e
    a primeira traz a janela para a frente. Um socket velho, de uma instância
    morta, é substituído;
  - **bandeja:** feature `tray-icon` do próprio Tauri (libappindicator,
    carregada em tempo de execução). A criação fica protegida contra pânico,
    então sem a biblioteca ou no GNOME sem a extensão AppIndicator o app segue
    sem bandeja.
- **Monitor de estado** (Rust, a cada 3 s): atualiza a dica, o status e o
  "Desconectar" da bandeja, e notifica as **transições** (conectou, caiu e
  reconectando, voltou, desconectou). A função `transition_notice` é pura e
  testada, e a primeira leitura só sincroniza, sem notificar. Como lê o estado
  do engine, funciona também para sessões abertas pelo CLI.
- **"Sair do app"** não derruba a VPN, que é um processo independente (como no
  CLI). A janela fechada pelo botão também encerra o app, sem esconder na
  bandeja, porque no GNOME sem extensão a janela ficaria inacessível.
- **Teste de fumaça:** com a instância A aberta, a B saiu em cerca de 300 ms com
  código 0, e A seguiu aberta.
- **A melhorar depois:** a UI e o monitor consultam o status separadamente (a
  cada 2 s e a cada 3 s). Dá para unificar, com o Rust emitindo o status como
  evento.

### Passo 23: segunda conexão real, com DPD

- A sessão real repetida mostrou **DPD funcionando**: o charon mandou
  `DPD request` e o FortiGate respondeu `DPD_ACK`. Os keep alives de NAT-T
  também estão saindo. A bandeja foi confirmada pelo mantenedor.

### Passo 24: helper com privilégio e backend nativo (`feat/helper`, fase D)

**Desenho, com a segurança em primeiro lugar:**

- **`crates/proto`** (só serde): protocolo JSON por linha e as regras de
  validação, as mesmas do `entrypoint.sh`, **compartilhadas** pelo app e pelo
  helper. `deny_unknown_fields`: um campo a mais, como um caminho ou comando
  escondido, é recusado (há teste).
- **`crates/helper`:** o único binário root, com dependências mínimas (serde,
  rustix e dbus).
  - Identidade por `SO_PEERCRED`, preenchido pelo kernel.
  - Autorização no **polkit** (`CheckAuthorization` com
    `unix-process{pid, start-time, uid}`, sem interação; a policy usa
    `allow_active=yes`).
  - O modo de teste troca o polkit por uma lista de uids, e só o root inicia o
    helper assim.
  - **O helper não executa nada que o cliente mande:** o cliente só envia
    `if_id`, MTU, nível de log, o nome da interface (validado) e o id do perfil.
    Caminhos, comando e sandbox são fixos no helper.
  - O engine sobe com `systemd-run --unit=ipsec-client-engine`, com sandbox
    (montagem testada como função pura) e o **`entrypoint.sh` reaproveitado**
    como ExecStart.
  - Prontidão pelo socket VICI. `stop` via `systemctl stop`, que é gracioso:
    o entrypoint limpa o host. `logs` via `journalctl`.
- **`unsafe`:** o workspace proíbe. O helper usa `deny` em vez de `forbid` para
  **uma** exceção comentada: adotar o descritor 3 herdado da ativação por
  socket do systemd.
- **`entrypoint.sh` no modo nativo:**
  - `ProtectSystem=strict` deixa a raiz somente leitura. A config de runtime
    vai para o `RuntimeDirectory` e entra pelo `STRONGSWAN_CONF`, incluindo a
    base;
  - o socket fica por usuário (`/run/ipsec-client/<uid>`);
  - o build do engine passou a usar `--with-piddir=/run/ipsec-client-engine`;
  - o modo Docker continua igual (o E2E passou).
- **`core`:**
  - `NativeEngine` (cliente do helper, testado com um helper de mentira);
  - `Backend` (nativo quando instalado, Docker como reserva, forçável por
    `IPSEC_CLIENT_BACKEND`). A sessão, o diagnóstico e o `logs` do CLI passaram
    a usá-lo;
  - o `updown` tem caminhos diferentes em cada engine (`ConnParams.updown`).
- **Bug encontrado no caminho:** o MTU do perfil era ignorado, e o engine
  sempre usava 1400. Agora ele vem do perfil.
- **Empacotamento (`packaging/`):**
  - units do helper, com hardening próprio e `RuntimeDirectoryPreserve` para
    não derrubar sessões quando o helper reinicia;
  - policy do polkit;
  - `install-native.sh`, que recusa um tarball com caminhos fora de
    `opt/ipsec-client/engine`, instala em `/opt/ipsec-client` e aplica o
    `restorecon`, porque o host do mantenedor tem SELinux enforcing.
- **Verificação sem root:**
  - testes unitários do proto, do helper (montagem do `systemd-run`, parse do
    `systemctl show` e do `/proc/<pid>/stat`, autorizador) e do cliente;
  - o helper se recusa a rodar sem root;
  - `test:docker` e `test:cli` verdes, provando que o caminho Docker não
    mudou.
- **Pendente (precisa de root):** `scripts/test-native.sh`. Ele compila,
  instala com `sudo` e roda o E2E do CLI com `IPSEC_CLIENT_BACKEND=native`,
  validando de verdade o polkit, o sandbox do systemd e o SELinux.

### Passo 25: correção do instalador

- A primeira execução de `scripts/test-native.sh` pelo mantenedor falhou com
  `argumento desconhecido: -d`. A função do script se chamava `install` e
  **escondia o comando `install`** do sistema: `install -d` chamava a própria
  função. As funções passaram a se chamar `do_install`/`do_uninstall`.
- O instalador passou a ser testado como root num container Fedora, com
  `systemctl` substituído por um stub. O teste cobre a extração do engine, a
  cópia dos scripts e do helper, as permissões, as units, a policy, as
  chamadas ao systemctl e a desinstalação.

### Passo 26: segunda rodada do teste com root e a corrida no socket

- **Progresso confirmado no computador do mantenedor:** o componente de rede
  instalou (unit, socket e SELinux), o **polkit autorizou** a sessão local e o
  **engine embutido subiu sob o systemd**, com sandbox.
- **Falha:** o app levou `Permission denied` ao conectar no socket VICI. Era
  uma **corrida no helper**: ele considerava o engine pronto assim que o
  *próprio root* conectava no socket. Isso acontece antes de o
  `entrypoint.sh` fazer o `chgrp` + `chmod 0660` que entrega o socket ao grupo
  do usuário. No modo Docker não acontecia, porque lá a prontidão é a linha
  `[engine] pronto`, escrita depois do `chgrp`.
- **Correção:** o helper só responde `Started` quando o socket já está como
  `root:<gid do usuário>` 0660 **e** aceita conexão. Há um teste unitário com
  um socket real: 0770 não está pronto, 0660 no grupo certo está, outro grupo
  não está.

### Passo 27: engine embutido validado com root

O `scripts/test-native.sh` passou inteiro no computador do mantenedor
(Fedora 45, SELinux enforcing), **sem Docker no caminho da VPN**: o Docker só
roda o gateway de teste. O que ficou provado:

- **instalação:** `install-native.sh` (units, policy do polkit, `restorecon`);
- **polkit:** a sessão local ativa foi autorizada sem senha (`allow_active`);
- **sandbox do systemd:** o strongSwan 6.1.0 subiu como unidade transitória
  restrita (raiz somente leitura, só capabilities de rede) e a configuração de
  runtime entrou pelo `STRONGSWAN_CONF`;
- **fluxo completo do CLI pelo engine embutido:** senha errada com a mensagem
  clara e o host limpo; conexão; status com VIP; ping pelo túnel; logs sem
  segredos (via `journalctl` no helper); desconexão com o host limpo.

Para isso foram necessárias duas correções (passos 25 e 26): o nome da função
do instalador e a prontidão do socket.

### Passo 28: o app instala o componente de rede (`feat/setup-appimage`)

**Objetivo:** quem baixa o AppImage não instala nada à mão. Na primeira vez,
o app oferece "Instalar componente de rede", o sistema pede a senha de
administrador **uma vez**, e depois conectar não pede mais senha. Decisão
registrada no **ADR-0018**.

**Como funciona:**

1. `scripts/stage-network-component.sh` monta o **pacote do componente** em
   `target/network-component/`: helper, tarball do strongSwan, scripts do
   engine, units, policy, o próprio instalador, `VERSION` e `SHA256SUMS`. São
   2,4 MB.
2. `npm run bundle`, com `src-tauri/tauri.release.conf.json`, roda esse script
   antes do build e coloca o diretório entre os recursos do app, em
   `<recursos>/network`. Os builds de desenvolvimento não trazem o pacote;
   para testar, use `IPSEC_CLIENT_COMPONENT_BUNDLE`, só em debug.
3. Ao instalar, `core::component`:
   - copia o pacote para um diretório 0700 em `$XDG_RUNTIME_DIR`, porque o
     root não lê a montagem FUSE do AppImage;
   - confere os hashes na cópia;
   - roda `pkexec <cópia>/install-native.sh install --bundle <cópia> --origin
     app`, com o ambiente limpo.
4. O instalador, como root:
   - copia o pacote para um diretório **só dele**;
   - recusa links e arquivos especiais;
   - confere o `SHA256SUMS` e exige a lista completa;
   - recusa tarball fora de `opt/ipsec-client/engine`;
   - instala e registra `VERSION`, `SHA256SUMS` e `ORIGIN`.
5. O app compara o `SHA256SUMS` instalado com o do pacote que traz. Se forem
   diferentes, o estado é **desatualizado** e aparece o botão "Atualizar". A
   remoção usa o instalador que ficou em `/opt/ipsec-client/libexec`, só
   depois de conferir que ele e os diretórios acima são do root e que ninguém
   mais pode alterá-los.

**Na interface:**

- **Sistema → Componente de rede** mostra o estado, a versão instalada e a do
  app, com Instalar/Atualizar/Reinstalar e Remover (com confirmação);
- na **tela principal**, um aviso de primeiro uso aparece quando falta algo;
- no **erro de pré-requisito**, o botão "Instalar componente de rede". Para
  isso, a UI recebe o tipo `componentMissing`.

Com uma conexão ativa, instalar e remover são recusados, porque derrubariam o
túnel. Cancelar o pedido de senha mostra só um aviso neutro.

**No terminal:** `ipsec-client-cli component status [--bundle]`, `component
install --bundle <dir>` e `component uninstall`.

**Diagnóstico:** sem componente e sem Docker, a verificação "Componente de
rede" agora **falha**, com a dica de instalar o componente. Antes, o usuário
do AppImage via a dica de instalar o Docker, que é justamente o que o app quer
evitar.

**Testes:**

- `tests/packaging/install-test.sh` (`npm run test:packaging`, também na CI):
  - monta um pacote pelo script de produção, com engine e helper falsos;
  - roda o instalador como root num Ubuntu descartável, com `systemctl` de
    mentira;
  - cenários: instalar, reinstalar com outra origem, remover, e recusar
    arquivo adulterado, link simbólico, pacote incompleto, tarball fora do
    engine, origem inválida e execução sem root.
- **Rust (core):**
  - parse do `SHA256SUMS`, recusando caminhos absolutos, `..`, repetidos e
    lista incompleta;
  - integridade do pacote, links simbólicos e cópia privada com os modos
    certos;
  - registro da instalação e estado comparado com o pacote (com um helper de
    mentira);
  - resultado do `pkexec`: 126 = cancelado, 127 = não autorizado, falha do
    instalador com as últimas linhas;
  - ambiente limpo e exigência de dono root para remover;
  - consistência da lista de arquivos com o `install-native.sh`.
- **Frontend:** cartão (instalar, remover com confirmação, cancelamento,
  desatualizado, instalado por pacote) e tela principal (aviso de primeiro uso
  e botão no erro).

**Achados no caminho:**

- O `tempfile` cria diretórios com 0755. O modo 0700 passou a ser definido na
  criação, e um teste cobre isso.
- O cargo do sistema (instalado pelo dnf) ganhava do rustup no PATH e ignorava
  o `rust-toolchain.toml`. Os scripts passaram a carregar sempre o
  `~/.cargo/env`.
- O primeiro build de pacote (`npm run bundle -- --bundles deb`) revelou um
  erro antigo do scaffold: `"category": "Network"` não é uma categoria válida
  do Tauri, que só aceita a lista do macOS. Trocado por `Utility`.
- O `.deb` gerado traz o pacote em `/usr/lib/IPsec Client/network/`, com as
  permissões preservadas, e o `SHA256SUMS` confere lá dentro. É o caminho que
  o app procura, pelo `resource_dir()` do Tauri. No AppImage, o caminho é
  `$APPDIR/usr/lib/IPsec Client/network`.

### Passo 29: instalar como um app de verdade (`chore/instalacao-desktop`)

**Pedido do mantenedor:** instalar "bonitinho", como um usuário faria, com o
app aparecendo no menu, e um manual curto no repositório.

**O que o primeiro `.rpm`/`.deb` mostrou:**

- **Nome do pacote `i-psec-client`.** O Tauri deriva o nome do pacote do
  `productName` ("IPsec Client" em kebab-case), sem opção para mudar. O mesmo
  nome entrava no caminho dos recursos (`/usr/lib/IPsec Client`, com espaço).
- **Mantenedor "github",** tirado do identificador
  (`io.github.lucastelso...`).
- **Atalho no menu** gerado a partir do nome do produto, na categoria
  `Utility`.

**Correções:**

- `productName` = `ipsec-client`. Agora o pacote se chama `ipsec-client` (`dnf
  remove ipsec-client`), os arquivos são `ipsec-client-0.0.0-1.x86_64.rpm` e
  `ipsec-client_0.0.0_amd64.deb`, e os recursos ficam em
  `/usr/lib/ipsec-client/network`. O nome exibido continua "IPsec Client": o
  título da janela e os textos da interface não dependem do `productName`.
- `publisher` = "IPsec Client contributors" (o mantenedor do `.deb`).
- Atalho próprio, `src-tauri/linux/ipsec-client.desktop`, usado pelo rpm e
  pelo deb:
  - `Name=IPsec Client`, categoria `Network`;
  - nome genérico e descrição em inglês e pt-BR;
  - palavras-chave (VPN, IPsec, IKE, XAuth), para a busca do menu achar o app
    por "vpn";
  - `StartupWMClass=ipsec-client`.

  Passa no `desktop-file-validate`.
- Conferido no código do Tauri: sem `enableGTKAppId`, o GTK usa o nome do
  programa (`ipsec-client`) como app id. Assim o GNOME liga a janela ao
  atalho, e a dock mostra o ícone certo.

**Verificado no `.rpm` gerado:**

- nome, licença e arquivos (`/usr/bin/ipsec-client`,
  `/usr/lib/ipsec-client/network/...`, ícones e o atalho);
- as dependências (`libwebkit2gtk-4.1`, `libgtk-3` e `libappindicator3`) já
  estão instaladas no Fedora do mantenedor;
- o `SHA256SUMS` do componente confere dentro do pacote.

**Documentação:**

- novo [manual de instalação](instalacao.md):
  - qual arquivo baixar e como instalar (rpm, deb e AppImage, inclusive como
    pôr o AppImage no menu);
  - primeiro uso, atualização e desinstalação;
  - como gerar o pacote a partir do código;
  - problemas comuns;
- o [guia rápido](guia-rapido.md) do colega passou a usar o app, sem Docker,
  Rust nem terminal;
- o README foi reescrito com o estado atual: app desktop, engine embutido sem
  Docker, instalação, novo diagrama e requisitos sem Docker;
- o SECURITY.md ganhou o modelo de ameaças do engine embutido (componente
  root, sandbox, instalação, AppImage). O Docker ficou como caminho só de
  desenvolvimento.

**Ponto de atenção registrado** (para o `chore/release`): o `.deb` gerado no
Fedora depende de `libappindicator3-1`, que não existe nos Debian/Ubuntu
recentes. O `.deb` de release deve ser gerado no Ubuntu, onde o Tauri escolhe
a `libayatana-appindicator3-1`.

**Achado de comportamento** (documentado no manual): fechar a janela encerra
o app, mas não desconecta a VPN, porque o engine é um serviço do sistema. Ao
reabrir, o app mostra a conexão ativa.

### Passo 30: validação na VPN real e versão 0.1.0

**Validação do mantenedor (2026-11-16), pelo caminho de um usuário final:**

1. instalou o `ipsec-client-0.0.0-1.x86_64.rpm` com `dnf install` e abriu o
   **IPsec Client** pelo menu de aplicativos;
2. o app mostrou **"Atualização do componente de rede"**, porque a instalação
   feita pelo `test-native.sh` não tinha registro de versão. Ele clicou em
   **Atualizar agora** e digitou a senha de administrador. É o `pkexec` real;
3. conectou na **VPN do trabalho** (FortiGate, IKEv1 agressivo + PSK de grupo
   + XAuth) pelo strongSwan 6.1.0 embutido, **sem Docker**, e acessou a
   máquina do trabalho pelo Remmina.

**Conferido no sistema depois do teste** (só metadados, sem logs da VPN):

- `rpm -q ipsec-client` = `ipsec-client-0.0.0-1.x86_64`;
- `/opt/ipsec-client/ORIGIN` = `app`;
- o `SHA256SUMS` instalado é idêntico ao do pacote do app.

Com isso ficam validados de ponta a ponta o engine embutido na VPN real (que
faltava desde o passo 27), a instalação e a atualização do componente pelo
app (passo 28) e a instalação como app (passo 29).

**Versão 0.1.0:** a versão sai de 0.0.0 para **0.1.0** no workspace Rust
(todos os crates), no `tauri.conf.json` e no `package.json`. É a primeira
versão funcional. O CHANGELOG ganhou a seção `[0.1.0] - a publicar`. A tag
`v0.1.0` e os pacotes no GitHub Releases ficam para o `chore/release`, como
no plano. Assim, a tag aponta para o commit que a CI empacota.

Efeito esperado ao instalar o `.rpm` 0.1.0 por cima do 0.0.0: o app vai
mostrar "Atualização do componente de rede", porque o `VERSION` do pacote
muda. É o fluxo de atualização funcionando.
