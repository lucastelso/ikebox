# Registros de decisão de arquitetura (ADRs)

Cada ADR registra **uma** decisão relevante: o contexto, o que foi decidido, as
consequências e as alternativas descartadas. ADRs não são editados depois de
aceitos. Se uma decisão mudar, um novo ADR a substitui e o antigo passa a
`Substituído por ADR-XXXX`.

| # | Decisão | Status |
|---|---|---|
| [0001](0001-engine-docker-strongswan-5.8.2.md) | Engine em container Docker com strongSwan 5.8.2 | Substituído pelo 0013 (só nos testes) |
| [0002](0002-container-sempre-recriado.md) | Container recriado a cada conexão | Aceito |
| [0003](0003-controle-via-vici.md) | Controle do charon via VICI | Aceito |
| [0004](0004-roteamento-por-interface-xfrm.md) | Roteamento e DNS por interface XFRM dedicada | Aceito |
| [0005](0005-privilegio-minimo.md) | Privilégio mínimo no container | Aceito |
| [0006](0006-imagem-versionada-por-hash.md) | Imagem do engine embutida e versionada por hash | Aceito |
| [0007](0007-segredos.md) | Segredos só em memória ou no chaveiro do sistema | Aceito |
| [0008](0008-perfis-genericos.md) | Perfis genéricos em JSON versionado | Aceito |
| [0009](0009-ipc-tipado-tauri-specta.md) | IPC tipado entre UI e Rust com tauri-specta | Aceito |
| [0010](0010-dados-privados-fora-do-repositorio.md) | Nenhum dado real de VPN no repositório | Aceito |
| [0011](0011-cliente-vici-proprio.md) | Cliente VICI próprio em vez do `rsvici` | Aceito |
| [0012](0012-presets-locais-nao-embutidos.md) | Presets locais não são embutidos no binário | Aceito |
| [0013](0013-engine-embutido-sem-docker.md) | Engine embutido no app, sem Docker para o usuário | Aceito |
| [0014](0014-helper-privilegiado-polkit.md) | Helper com privilégio, autorizado uma vez (polkit + systemd) | Aceito |
| [0015](0015-distribuicao-appimage-rpm-deb.md) | Distribuição em AppImage, rpm e deb (sem Flatpak) | Aceito |
| [0016](0016-strongswan-6.1-compilado.md) | strongSwan 6.1.0 no engine embutido, com a 5.8.2 de reserva | Aceito |
| [0017](0017-licenca-gpl-3.md) | Licença GPL-3.0-or-later | Aceito |

Modelo para novos ADRs: copie um existente e mantenha as seções
**Contexto · Decisão · Consequências · Alternativas consideradas**.
