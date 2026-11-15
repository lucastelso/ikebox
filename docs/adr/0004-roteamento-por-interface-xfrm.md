# ADR-0004: Roteamento e DNS por interface XFRM dedicada

- **Status:** Aceito
- **Data:** 2026-11-15

## Contexto

O fluxo manual fazia três coisas que mexiam no host além do necessário:

- colocava o IP virtual (VIP) e a rota da VPN **na interface física**
  (Wi-Fi/Ethernet), que é gerenciada pelo NetworkManager;
- configurava o DNS nessa mesma interface;
- na limpeza, fazia `ip xfrm state flush` e `ip xfrm policy flush` **globais**.

Isso conflitava com o NetworkManager e apagava qualquer outro IPsec do host.
Também exigia lógica frágil para achar e remover VIPs "residuais".

Desde o strongSwan 5.8.0 e o Linux 4.19, existem **interfaces XFRM**. Elas são
interfaces de rede virtuais ligadas a SAs e políticas marcadas com um `if_id`.

## Decisão

- O entrypoint cria a interface `ipsc0` (`type xfrm if_id <N>`), com MTU 1400.
- O `if_id` é derivado do perfil e nunca é 0.
- As SAs e políticas da conexão recebem `if_id_in`/`if_id_out = N`.
- Configuração do charon:
  - `install_virtual_ip_on = ipsc0`: o VIP fica na interface dedicada;
  - `install_routes = no`: o charon não instala rotas;
  - plugin `resolve` desligado.
- O `updown.sh` roda como root no container, no namespace de rede do host:
  - **rotas:** `ip route replace <TS remoto> dev ipsc0 src <VIP>`;
  - **DNS split:** `resolvectl dns|domain|default-route ipsc0 …`, pelo D-Bus do
    host. Domínios `~dominio` só roteiam consultas, não entram na busca;
  - **túnel completo** (`0.0.0.0/0`): rotas `0.0.0.0/1` + `128.0.0.0/1`, mais uma
    rota de host para o gateway pelo caminho original, para não criar loop.
- Os argumentos do `updown` são validados no Rust e de novo no script.
- **Teardown:** o charon remove SAs e VIP, e o entrypoint faz
  `resolvectl revert ipsc0` + `ip link del ipsc0`. Rotas, VIP e DNS somem junto
  com a interface.

## Consequências

- A interface física e o NetworkManager não são tocados.
- O MTU 1400 na interface evita o "blackhole" de pacotes grandes no Wi-Fi.
- Nenhum flush global: outras VPNs (WireGuard, Tailscale, outro IPsec) não são
  afetadas.
- A limpeza é atômica: apagar a interface desfaz tudo.
- Exige kernel ≥ 4.19 com o módulo `xfrm_interface`. O diagnóstico do app
  verifica isso.
- É uma mudança em relação ao fluxo validado e precisa de teste ponta a ponta.
  O repositório tem um teste com um gateway IPsec local
  (`engine/tests/e2e.sh`).

## Alternativas consideradas

- **VIP e rotas na interface física (fluxo manual).** Conflita com o
  NetworkManager e exige flush global.
- **Tabela 220 do charon (`install_routes = yes`).** Funciona, mas espalha rotas
  e regras de política pelo host, e o DNS continuaria na interface física.
- **Interfaces VTI.** São mais antigas, exigem marcas (`mark`) e são menos
  flexíveis que XFRM.
