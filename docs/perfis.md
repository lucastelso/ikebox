# Perfis de conexão

Um perfil é um arquivo JSON que descreve **a VPN**:

- gateway;
- propostas;
- rotas;
- DNS.

Ele **nunca** contém credenciais: nem PSK, nem usuário ou senha XAuth. Por isso
um perfil pode ser passado para colegas, e cada pessoa informa as próprias
credenciais no app, com a opção de guardá-las no chaveiro do sistema.

- Formato: [ADR-0008](adr/0008-perfis-genericos.md).
- Por que `presets.local/` não é embutido no binário:
  [ADR-0012](adr/0012-presets-locais-nao-embutidos.md).
- Exemplos: [`presets/`](../presets).

## Onde ficam

| Local | Uso |
|---|---|
| `~/.config/ipsec-client/profiles/<id>.json` | Perfis do usuário (diretório 0700, arquivos 0600) |
| `presets/` (no repositório) | Exemplos genéricos, só com endereços de documentação |
| `presets.local/` (no repositório, **ignorado pelo git**) | Lugar para os seus perfis com dados reais, antes de importar |

## Formato (schema 1)

```jsonc
{
  "schema_version": 1,
  "id": "minha-vpn",                // [a-z0-9-]: arquivo, conexão no charon, chave no chaveiro
  "name": "Minha VPN",               // nome exibido
  "gateway": "vpn.example.com",      // nome DNS ou IP
  "ike": {
    "version": 1,                    // 1 ou 2
    "aggressive": true,              // só IKEv1
    "encap": true,                   // força NAT-T (UDP 4500)
    "proposals": ["aes256-sha1-modp1024"],
    "rekey_time_secs": 86400,        // opcional
    "dpd_delay_secs": 30             // opcional (sem = sem DPD)
  },
  "auth": {
    "method": "psk-xauth",           // "psk" | "psk-xauth" (só IKEv1)
    "local_id": "keyid:grupo",       // opcional: identidade da rodada PSK
    "remote_id": "vpn.example.com"   // opcional: identidade esperada do gateway
  },
  "child": {
    "name": "rede-interna",          // padrão: "rede"
    "esp_proposals": ["aes256-sha1-modp1024"],
    "rekey_time_secs": 43200,        // opcional
    "remote_ts": ["192.0.2.0/24"],   // redes acessadas; 0.0.0.0/0 = túnel completo
    "local_ts": [],                  // opcional; vazio = o IP virtual recebido
    "dpd_action": "restart",         // "none" | "trap" | "restart"
    "close_action": "restart"        // "none" | "trap" | "restart"
  },
  "network": {                       // opcional (valores padrão abaixo)
    "request_vip": true,             // pede IP virtual ao gateway
    "mtu": 1400,
    "dns": {
      "servers": ["192.0.2.53"],     // DNS fixos
      "domains": ["~corp.example.com"], // "~x" só roteia; "x" também entra na busca; "~." = tudo
      "use_received": true,          // usa também os DNS enviados pelo gateway
      "default_route": false         // usa estes DNS para nomes fora dos domínios
    }
  }
}
```

Campos desconhecidos são recusados. Assim, um erro de digitação, ou uma
tentativa de colocar a `"psk"` no arquivo, não passa despercebido.

## Montando um perfil a partir de um `swanctl.conf`

Se você já tem um `swanctl.conf` funcionando, cada chave tem um equivalente.
A importação automática está no roadmap (`feat/import-swanctl`); até lá, a
tabela abaixo serve de guia.

| `swanctl.conf` | Perfil |
|---|---|
| `remote_addrs = X` | `gateway` |
| `version = 1` | `ike.version` |
| `aggressive = yes` | `ike.aggressive: true` |
| `encap = yes` | `ike.encap: true` |
| `proposals = a,b` | `ike.proposals: ["a", "b"]` |
| `rekey_time = 86400s` | `ike.rekey_time_secs: 86400` |
| `dpd_delay = 30s` | `ike.dpd_delay_secs: 30` |
| `vips = 0.0.0.0` | `network.request_vip: true` |
| `local-1 { auth = psk; id = X }` | `auth.local_id: "X"` |
| `local-2 { auth = xauth; xauth_id = ... }` | `auth.method: "psk-xauth"`. O usuário **não** vai no perfil: é credencial |
| `remote { auth = psk; id = X }` | `auth.remote_id: "X"` (se houver `id`) |
| `children { NOME { ... } }` | `child.name: "nome"` (minúsculas, dígitos e hífen) |
| `esp_proposals = a,b` | `child.esp_proposals` |
| `rekey_time` (no child) | `child.rekey_time_secs` |
| `remote_ts = X` | `child.remote_ts: ["X"]` |
| `dpd_action = restart` | `child.dpd_action: "restart"` |
| `close_action = start` | `child.close_action: "restart"` (no charon, `start` e `restart` são a mesma ação) |
| `secrets { ... }` | **Nada.** A PSK, o usuário e a senha são informados no app |
| DNS configurado à mão (ex.: `resolvectl dns`/`domain`) | `network.dns.servers` e `network.dns.domains` |

Coloque o arquivo em `presets.local/`, que nunca vai para o git, e importe-o
pelo app ou pelo CLI (`ipsec-client-cli profiles import`, no roadmap).

## Passando o perfil para um colega

1. Exporte o perfil pelo app ou pelo CLI. O arquivo não contém credenciais.
2. Envie o arquivo por um canal privado, nunca por um repositório público.
3. Passe a PSK do grupo por um canal seguro e separado.
4. O colega importa o arquivo e informa as próprias credenciais: o usuário e a
   senha XAuth dele, mais a PSK do grupo.
