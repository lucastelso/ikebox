# Changelog

Todas as mudanças relevantes deste projeto são registradas aqui.

O formato segue o [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/),
e o projeto adota o [Versionamento Semântico](https://semver.org/lang/pt-BR/).
A partir da v0.1.0, este arquivo será gerado com git-cliff a partir dos
commits.

## [Não lançado]

### Licença

- O projeto passa a ser **GPL-3.0-or-later** (ADR-0017). `THIRD_PARTY_NOTICES.md`
  identifica o strongSwan e o GMP distribuídos no engine, junto com o
  código-fonte correspondente. Os textos de licença deles vão no pacote do
  engine.

### Adicionado

- Documentação inicial: README, arquitetura, roadmap, diário de desenvolvimento
  e ADRs 0001–0010.
- Guarda de dados privados (`scripts/check-private.sh`), com padrões locais
  fora do git.
- Ferramentas do repositório: lefthook (pre-commit, commit-msg, pre-push),
  commitlint (Conventional Commits em pt-BR), shellcheck (local ou via imagem
  Docker fixada) e `npm run check`.
- CI no GitHub Actions (verificações + gitleaks) e dependabot.
- Engine (`engine/`): imagem Ubuntu 20.04 fixada por digest + strongSwan
  5.8.2-1ubuntu3.6, entrypoint com interface XFRM dedicada, socket VICI
  restrito ao usuário e limpeza no SIGTERM, updown com rotas `proto 241` e DNS
  split via resolvectl.
- Teste ponta a ponta do engine (`npm run test:engine`) contra um gateway
  IPsec local (IKEv1 agressivo + PSK + XAuth), também na CI. Três cenários:
  ciclo normal, recuperação após SIGKILL e túnel completo em rede isolada.
- Workspace Rust (`crates/core`, `crates/cli`) com toolchain fixada (1.100.0),
  lints, `cargo-deny` e verificações no `npm run check`, nos hooks e na CI.
- `docs/retomar.md`: estado atual e como retomar o desenvolvimento.
- `core::engine`: contexto da imagem embutido com hash e tar determinístico,
  `EngineSpec` validada (privilégio mínimo), build e ciclo de vida do
  container via Docker API (só socket Unix), seguindo o contrato de prontidão.
  Testes unitários e de integração (`npm run test:docker`).
- `core::vici`: cliente VICI próprio (ADR-0011), com mensagens zeradas no
  drop, conexões de comandos e de eventos e a trait `Charon` tipada. Um teste
  de integração estabelece um túnel IPsec completo contra o gateway de teste
  só pelo Rust.
- `core::profiles`: perfil JSON (schema 1, sem credenciais), validação campo a
  campo, geração do `load-conn` e do `updown` (aspas simples + validação),
  `if_id` por perfil e store com gravação atômica e permissões privadas. Inclui
  os presets genéricos `presets/`, o guia `docs/perfis.md` (com a
  correspondência swanctl → perfil) e o ADR-0012.
- `core::secrets`: credenciais em `SecretString`, chaveiro do sistema (Secret
  Service via libdbus), armazenamento em memória, geração dos segredos do
  `load-shared` e mascaramento de identidades em logs.
- `core::diagnostics` e `core::session`: preflight sem root, conexão com
  eventos, `status`/`disconnect` de qualquer processo e diagnóstico das causas
  de falha (PSK/grupo, XAuth, propostas IKE/ESP, redes, gateway inacessível).
- `ipsec-client-cli`: `doctor`, `profiles`
  (list/show/import/export/validate/delete/path), `secrets` (set/status/forget,
  chaveiro), `connect` (primeiro plano com Ctrl+C, `--detach`, `--remember`,
  `--credentials-stdin`), `status` (sai com 3 se desconectado), `logs`
  (mascarados) e `disconnect`. Guia em `docs/cli.md`. Teste ponta a ponta:
  `npm run test:cli`.
- `profiles import-swanctl`: gera o perfil a partir de um `swanctl.conf`
  existente, pulando a seção `secrets` sem lê-la e sem copiar o usuário XAuth.
  O DNS vem por `--dns`/`--domain`.
- `docs/guia-rapido.md`: passo a passo para quem recebe um perfil exportado.
- Engine embutido (`native-engine/`): strongSwan compilado a partir do
  código-fonte oficial (SHA-256 + GPG), estático, sem OpenSSL. O charon
  depende só da glibc, e o engine inteiro tem cerca de 4 MB. Versões testáveis:
  6.1.0 (padrão), 6.0.7, 5.9.14 e 5.8.2 (`npm run test:native`). ADRs
  0013–0016.
- E2E do engine: cenário 4, com PFS só modp768 (DH1) na fase 2.
- Scaffold do app desktop:
  - `src-tauri/`: Tauri 2.12, IPC tipado com tauri-specta (versão exata),
    DTOs próprios, CSP estrita, capabilities mínimas, sem plugins de shell/fs;
  - `src/`: React 19, TypeScript, Vite 8, Tailwind 4 com tokens claro/escuro e
    componentes shadcn/ui;
  - biome, vitest com mockIPC, logo e ícones.
- Tela de conexão do app:
  - seletor de perfil e botão central com anel de estado;
  - estado com IP virtual, gateway, redes, tempo e tráfego;
  - diálogo de credenciais que pede só o que falta, com "lembrar no chaveiro"
    e mostrar/ocultar senha;
  - erros com a causa provável, detalhes e "Informar credenciais";
  - log mascarado copiável;
  - consulta de status que adota conexões abertas pelo CLI.

  IPC: `list_profiles`, `credentials_status`, `connect`, `disconnect`,
  `status` e `forget_credentials`, com o evento `SessionUpdate` e erros
  tipados (`UiError`).
- Gerenciador de presets no app:
  - importar um perfil (.json) ou um swanctl.conf, pelo diálogo nativo ou
    arrastando o arquivo para a janela, com prévia, DNS para swanctl.conf,
    renomear/substituir e avisos da conversão;
  - exportar sem credenciais (diálogo nativo, arquivo 0600);
  - ver detalhes;
  - excluir, opcionalmente apagando as credenciais.

  Navegação Conexão | Perfis e avisos (toasts). Os caminhos de arquivo ficam
  só no Rust: a UI recebe uma prévia com token.
- Bandeja do app: estado da VPN, "Abrir", "Desconectar" e "Sair do app" (a
  VPN continua). Notificações do sistema quando a VPN conecta, cai, reconecta
  ou desconecta, inclusive em sessões abertas pelo CLI. Instância única: abrir
  o app de novo traz a janela existente.
- Componente de rede (engine embutido, sem Docker):
  - `crates/helper`, o único componente root: ativação por socket do systemd,
    identidade por `SO_PEERCRED`, autorização no polkit (sessão local ativa
    sem senha) e charon via `systemd-run` com sandbox (só `NET_ADMIN`,
    `NET_BIND_SERVICE`, `NET_RAW` e `CHOWN`; `ProtectSystem=strict`...);
  - `crates/proto`, o protocolo compartilhado: o cliente só manda números
    validados e o id do perfil, nunca caminhos, comandos ou segredos;
  - `packaging/` com as units do systemd, a policy do polkit e o instalador;
  - `scripts/test-native.sh` para a validação com root, que passou no
    computador do mantenedor (polkit, sandbox do systemd e SELinux
    enforcing).

  O `core` escolhe o engine embutido quando ele está instalado (`Backend`), e
  o `entrypoint.sh` ganhou o modo nativo (raiz somente leitura, socket por
  usuário).
- Aba Sistema no app: diagnóstico do ambiente (o mesmo do `doctor`, com dicas
  de correção), tema claro/escuro/sistema (lembrado) e versão.
- **O app instala o componente de rede** (ADR-0018), com a senha de
  administrador uma vez (`pkexec`):
  - o pacote do componente vai dentro do app (`npm run stage:component`;
    `npm run bundle` o inclui nos recursos);
  - na aba Sistema, o cartão "Componente de rede" mostra o estado e as
    versões, com Instalar/Atualizar/Reinstalar e Remover;
  - na tela principal, um aviso de primeiro uso; no erro de pré-requisito, o
    botão "Instalar componente de rede";
  - o app percebe quando traz um componente diferente do instalado e oferece
    atualizar;
  - no terminal: `ipsec-client-cli component status|install|uninstall`;
  - `tests/packaging/install-test.sh` testa o instalador como root num
    container, inclusive com pacotes adulterados, e roda na CI.

- **Manual de instalação** (`docs/instalacao.md`): rpm, deb e AppImage
  (inclusive como pô-lo no menu), primeiro uso, atualização, desinstalação,
  como gerar o pacote a partir do código e problemas comuns.
- Atalho próprio para o menu de aplicativos (`src-tauri/linux/ipsec-client.desktop`):
  categoria Rede, descrição em inglês e pt-BR e palavras-chave (a busca por
  "vpn" acha o app).

### Alterado

- Pacotes com o nome `ipsec-client` (antes, `i-psec-client`), recursos em
  `/usr/lib/ipsec-client` (sem espaço) e mantenedor "IPsec Client
  contributors".
- README reescrito com o estado atual (app desktop, engine embutido sem Docker,
  instalação), guia rápido do colega pelo app, e SECURITY.md com o modelo de
  ameaças do engine embutido.

- O MTU do perfil passou a ser aplicado à interface do engine (antes era sempre
  1400).
- `install-native.sh` recebe só um pacote (`--bundle <dir> --origin
  app|package|manual`), confere os hashes numa cópia só do root e recusa
  links simbólicos. O `scripts/test-native.sh` usa o mesmo caminho do app.
- `doctor`/preflight: sem componente de rede e sem Docker, a verificação
  "Componente de rede" falha com a dica de instalar o componente, em vez de
  mandar instalar o Docker.
- Os scripts carregam sempre o `~/.cargo/env`, para que o cargo do rustup
  (que respeita o `rust-toolchain.toml`) ganhe de um cargo do sistema.

- `entrypoint.sh`: caminhos do charon configuráveis
  (`IPSEC_CLIENT_CHARON`/`IPSEC_CLIENT_STRONGSWAN_D`) e remoção das políticas
  XFRM órfãs quando o iproute2 aceita `if_id` (o caso do engine embutido).
- Preset IKEv2 de exemplo: `curve25519` no lugar de `ecp256`, porque o engine
  embutido não tem ECP.
