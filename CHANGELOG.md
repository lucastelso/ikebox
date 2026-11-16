# Changelog

Todas as mudanças relevantes deste projeto são registradas aqui.

O formato segue o [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/),
e o projeto adota o [Versionamento Semântico](https://semver.org/lang/pt-BR/).
A partir da v0.1.0, este arquivo será gerado com git-cliff a partir dos
commits.

## [Não lançado]

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
