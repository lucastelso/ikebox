# Estado atual e como retomar

> Este arquivo é atualizado **a cada merge em `main`**. Se o trabalho for
> interrompido no meio de um branch, o último commit desse branch e a última
> entrada do [diário](desenvolvimento.md) dizem onde ele parou.

## Onde estamos

- **Último branch integrado:** `feat/sessao-cli`. O `ipsec-client-cli` conecta
  de ponta a ponta (`npm run test:cli`): doctor, profiles, secrets, connect,
  status, logs e disconnect, com diagnóstico de falhas.
- **Próximo branch:** `feat/import-swanctl`, adiantado no roadmap. Ele importa
  um `swanctl.conf` existente para um perfil sem ler os segredos, o que
  permite ao mantenedor gerar o perfil do trabalho sem digitar nada, e depois
  passá-lo ao colega.
- **E2E real (com o mantenedor):** importar o perfil, rodar
  `ipsec-client-cli secrets set <perfil>`, depois `ipsec-client-cli connect
  <perfil>`, e validar o RDP pelo Remmina. Antes, desconectar o script antigo,
  porque as portas 500/4500 ficam ocupadas.
- **Status por branch:** ver o [roadmap](roadmap.md).

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
npm run test:docker    # integração do core::engine (Rust) com o Docker real
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
