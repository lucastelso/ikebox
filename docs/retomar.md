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
- **Release v0.1.0** (`chore/release-pacotes`, passo 31): AppImage, deb e rpm
  gerados por `npm run release:build -- v0.1.0` num container Ubuntu 22.04,
  conferidos e prontos em `target/release-artifacts/0.1.0/`. A publicação é
  manual, pelo [guia de release](release.md).
- **Auditoria de segurança e privacidade (passo 32, rumo à v0.1.1):**
  - varredura de vazamento limpa: nenhum dado do trabalho no código, no
    histórico nem nos pacotes da v0.1.0;
  - corrigida uma **escalada local para root** pelo VICI (ADR-0019,
    `fix/engine-updown-forcado`). Quem tem a v0.1.0 precisa atualizar o app
    e clicar em "Atualizar" no componente de rede;
  - log anonimizado para copiar e para relatório de bug
    (`feat/log-anonimizado`, passo 33);
  - varredura de dados privados dentro dos pacotes, no fim de todo build de
    release (`chore/higiene-privacidade`, passo 34);
  - falta a release v0.1.1.
  - Enquanto o componente instalado neste computador for o da v0.1.0, rode
    `test:docker` e `test:cli` com `IPSEC_CLIENT_BACKEND=docker`. O helper
    antigo recusa o pedido novo, e é para recusar mesmo.
- **Depois:** `chore/release`, para automatizar na CI:
  - gerar e anexar os pacotes à release ao criar a tag (o mesmo
    `scripts/build-release.sh`);
  - pós-instalação do rpm/deb instalando o componente (`--origin package`);
  - `SHA256SUMS` assinado;
  - aceite numa VM limpa.
- **Em seguida:** `feat/ui-editor-perfis` (editar perfis na interface).
- **Ideias anotadas:**
  - teste de longa duração (rekey em ~11 h);
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
npm run test:native    # strongSwan compilado (engine embutido) + E2E (inclui o ataque do ADR-0019)
npm run test:systemd   # engine embutido com systemd de verdade (container privilegiado)
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
