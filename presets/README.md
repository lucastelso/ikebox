# Presets de exemplo

Perfis **genéricos**, só com endereços e nomes reservados para documentação
(RFC 5737 e RFC 2606). Servem de ponto de partida e são validados pelos testes.

| Arquivo | Cenário |
|---|---|
| `exemplo-ikev1-agressivo-xauth.json` | VPN legada típica (ex.: FortiGate): IKEv1 agressivo, PSK de grupo (`keyid:`) + XAuth, modp1024/768, split tunnel |
| `exemplo-ikev2-psk.json` | IKEv2 com PSK, propostas modernas, IPv4 + IPv6 |

Para criar o seu, copie um exemplo para `presets.local/`, que é ignorado pelo
git, e ajuste. O formato e a correspondência com o `swanctl.conf` estão em
[docs/perfis.md](../docs/perfis.md). **Não** coloque credenciais no arquivo:
elas são informadas no app.
