# Estado atual e como retomar

> Este arquivo é atualizado **a cada merge em `main`**. Se o trabalho for
> interrompido no meio de um branch, o último commit desse branch e a última
> entrada do [diário](desenvolvimento.md) dizem onde ele parou.

## Onde estamos

- **Direção atual (2026-11-16):** um app desktop "baixe e rode" (AppImage),
  sem Docker para o usuário. O plano completo, com as fases A a E, está no
  [roadmap](roadmap.md).
- **Base pronta:** o núcleo Rust e o `ipsec-client-cli` funcionam de ponta a
  ponta. O mantenedor já importou o perfil do trabalho.
- **Fase B concluída** (`spike/engine-nativo`): o strongSwan **6.1.0** compilado
  por nós funciona (ADRs 0013–0016). O build e o E2E rodam com
  `npm run test:native`.
- **Próximo branch:** `chore/scaffold-gui` (fase C): Tauri 2.12 estável + React
  19 + Vite + Tailwind 4 + shadcn/ui.
- **Pendente com o mantenedor:** `git push origin --all` (o ambiente
  automatizado não tem as credenciais do GitHub) e conferir a primeira
  execução da CI.

## Preparar o ambiente (uma vez)

1. Instale os pré-requisitos do README, seção "Pré-requisitos".
2. Instale o cargo-deny: `cargo install --locked cargo-deny`.
3. Rode `npm install`, que instala os hooks de git.
4. Crie o arquivo `.git/info/private-patterns` com os seus padrões privados
   (ver CONTRIBUTING.md).

## Verificar que está tudo verde

```bash
npm run check          # dados privados + shellcheck + fmt/clippy/testes/cargo-deny
npm run test:engine    # E2E do engine com gateway IPsec local (~10 s)
npm run test:docker    # integração do núcleo Rust com o Docker real
npm run test:cli       # fluxo completo do CLI
npm run test:native    # strongSwan compilado (engine embutido) + E2E
```

## Regras que não mudam

- Nenhum dado real de VPN no repositório (ADR-0010). O guarda roda nos hooks.
- Um branch por item do roadmap e Conventional Commits em pt-BR.
- Merge `--no-ff` com o `npm run check` verde.
- A cada passo relevante, uma entrada no diário. A cada merge, atualizar este
  arquivo, o roadmap e o CHANGELOG.

## Pendências do mantenedor

- [ ] Definir a licença antes de tornar o repositório público.
- [ ] Antes de tornar o repositório público, decidir se o e-mail pessoal fica
      nos commits. A alternativa é reescrever o histórico com o e-mail
      `noreply` do GitHub (`git filter-repo --mailbox`).
