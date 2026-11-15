# ADR-0003: Controle do charon via VICI

- **Status:** Aceito. A escolha da biblioteca está detalhada no ADR-0011.
- **Data:** 2026-11-15

## Contexto

O fluxo manual montava um `swanctl.conf` com os segredos em texto puro e usava
`docker exec swanctl --load-all`. Os segredos ficavam em disco, e o resultado
era interpretado por `grep` na saída de texto.

O charon expõe o **VICI**, um protocolo binário estável sobre socket Unix. O
`swanctl` também usa o VICI por baixo.

## Decisão

- O app fala VICI direto com o charon, por um socket Unix que é bind-mount de
  `$XDG_RUNTIME_DIR/ipsec-client/charon.vici`.
- O entrypoint ajusta o socket para `root:<gid do usuário>` e modo `0660`, então
  só o usuário que iniciou a sessão consegue se conectar.
- O cliente VICI fica atrás de uma trait `Charon`, que pode ser substituída por
  um mock nos testes.
  - Uma conexão é usada para comandos.
  - Outra conexão é usada para eventos (`ike-updown`, `child-updown`, `log`).
- Operações usadas: `version`, `load-shared`, `load-conn`, `initiate`,
  `terminate`, `list-sas`, `unload-conn`.
- A biblioteca candidata é `rsvici` (MIT, async/tokio), com a versão fixada. Se
  ela não atender, o protocolo é simples o bastante para uma implementação
  própria e auditável. Essa troca seria registrada num novo ADR.

## Consequências

- Os segredos vão direto da memória do app para a memória do charon.
- Os erros chegam estruturados, sem depender de parse de texto.
- O log de controle aparece ao vivo na UI.
- O app passa a depender do protocolo VICI do 5.8.x, que é estável desde a 5.2.

## Alternativas consideradas

- **`docker exec swanctl`.** Os segredos precisariam ir para arquivo ou para
  argumentos (visíveis em `ps`), e as respostas seriam parse de texto.
- **Socket VICI por TCP.** Expõe o controle a qualquer processo local, sem
  controle de acesso por permissão de arquivo.
