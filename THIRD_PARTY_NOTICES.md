# Avisos de terceiros

O IPsec Client é licenciado sob a GPL-3.0-or-later (ver `LICENSE` e
ADR-0017). Ele distribui ou depende dos componentes abaixo, sob as licenças de
cada um.

## Engine embutido (distribuído compilado dentro do app)

### strongSwan

- **Licença:** GPL-2.0-or-later, com exceção para combinar com bibliotecas
  LGPL e OpenSSL. Os plugins des, md4 e md5 têm termos próprios, permissivos.
  Os textos completos vão no pacote do engine, em
  `opt/ipsec-client/engine/share/licenses/strongswan/`.
- **Código-fonte correspondente:**
  - tarball oficial em <https://download.strongswan.org/>, com a versão e o
    SHA-256 fixados em `native-engine/versions.txt` e a assinatura GPG
    conferida no build (chave em `native-engine/keys/`);
  - nenhuma modificação no código. As opções de compilação estão em
    `native-engine/build/build-strongswan.sh` e no `BUILDINFO` do pacote.

### GMP (GNU Multiple Precision Arithmetic Library)

- **Licença:** LGPL-3.0-or-later ou GPL-2.0-or-later. O texto vai no pacote do
  engine, em `opt/ipsec-client/engine/share/licenses/gmp/`.
- **Uso:** linkado estaticamente no charon (plugin `gmp`, grupos DH MODP).
- **Código-fonte correspondente:** pacote-fonte `gmp` do Ubuntu 22.04, na
  versão registrada no `BUILDINFO` (atualmente `2:6.2.1+dfsg-3ubuntu1`), em
  <https://launchpad.net/ubuntu/+source/gmp>. Para recompilar, use
  `native-engine/build.sh`.

## App desktop

- **Tauri** e crates Rust: MIT, Apache-2.0, BSD, ISC, MPL-2.0, Zlib,
  Unicode-3.0 e CDLA-Permissive-2.0, verificadas pelo `cargo deny` (ver
  `deny.toml`).
- **Frontend** (React, Radix UI, lucide, sonner etc.): MIT, Apache-2.0, ISC e
  0BSD.
- **WebKitGTK, GTK e dependências** (no AppImage): LGPL-2.1-or-later e outras
  licenças livres, como bibliotecas dinâmicas sem modificação.

## Backend Docker (testes e desenvolvimento)

A imagem `engine/` é **construída localmente** a partir dos pacotes do Ubuntu
20.04 (strongSwan 5.8.2). O projeto não distribui essa imagem.
