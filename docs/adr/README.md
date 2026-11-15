# Registros de decisão de arquitetura (ADRs)

Cada ADR registra **uma** decisão relevante: o contexto, o que foi decidido, as
consequências e as alternativas descartadas. ADRs não são editados depois de
aceitos. Se uma decisão mudar, um novo ADR a substitui e o antigo passa a
`Substituído por ADR-XXXX`.

| # | Decisão | Status |
|---|---|---|
| [0001](0001-engine-docker-strongswan-5.8.2.md) | Engine em container Docker com strongSwan 5.8.2 | Aceito |
| [0002](0002-container-sempre-recriado.md) | Container recriado a cada conexão | Aceito |
| [0003](0003-controle-via-vici.md) | Controle do charon via VICI | Aceito |
| [0004](0004-roteamento-por-interface-xfrm.md) | Roteamento e DNS por interface XFRM dedicada | Aceito |
| [0005](0005-privilegio-minimo.md) | Privilégio mínimo no container | Aceito |
| [0006](0006-imagem-versionada-por-hash.md) | Imagem do engine embutida e versionada por hash | Aceito |
| [0007](0007-segredos.md) | Segredos só em memória ou no chaveiro do sistema | Aceito |
| [0008](0008-perfis-genericos.md) | Perfis genéricos em JSON versionado | Aceito |
| [0009](0009-ipc-tipado-tauri-specta.md) | IPC tipado entre UI e Rust com tauri-specta | Aceito |
| [0010](0010-dados-privados-fora-do-repositorio.md) | Nenhum dado real de VPN no repositório | Aceito |

Modelo para novos ADRs: copie um existente e mantenha as seções
**Contexto · Decisão · Consequências · Alternativas consideradas**.
