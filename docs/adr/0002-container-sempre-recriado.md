# ADR-0002: Container recriado a cada conexão

- **Status:** Aceito
- **Data:** 2026-11-15

## Contexto

No fluxo validado, reaproveitar um container parado com `docker start` deixava
estado velho para trás (pidfiles, sockets, SAs). Isso causava falhas
intermitentes na reconexão.

## Decisão

- Toda conexão faz `docker rm -f` do container anterior e cria um novo
  (`create` + `start`). O app nunca usa `docker start` num container existente.
- O entrypoint ainda apaga sockets e pidfiles velhos e recria a interface XFRM,
  como garantia extra.
- Ao desconectar: `terminate` via VICI, `stop` (10 s) e `rm`.

## Consequências

- O estado é sempre limpo e previsível.
- O strongSwan só escuta nas portas UDP 500/4500 enquanto a VPN está conectada.
- Criar o container custa menos de 1 s, o que é desprezível perto da
  negociação IKE.
- Os logs de sessões anteriores não ficam no container. O app guarda os logs
  da sessão atual em memória.

## Alternativas consideradas

- **Container persistente com `--restart`.** Deixa o daemon IKE exposto o tempo
  todo e acumula estado.
- **`docker start` de um container parado.** É o comportamento que causou as
  falhas no fluxo manual.
