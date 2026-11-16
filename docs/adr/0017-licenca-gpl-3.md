# ADR-0017: Licença GPL-3.0-or-later

- **Status:** Aceito
- **Data:** 2026-11-16

## Contexto

O projeto vai ser publicado como código aberto. Ele é uma ferramenta de
**segurança** (credenciais, rede, privilégio) e substitui um cliente
proprietário, e o app distribui dentro de si o **strongSwan compilado**
(ADR-0013).

| Componente | Licença | Relação com o app |
|---|---|---|
| strongSwan (charon, swanctl) | GPL-2.0-or-later, com exceção para combinar com bibliotecas LGPL e OpenSSL. Os plugins des/md4/md5 têm termos próprios, permissivos | Distribuído compilado; roda como **processo separado** e conversa pelo socket VICI |
| GMP (estático no charon) | LGPL-3.0-or-later ou GPL-2.0-or-later | Parte do binário do charon (permitido pela exceção do strongSwan) |
| Dependências Rust | MIT, Apache-2.0, BSD, ISC, MPL-2.0, Zlib, Unicode-3.0, CDLA-Permissive-2.0 | Linkadas no app |
| Dependências do frontend | MIT, Apache-2.0, ISC, 0BSD | Embutidas no app |
| WebKitGTK/GTK (AppImage) | LGPL | Bibliotecas dinâmicas levadas no AppImage |

Todas as dependências são compatíveis com a GPLv3. O app não é obra derivada
do strongSwan, porque são processos separados que se comunicam por um
protocolo. A licença do app é, portanto, uma escolha livre.

## Decisão

- O código do projeto é licenciado sob **GPL-3.0-or-later** (`LICENSE`).
- O SPDX `GPL-3.0-or-later` vai nos metadados: `Cargo.toml`, `package.json` e
  pacotes rpm/deb.
- Contribuições entram sob a mesma licença (*inbound = outbound*;
  CONTRIBUTING.md).
- **Conformidade dos componentes distribuídos**
  (`THIRD_PARTY_NOTICES.md`):
  - o tarball do engine leva os textos de licença do strongSwan e do GMP
    (`share/licenses/`) e um `BUILDINFO` com versões e hashes;
  - o código-fonte correspondente fica identificado com precisão: o tarball
    oficial do strongSwan (URL + SHA-256 + assinatura), o pacote-fonte do GMP
    do Ubuntu 22.04 e os scripts de build em `native-engine/`.

## Consequências

- Qualquer versão redistribuída, modificada ou não, precisa manter o
  código-fonte disponível, então ninguém pode lançar um derivado fechado de
  uma ferramenta de segurança.
- A licença é da mesma família do strongSwan que vai dentro do app, e código
  GPL-2.0-or-later pode ser reaproveitado se for preciso.
- Empresas podem usar o app livremente. A obrigação só existe ao
  **redistribuir**.
- Para trocar a licença depois, seria preciso o consentimento de todos os
  contribuidores. Hoje há só o mantenedor.

## Alternativas consideradas

- **MIT OR Apache-2.0**, o padrão do ecossistema Rust: máxima adoção, mas
  permite derivados fechados.
- **GPL-2.0-or-later**, igual ao strongSwan: seria incompatível com as
  dependências Apache-2.0 se alguém optasse só pela v2.
- **AGPL-3.0**: protege o uso como serviço de rede, o que não se aplica a um
  app desktop.
- **MPL-2.0**: copyleft só por arquivo, uma proteção mais fraca para o
  conjunto.
