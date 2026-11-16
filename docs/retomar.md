# Estado atual e como retomar

> Este arquivo é atualizado **a cada merge em `main`**. Se o trabalho for
> interrompido no meio de um branch, o último commit desse branch e a última
> entrada do [diário](desenvolvimento.md) dizem onde ele parou.

## Onde estamos

- **Versão 0.1.0 (2026-11-16): primeira versão funcional, validada pelo
  mantenedor na VPN real** (FortiGate, IKEv1 agressivo + PSK de grupo +
  XAuth), com acesso à máquina do trabalho pelo Remmina. Caminho validado, o
  mesmo de um usuário final:
  1. app instalado pelo `.rpm`, aberto pelo menu de aplicativos;
  2. componente de rede instalado pelo próprio app, com a senha de
     administrador (`pkexec`, origem `app`);
  3. conexão pelo strongSwan 6.1.0 embutido, **sem Docker**.
- **Pronto:**
  - núcleo Rust e `ipsec-client-cli`;
  - app desktop: conexão, perfis (importar/exportar), aba Sistema,
    bandeja, notificações e instância única;
  - engine embutido (helper com polkit e sandbox do systemd);
  - instalação do componente pelo app (ADR-0018);
  - pacotes `.rpm`/`.deb` que entram no menu, com o
    [manual de instalação](instalacao.md).
- **Próximo:** `chore/release`:
  - AppImage + rpm + deb gerados na CI (Ubuntu 22.04);
  - pós-instalação do rpm/deb instalando o componente (`--origin package`);
  - `.deb` com a `libayatana-appindicator3-1`;
  - `SHA256SUMS` assinado, GitHub Releases e tag `v0.1.0`.
- **Depois:** `feat/ui-editor-perfis` (editar perfis na interface).
- **Ideias anotadas:**
  - teste de longa duração (rekey em ~11 h);
  - "copiar log anonimizado";
  - unificar a consulta de estado da UI e da bandeja;
  - opção de fechar a janela para a bandeja (hoje, fechar encerra o app, mas
    a VPN continua conectada).
- **Pendente com o mantenedor:**
  1. conferir a CI no GitHub depois do push;
  2. antes de tornar o repositório público: reescrever o e-mail dos commits
     (`git filter-repo --mailbox`) e varrer o histórico
     (`scripts/check-private.sh --history` + gitleaks).

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
npm run test:packaging # instalador do componente de rede, como root num container
```

## Regras que não mudam

- Nenhum dado real de VPN no repositório (ADR-0010). O guarda roda nos hooks.
- Um branch por item do roadmap e Conventional Commits em pt-BR.
- Merge `--no-ff` com o `npm run check` verde.
- A cada passo relevante, uma entrada no diário. A cada merge, atualizar este
  arquivo, o roadmap e o CHANGELOG.

## Pendências do mantenedor

- [x] Licença definida: GPL-3.0-or-later (ADR-0017).
- [ ] Antes de tornar o repositório público, decidir se o e-mail pessoal fica
      nos commits. A alternativa é reescrever o histórico com o e-mail
      `noreply` do GitHub (`git filter-repo --mailbox`).
