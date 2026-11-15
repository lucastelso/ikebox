# Estado atual e como retomar

> Este arquivo é atualizado **a cada merge em `main`**. Se o trabalho for
> interrompido no meio de um branch, o último commit desse branch e a última
> entrada do [diário](desenvolvimento.md) dizem onde ele parou.

## Onde estamos

- **Último branch integrado:** `feat/vici`. O cliente VICI próprio (ADR-0011)
  já estabelece um túnel completo contra o gateway de teste, com
  `npm run test:docker`.
- **Próximo branch:** `feat/perfis`:
  - modelo de perfil em JSON com `schema_version`;
  - validação;
  - geração da mensagem `load-conn` e dos argumentos do `updown` (com aspas
    simples e validados);
  - presets genéricos em `presets/` e `presets.local/` (ignorado pelo git);
  - import/export;
  - snapshots com insta.

  O teste `crates/core/tests/vici_docker.rs` (`test_conn`) mostra a mensagem
  `load-conn` que o perfil precisa gerar.
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
