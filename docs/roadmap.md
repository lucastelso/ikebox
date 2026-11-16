# Roadmap

Cada linha é um branch, criado a partir de `main` e integrado com
`merge --no-ff`, com o `npm run check` verde. Os branches ficam guardados como
histórico.

## Objetivo final

Um **app desktop para Linux que substitui o FortiClient**: bonito, simples e
seguro, distribuído como **AppImage** (baixe e rode) e como rpm/deb. Requisitos:

- o usuário **não instala nada além do app**: nem Docker, nem Node/npm, nem
  Rust;
- npm e Rust existem só na máquina de quem desenvolve e na CI. O Tauri compila
  o frontend para dentro do binário.

### Decisões de 2026-11-16 (mantenedor)

| Tema | Decisão |
|---|---|
| Engine | **Embutido no app**: strongSwan compilado por nós, sem Docker para o usuário. O Docker fica só nos testes |
| Privilégio | Autorizado **uma vez na instalação**, com a senha de administrador. Depois conecta sem senha, como o NetworkManager |
| strongSwan | A versão **mais nova que funcionar** (6.x → 5.9.14), com a 5.8.2 como reserva |
| Ordem | **Prova do engine → GUI → engine embutido → release** |
| Formatos | AppImage + rpm + deb. **Flatpak não**: o sandbox impede netlink/XFRM e serviços com privilégio |

## Arquitetura-alvo

```
usuário: App (Tauri 2 + React) ou CLI
  └── crates/core (profiles · secrets · session · vici · diagnostics)
       └── EngineBackend (trait)
            ├── NativeBackend ──(socket Unix + polkit)──► ipsec-client-helper (root, systemd)
            │                                               └── charon embutido (systemd-run com sandbox)
            └── DockerBackend (atual; só testes/dev)
```

## Feito (base: núcleo + CLI)

| # | Branch | Entrega | Status |
|---|---|---|---|
| 0 | `main` | README, ADRs 0001–0010, roadmap, diário, guarda de dados privados | ✅ |
| 1 | `chore/tooling` | lefthook, commitlint, shellcheck, `npm run check`, CI inicial | ✅ |
| 2 | `feat/engine-imagem` | Engine em container + E2E com gateway IPsec local | ✅ |
| 3 | `chore/scaffold-rust` | Workspace Cargo, toolchain, `cargo-deny`, CI Rust | ✅ |
| 4 | `feat/engine-rust` | `core::engine`: imagem embutida e ciclo do container | ✅ |
| 5 | `feat/vici` | Cliente VICI próprio (ADR-0011) | ✅ |
| 6 | `feat/perfis` | Perfis JSON, validação, geração do `load-conn` (ADR-0012) | ✅ |
| 7 | `feat/segredos` | Credenciais, chaveiro (libdbus), mascaramento | ✅ |
| 8 | `feat/sessao-cli` | Sessão, diagnóstico de falhas, `ipsec-client-cli` | ✅ |
| 14 | `feat/import-swanctl` | Importar `swanctl.conf` sem ler os segredos | ✅ |
| — | `docs/guia-rapido` | Guia para quem recebe um perfil exportado | ✅ |

## Próximas fases

### Fase A: fechar a base

| Entrega | Aceite | Status |
|---|---|---|
| Push de `main` e dos branches; primeira execução da CI no GitHub | CI verde (ou correções em `fix/ci`) | ⏳ com o mantenedor (o ambiente automatizado não tem credenciais do GitHub) |
| Validação real do mantenedor (`connect trabalho` + Remmina) | registrada no diário | ✅ pelo app (passo 20); falta o teste de longa duração |

### Fase B: prova do engine embutido (decide o caminho)

| Branch | Entrega | Aceite | Status |
|---|---|---|---|
| `spike/engine-nativo` | `native-engine/`: compila o strongSwan num container com glibc antiga, monolítico, com plugins próprios (gmp estático, sem OpenSSL) e tarball verificado. Testa 6.x → 5.9.14 → 5.8.2 | Contra o gateway de teste, com o charon no host: IKEv1 agressivo + PSK + XAuth + modp1024/768, `ipsc0` com `if_id`, VICI pelo usuário, updown, ping. Medir tamanho e dependências | ✅ as 4 versões passaram nos 26 testes; engine de cerca de 4 MB, dependendo só da glibc |
| (mesmo) | ADRs 0013 (engine embutido), 0014 (helper + polkit), 0015 (distribuição sem Flatpak), 0016 (versão do strongSwan) | aceitos | ✅ escolhido o strongSwan 6.1.0 (reserva: 5.8.2) |

### Fase C: interface gráfica

| Branch | Entrega | Status |
|---|---|---|
| `chore/scaffold-gui` | Tauri 2.12 (estável) + React 19 + Vite + Tailwind 4 + shadcn/ui, biome, vitest, tauri-specta com versão exata, capabilities mínimas, CSP estrita | ✅ |
| `feat/ui-conexao` | Tela principal (perfil, botão Conectar/Desconectar, estado com VIP, tempo e tráfego), credenciais com "lembrar", erros claros, log mascarado | ✅ |
| `feat/ui-perfis` | Gerenciador de presets: importar JSON/swanctl.conf (diálogo nativo e arrastar e soltar), **exportar**, ver detalhes, excluir | ✅ |
| `feat/ui-editor-perfis` | Criar/editar/duplicar perfis na interface, com abas e validação campo a campo vinda do Rust | ⏳ |
| `feat/ui-diagnostico` | Aba Sistema: diagnóstico (o `doctor` com dicas), tema claro/escuro/sistema, versão. "Iniciar com o sistema" e bandeja ficam no `feat/tray` | ✅ |
| `feat/tray` | Bandeja, notificações, instância única | ⏳ |

### Fase D: engine embutido de verdade

| Branch | Entrega | Status |
|---|---|---|
| `feat/engine-nativo-build` | Build reproduzível do strongSwan na CI. Em parte adiantado: o build e o E2E já rodam na CI (`npm run test:native`). Falta empacotar no bundle | ⏳ |
| `feat/helper` | `crates/helper` (root): socket systemd, `SO_PEERCRED` + polkit, charon via `systemd-run` com hardening | ⏳ |
| `feat/backend-nativo` | Trait `EngineBackend`, com `NativeBackend` (padrão) e `DockerBackend` (testes) | ⏳ |
| `feat/setup-appimage` | Primeiro uso do AppImage: instala o componente de rede com a senha de administrador, uma vez | ⏳ |

### Fase E: distribuição

| Branch | Entrega | Status |
|---|---|---|
| `chore/release` | AppImage + rpm + deb na CI, checksums assinados, GitHub Releases, tag `v0.1.0`. Aceite: numa VM limpa, só com o AppImage, conecta com um perfil exportado | ⏳ |
| `feat/updater` (opcional) | Atualização automática assinada | ⏳ |

**Antes de tornar público:** reescrita do e-mail dos commits e varredura completa
do histórico (`check-private.sh --history` + gitleaks). A licença já está
definida (GPL-3.0-or-later, ADR-0017).

## Depois da v0.1.0 (ideias)

- Várias conexões simultâneas. O `if_id` por perfil já prevê isso.
- Restringir as portas 500/4500 ao IP do gateway durante a sessão.
- Internacionalização (en-US).
- Autenticação por certificado e EAP (schema 2 do perfil).
- "Copiar log anonimizado": mascarar também gateway, DNS, domínios e redes internas para relatórios de bug.
