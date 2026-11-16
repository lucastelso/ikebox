# Linha de comando: `ipsec-client-cli`

O CLI tem a mesma lógica do app desktop, sem GUI. Ele serve para usar a VPN
no terminal, em scripts, e para validar o núcleo antes da interface gráfica.

## Instalação (a partir do código)

```bash
cargo build --release -p ipsec-client-cli
install -Dm755 target/release/ipsec-client-cli ~/.local/bin/ipsec-client-cli
```

Pré-requisitos de uso: Docker (com o seu usuário no grupo `docker`), kernel com
`xfrm_interface`, systemd-resolved e, opcionalmente, o chaveiro do sistema.
Para conferir tudo: `ipsec-client-cli doctor`.

> **Não use `sudo`.** O CLI roda como o seu usuário. Os privilégios de rede
> ficam no container do engine (ADR-0005).

## Primeiro uso

```bash
ipsec-client-cli doctor                                   # o ambiente está pronto?
ipsec-client-cli profiles import presets.local/vpn.json   # o perfil (ver docs/perfis.md)
ipsec-client-cli secrets set vpn                          # PSK, usuário e senha → chaveiro
ipsec-client-cli connect vpn                              # Ctrl+C desconecta
```

O primeiro `connect` constrói a imagem do engine, o que leva cerca de 1 minuto
e precisa de internet. Os seguintes conectam em poucos segundos.

## Comandos

| Comando | O que faz |
|---|---|
| `doctor` | Verifica usuário, runtime dir, Docker, `xfrm_interface`, resolved, D-Bus, portas 500/4500, chaveiro e imagem. Sai com 1 se algo impede a conexão |
| `profiles list` | Lista os perfis e avisa sobre arquivos inválidos |
| `profiles show <id>` | Mostra o perfil (JSON) |
| `profiles import <arquivo> [--force]` | Importa um perfil (valida antes; `--force` substitui um existente) |
| `profiles export <id> <arquivo>` | Exporta o perfil, sem credenciais, para passar a um colega |
| `profiles validate <arquivo>` | Valida sem importar e lista os problemas campo a campo |
| `profiles delete <id>` | Remove o perfil (as credenciais ficam no chaveiro) |
| `profiles path` | Mostra o diretório dos perfis |
| `secrets set <id> [--credentials-stdin]` | Pede (senhas sem eco) e guarda as credenciais no chaveiro. Enter mantém o valor atual |
| `secrets status <id>` | Mostra o que está guardado, sem revelar valores; o usuário aparece mascarado |
| `secrets forget <id>` | Apaga as credenciais do chaveiro |
| `connect <id>` | Conecta em primeiro plano. **Ctrl+C desconecta.** Quedas e retornos do túnel aparecem na tela |
| `connect <id> --detach` | Conecta e volta ao terminal; o túnel continua ativo |
| `connect <id> --remember` | Guarda no chaveiro as credenciais pedidas agora |
| `connect <id> --timeout N` | Tempo máximo da negociação (padrão 30 s) |
| `connect <id> --privileged` | Modo privilegiado de reserva do container (evite; ver SECURITY.md) |
| `status` | Mostra a conexão ativa: gateway, IP virtual, redes, tempo e tráfego. Sai com **3** se desconectado |
| `logs [-f] [-n N]` | Log do engine, com o usuário XAuth e o id do grupo mascarados |
| `disconnect` | Avisa o gateway (`terminate`) e derruba o engine. Funciona de qualquer terminal |
| `-v` / `-vv` | Mostra também o log do charon depois de conectar / mostra a depuração |

## Credenciais

Ordem de busca no `connect`:

1. o chaveiro do sistema (`secrets set`, ou `--remember`);
2. o que faltar, ele pergunta no terminal;
3. com `--credentials-stdin`, lê linhas `chave=valor` da entrada padrão. As
   chaves são `psk`, `xauth-user` e `xauth-password`. É para automação; o
   chaveiro não é usado nesse modo.

Nunca passe segredos por argumento ou variável de ambiente: eles aparecem em
`ps` e no histórico do shell. Para automação, use um arquivo 0600 com
redirecionamento:

```bash
ipsec-client-cli connect vpn --detach --credentials-stdin < ~/.config/minha-vpn.cred
```

## Erros comuns

O `connect` lê o log do charon e explica a causa provável:

| Mensagem | O que conferir |
|---|---|
| "o gateway recusou a autenticação da fase 1" | PSK do grupo e `auth.local_id` (FortiGate/Cisco: `keyid:<grupo>`) |
| "usuário ou senha XAuth incorretos" | `secrets set <id>` |
| "não aceitou nenhuma proposta da fase 1" | `ike.proposals` |
| "não aceitou nenhuma proposta da fase 2" | `child.esp_proposals` (algoritmos e grupo DH/PFS) |
| "o gateway recusou as redes pedidas" | `child.remote_ts`/`local_ts` |
| "o gateway não respondeu" | endereço, rede, firewall ou NAT (UDP 500/4500) |
| "Portas UDP 500/4500 em uso" | outra VPN IPsec conectada (ex.: um container antigo) |

## Comportamento

- O túnel é um container independente do terminal. Fechar o terminal de um
  `connect` em primeiro plano não derruba a VPN; use `disconnect`.
- Só uma conexão por vez. Um segundo `connect` é recusado e informa o perfil
  ativo.
- O código de saída do `status` (0 = conectado, 3 = desconectado) serve para
  scripts.
