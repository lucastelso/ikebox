# ADR-0011: Cliente VICI próprio em vez do `rsvici`

- **Status:** Aceito (detalha a escolha de biblioteca deixada em aberto no
  ADR-0003)
- **Data:** 2026-11-16

## Contexto

O ADR-0003 escolheu o VICI para controlar o charon e indicou o `rsvici` como
candidato, com a ressalva de que uma implementação própria seria considerada
se ele não atendesse. Avaliação do `rsvici` 0.1.5:

- cerca de 1.100 linhas, pré-1.0, depende de `serde_vici` e de mais três
  crates de stream;
- serializa as mensagens via serde em buffers comuns, que não são zerados. O
  `load-shared` leva PSK e senha XAuth, e o ADR-0007 exige zeroização;
- inclui transporte TCP, que o app não usa (só socket Unix).

O protocolo é pequeno: pacotes com tamanho prefixado, 8 tipos de pacote e 6
tipos de elemento de mensagem. O teste do engine já tinha um cliente mínimo em
Python (`tests/engine/vici_version.py`).

## Decisão

Implementar o cliente em `crates/core/src/vici/`:

- `message`: modelo ordenado (`Message`/`Value`) com codificação e
  decodificação. **Todo valor fica em `Zeroizing<Vec<u8>>`**. O `Debug` nunca
  mostra valores. A decodificação limita a profundidade (32) e valida os
  tamanhos;
- `client`: transporte por socket Unix, com pacotes de no máximo 512 KiB, o
  mesmo limite do charon. Os buffers de envio são zerados. São duas conexões:
  `ViciClient` para comandos, incluindo os streamed como `list-sas`, e
  `ViciEvents` para eventos assinados;
- `charon`: a trait `Charon`, com as operações tipadas usadas pelo app e
  mockável na sessão, os tipos de resposta sem identidades (`IkeSa`, `ChildSa`)
  e o parse dos eventos (`CharonEvent`). `SharedSecret` tem `Debug` redigido,
  inclusive os donos, que são identidades.

## Consequências

- Os segredos passam só por buffers zerados, da UI ao socket.
- Nenhuma dependência nova além de `zeroize`, que já estava prevista para o
  ADR-0007.
- A manutenção do protocolo fica conosco. O risco é baixo: o VICI é estável
  desde a 5.2, e há testes unitários (formato de fio, entradas malformadas,
  servidor de mentira) e um teste de integração com um túnel IPsec real contra
  o gateway de teste.

## Alternativas consideradas

- **`rsvici`**: descartado pelos motivos acima.
- **`swanctl` via `docker exec`**: descartado no ADR-0003.
