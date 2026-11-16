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
- **Scaffold da GUI pronto** (`chore/scaffold-gui`): `npm run tauri dev` abre o
  app.
- **Tela de conexão pronta** (`feat/ui-conexao`). Ela já conecta de verdade
  pelo backend Docker atual; teste com `npm run tauri dev`.
- **Bandeja, notificações e instância única prontas** (`feat/tray`).
- **Aba Sistema pronta** (`feat/ui-diagnostico`): diagnóstico visual, tema e
  versão.
- **Gerenciador de presets pronto** (`feat/ui-perfis`): importar JSON ou
  swanctl.conf (botão ou arrastar o arquivo para a janela), exportar, ver
  detalhes e excluir.
- **Próximo: fase D, o engine embutido de verdade, sem Docker** (ADRs 0013 e
  0014). Ordem sugerida:
  1. `feat/helper` (`crates/helper`, roda como root):
     - socket `/run/ipsec-client-helper.sock`, com ativação pelo systemd ou
       bind próprio em modo de teste;
     - protocolo JSON por linha: `start(EngineSpec)`, `stop`, `status`,
       `logs`;
     - uid do cliente por `SO_PEERCRED` (rustix) e autorização no polkit
       (`CheckAuthorization` via libdbus, ação
       `io.github.lucastelso.ipsecclient.connect`, `allow_active=yes`);
     - `start` valida com o mesmo `EngineSpec::validate`. Depois roda o
       charon embutido com
       `systemd-run --unit=ipsec-client-engine` e hardening
       (`CapabilityBoundingSet`, `ProtectSystem=strict`...), **reaproveitando
       o `engine/entrypoint.sh` como ExecStart**, com
       `IPSEC_CLIENT_CHARON`/`STRONGSWAN_D` apontando para o engine embutido;
     - prontidão pelo socket VICI; `stop` = `systemctl stop`; `logs` =
       `journalctl -u`.
  2. `feat/backend-nativo`:
     - trait (ou enum) `EngineBackend` no `core::engine`;
     - `NativeBackend` fala com o helper; o Docker vira o backend de testes;
     - a sessão escolhe o nativo quando o helper está instalado.
  3. `feat/setup-appimage`: unit systemd (socket + service), policy polkit e
     instalador via `pkexec` no primeiro uso.

  **Os testes que exigem root vão precisar do mantenedor (`sudo`).**
- **Ainda na fase C:** `feat/ui-editor-perfis`, para editar perfis na
  interface.
- **Pendente com o mantenedor:**
  1. `git push origin --all` (o ambiente automatizado não tem as credenciais do
     GitHub) e conferir a primeira execução da CI;
  2. **testar o app com a VPN real:** `npm run tauri dev`, depois Perfis →
     Importar (ou arrastar o `trabalho.json`), Conexão → Conectar → Remmina,
     e dar retorno sobre o visual e o uso;
  3. a fase D (helper com root) vai precisar de testes com `sudo` no
     computador do mantenedor.

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

- [x] Licença definida: GPL-3.0-or-later (ADR-0017).
- [ ] Antes de tornar o repositório público, decidir se o e-mail pessoal fica
      nos commits. A alternativa é reescrever o histórico com o e-mail
      `noreply` do GitHub (`git filter-repo --mailbox`).
