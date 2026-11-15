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
