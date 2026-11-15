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
