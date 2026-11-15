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
