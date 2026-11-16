# ADR-0014: Helper com privilégio, autorizado uma vez (polkit + systemd)

- **Status:** Aceito. Implementado em `crates/helper`, `crates/proto` e
  `packaging/` (branch `feat/helper`); a validação com root está pendente.
- **Data:** 2026-11-16

## Contexto

O charon embutido (ADR-0013) precisa de `CAP_NET_ADMIN` no namespace de rede do
host (XFRM, rotas, interface), das portas UDP 500/4500 e de acesso ao D-Bus do
sistema (resolvectl). O app roda como usuário comum.

Decisão do mantenedor: a permissão de administrador é pedida **uma vez, na
instalação**. Depois, conectar e desconectar não pede senha, como no
NetworkManager.

## Decisão

- **`ipsec-client-helper`** (crate `crates/helper`, Rust):
  - serviço do systemd ativado por socket (`/run/ipsec-client-helper.sock`);
  - roda como root e expõe uma API **mínima**: `start` (com a `EngineSpec`
    validada pelo mesmo `EngineSpec::validate` do core), `stop`, `status` e
    `cleanup`;
  - nunca recebe segredos: os segredos vão do app direto ao charon, pelo
    socket VICI 0660 do usuário.
- **Autorização** a cada pedido:
  - uid do cliente via `SO_PEERCRED`;
  - checagem no polkit da ação `io.github.lucastelso.ipsecclient.connect` com
    `allow_active=yes`: o usuário da sessão local ativa conecta sem senha, e
    sessões remotas ou inativas precisam de administrador.
- **Sandbox do charon:** o helper o sobe com `systemd-run` como unidade
  transitória, com:
  - `CapabilityBoundingSet`/`AmbientCapabilities` só com `CAP_NET_ADMIN` e
    `CAP_NET_BIND_SERVICE` (mais as que o updown precisar);
  - `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`,
    `NoNewPrivileges=yes`;
  - `ReadWritePaths` só no diretório do socket e no runtime;
  - `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK`.
- **Instalação:**
  - nos pacotes **rpm/deb**, o helper, a unit e a policy vêm no pacote;
  - no **AppImage**, o primeiro uso oferece "Instalar componente de rede". O
    sistema pede a senha de administrador uma vez (`pkexec`), e o instalador
    copia o helper e o engine para `/usr/lib/ipsec-client`, verifica o hash,
    instala a unit e a policy e ativa o socket. Há também desinstalação.

## Consequências

- O usuário autoriza uma vez, e o dia a dia não tem senha.
- O código que roda como root fica num binário pequeno, com testes e fuzz da
  desserialização dos pedidos. O charon roda isolado pelo systemd.
- O AppImage precisa do passo de instalação do componente, porque o root não
  consegue ler o ponto de montagem FUSE do AppImage do usuário.

## Alternativas consideradas

- **`pkexec` a cada conexão.** Pede senha todo dia. Recusado pelo mantenedor.
- **`setcap` no charon.** As capabilities ficariam num binário que qualquer
  usuário pode executar, sem autorização por sessão.
- **Daemon próprio sem systemd.** Reinventa ativação, sandbox e logs.
