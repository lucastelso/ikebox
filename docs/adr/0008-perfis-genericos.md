# ADR-0008: Perfis genéricos em JSON versionado

- **Status:** Aceito. O trecho sobre embutir `presets.local/` foi substituído
  pelo ADR-0012.
- **Data:** 2026-11-15

## Contexto

O app nasceu para uma VPN específica, mas deve servir para qualquer VPN IPsec
que o strongSwan 5.8.2 suporte. Equipes também precisam distribuir uma
configuração pronta, em que cada pessoa só informa as próprias credenciais.

## Decisão

- Um perfil é um JSON com `schema_version` e cobre:
  - **geral:** nome, gateway, IKE v1/v2, modo main/aggressive, `encap` (força
    NAT-T);
  - **fase 1:** propostas IKE, lifetimes, DPD;
  - **autenticação:** rodadas `psk`/`xauth`/`pubkey`/`eap`, com tipo de
    identidade (`fqdn`/`keyid`/`ip`/`email`);
  - **fase 2:** propostas ESP (com PFS), lifetimes, TS local e remoto,
    `dpd_action`/`close_action`;
  - **rede/DNS:** VIP (pull/push), servidores DNS, domínios (`~dominio` para
    split), uso dos DNS recebidos, MTU.
- A validação acontece no Rust, que é a fonte da verdade. O frontend espelha as
  regras em zod.
- O perfil gera a mensagem `load-conn` do VICI e os argumentos do `updown`.
  Testes de snapshot (insta) garantem que a geração não muda sem querer.
- Perfis ficam em `~/.config/ipsec-client/profiles/<id>.json` e **nunca**
  contêm segredos (ADR-0007).
- Presets:
  - `presets/` traz exemplos genéricos versionados, com endereços de
    documentação (RFC 5737) e `example.com`;
  - `presets.local/` guarda presets com dados reais. Fica no `.gitignore`, é
    embutido no build quando existe e também pode ser importado ou exportado.
- Migrações de `schema_version` são explícitas e testadas.

## Consequências

- Serve para FortiGate, Cisco, Palo Alto, strongSwan etc., desde que a
  combinação seja suportada pelo 5.8.2.
- A UI do editor precisa expor bastante coisa. A aba "Avançado" esconde o que
  raramente muda.

## Alternativas consideradas

- **Guardar `swanctl.conf` cru como perfil.** É difícil de validar e editar na
  UI, e mistura segredos com a configuração. Fica como *importação*: roadmap,
  item "importar swanctl.conf".
- **TOML/YAML.** JSON é nativo no frontend e no serde, e `schema_version` resolve
  a evolução do formato.
