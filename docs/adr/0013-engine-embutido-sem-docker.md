# ADR-0013: Engine embutido no app, sem Docker para o usuário

- **Status:** Aceito. Substitui o ADR-0001 para o usuário final; o Docker
  continua só nos testes. Adapta os ADRs 0002, 0005 e 0006 (ver abaixo).
- **Data:** 2026-11-16

## Contexto

O objetivo do projeto é um app desktop "baixe e rode" que substitua o
FortiClient. O engine em Docker (ADR-0001) tem três problemas para isso:

- o usuário precisa instalar o Docker;
- o usuário precisa entrar no grupo `docker`, o que equivale a root
  (SECURITY.md);
- a imagem depende do Ubuntu 20.04, que saiu do suporte padrão, e só o
  primeiro build já leva cerca de 1 minuto.

O motivo original do container era usar o strongSwan 5.8.2 **com os próprios
plugins de cripto**, sem as políticas de criptografia do host. Isso também se
consegue **compilando o strongSwan por conta própria**.

O spike `spike/engine-nativo` (diário, 2026-11-16) mostrou que dá:

- o strongSwan 6.1.0, 6.0.7, 5.9.14 e 5.8.2 compilam estáticos (gmp
  embutido), com os próprios plugins e sem OpenSSL;
- o charon depende **só da glibc** e o engine inteiro ocupa cerca de 4 MB
  instalado (1,8 MB compactado);
- num host Fedora 45 simulado, as quatro versões passaram nos 26 testes do
  E2E contra o gateway de teste: IKEv1 agressivo, PSK + XAuth, modp1024 e
  **PFS modp768**, interface XFRM, DNS split, túnel completo e recuperação
  após `SIGKILL`.

## Decisão

- O app leva o strongSwan compilado por nós (`native-engine/`):
  - tarball oficial conferido por **SHA-256 e assinatura GPG**;
  - builder Ubuntu 22.04 (glibc 2.35, a mesma base do AppImage do Tauri);
  - `--disable-shared --enable-static --enable-monolithic`;
  - lista explícita de plugins, sem OpenSSL;
  - tarball determinístico.
- O charon roda no host, iniciado pelo helper com privilégio (ADR-0014) e com
  os mesmos scripts e o mesmo contrato do engine atual: `entrypoint.sh`,
  `updown.sh`, socket VICI 0660, prontidão, `proto 241`, `if_id`.
- No `core`, o engine passa a ser uma trait `EngineBackend`:
  - `NativeBackend` é o padrão;
  - `DockerBackend` é o atual, para testes e desenvolvimento.

### O que muda nos ADRs anteriores

| ADR | No engine embutido |
|---|---|
| 0001 (Docker + 5.8.2) | Vale só para os testes (gateway de teste e o backend Docker) |
| 0002 (container recriado) | Vira "processo recriado": cada conexão sobe um charon novo, e o anterior sai pelo caminho gracioso |
| 0005 (privilégio mínimo) | O sandbox passa a ser do systemd (`systemd-run` com capabilities mínimas), ver ADR-0014 |
| 0006 (imagem por hash) | O engine é identificado pelo hash do tarball embutido |

## Consequências

- **Positivas.**
  - O usuário só instala o app.
  - Sem o grupo `docker`.
  - Engine de 4 MB.
  - strongSwan atual, com as correções de segurança.
  - **A limitação das políticas XFRM órfãs some**, porque o iproute2 do host
    aceita `if_id` no delete (cenário 2 do E2E: 0 políticas restantes).
- **Negativas.**
  - Exige um helper com privilégio (ADR-0014) e manutenção do build do
    strongSwan.
  - Sem OpenSSL, **não há curvas ECP** (`ecp256`/`ecp384`). Há modp, curve25519
    e AES-GCM/ChaCha20. Se um gateway exigir ECP, dá para avaliar um plugin
    OpenSSL ou wolfSSL estático.

## Alternativas consideradas

- **Manter o Docker.** Contraria o objetivo "baixe e rode" e o modelo de
  segurança.
- **Usar o strongSwan do host.** Esbarra nas políticas de cripto e nas
  diferenças entre distribuições, que foram o motivo do container.
- **Podman rootless.** Não tem `CAP_NET_ADMIN` no namespace de rede do host.
