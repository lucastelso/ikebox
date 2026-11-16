# Roadmap

Cada linha é um branch, criado a partir de `main` e integrado com
`merge --no-ff`, com o `npm run check` verde. Os branches ficam guardados como
histórico.

## Ordem atual

A ordem original previa o scaffold do Tauri logo no início. Ela foi
**reordenada**, porque o Rust e as bibliotecas de sistema do Tauri ainda não
estavam instalados (ver o [diário](desenvolvimento.md), 2026-11-15). A ordem
nova traz primeiro o que é testável sem GUI:

1. o **engine** (container), que é a parte de maior risco técnico;
2. o **núcleo em Rust + CLI**, que já substitui o script manual;
3. a **GUI**.

| # | Branch | Entrega | Critério de aceite | Status |
|---|---|---|---|---|
| 0 | `main` | README, ADRs 0001–0010, roadmap, diário, guarda de dados privados | commit inicial | ✅ |
| 1 | `chore/tooling` | `package.json` raiz, lefthook (pre-commit/commit-msg/pre-push), commitlint, shellcheck, `npm run check`, CI inicial | hooks ativos; `npm run check` verde | ✅ |
| 2 | `feat/engine-imagem` | `engine/`: Dockerfile fixado, entrypoint, updown, config do strongSwan; teste E2E com gateway IPsec local | `npm run test:engine` verde: túnel sobe, `ipsc0` com MTU 1400, VIP, rotas, DNS, ping pelo túnel; teardown limpa tudo | ✅ |
| 3 | `chore/scaffold-rust` | workspace Cargo, `rust-toolchain.toml`, `deny.toml`, crates `core` e `cli`, CI Rust | `cargo fmt/clippy/test/deny` verdes | ✅ |
| 4 | `feat/engine-rust` | `core::engine`: contexto embutido com hash, build e ciclo do container via bollard | integração: sobe e derruba o engine pelo Rust | ✅ |
| 5 | `feat/vici` | cliente VICI atrás da trait `Charon` + tipos das mensagens | integração com charon real (`version`, `load-conn`/`list-conns`/`unload-conn`); superado: túnel completo só pelo cliente Rust | ✅ |
| 6 | `feat/perfis` | modelo, validação, presets, import/export, geração do `load-conn` e do `updown` | snapshots (insta) de um perfil genérico IKEv1 agressivo + XAuth; túnel real a partir de perfil JSON | ✅ |
| 7 | `feat/segredos` | keyring + memória (`secrecy`) + redação | testes com backend mock | ⏳ |
| 8 | `feat/sessao-cli` | máquina de estados, preflight, eventos, erros; CLI `connect/disconnect/status/logs/secrets/doctor/profiles` | mocks + E2E com gateway local + **E2E real** (local, com o perfil do usuário) | ⏳ |
| 9 | `chore/scaffold-gui` | Tauri v2 + React 19/TS/Vite + Tailwind v4 + shadcn/ui, biome, vitest | `npm run tauri dev` abre a janela | ⏳ |
| 10 | `feat/ui-conexoes` | lista de perfis, conectar/desconectar, credenciais (com "lembrar"), status, logs ao vivo | vitest + mockIPC; teste manual | ⏳ |
| 11 | `feat/ui-editor-perfis` | criar/editar/duplicar/excluir; abas Geral/Fase 1/Auth/Fase 2/DNS-Rede/Avançado; import/export | vitest | ⏳ |
| 12 | `feat/diagnostico` | tela de diagnóstico + `doctor` | — | ⏳ |
| 13 | `feat/tray` | ícone na bandeja (no GNOME exige a extensão AppIndicator; sem ela, some sem erro) | — | ⏳ |
| 14 | `feat/import-swanctl` | importar um `swanctl.conf` existente para perfil, sem os segredos | testes do parser | ⏳ |
| 15 | `chore/release` | ícone, pacotes rpm/deb/AppImage, CHANGELOG (git-cliff), tag `v0.1.0` | instala e conecta | ⏳ |

## Depois da v0.1.0 (ideias)

- Várias conexões simultâneas. O `if_id` por perfil já prevê isso.
- Imagem do engine publicada e assinada no GHCR, para evitar o build local.
- Avaliar Podman.
- Restringir as portas 500/4500 ao IP do gateway durante a sessão.
- Internacionalização (en-US).
- Remover também as *políticas* XFRM órfãs depois de um `SIGKILL` do engine. O
  iproute2 5.5 do Ubuntu 20.04 não consegue; as opções são embutir um `ip`
  moderno na imagem ou mandar o netlink direto. Hoje elas são inertes e somem
  no reboot (ver arquitetura, "Contrato do engine").
