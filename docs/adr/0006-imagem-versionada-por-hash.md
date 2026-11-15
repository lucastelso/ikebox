# ADR-0006: Imagem do engine embutida e versionada por hash

- **Status:** Aceito
- **Data:** 2026-11-15

## Contexto

O usuário final não deveria precisar clonar o repositório nem rodar
`docker build` para usar o app. Ao mesmo tempo, uma imagem velha com scripts
desatualizados causaria bugs difíceis de diagnosticar.

## Decisão

- O diretório `engine/` (Dockerfile e scripts) é embutido no binário com
  `include_bytes!`.
- O nome da imagem é `ipsec-client-engine:5.8.2-<hash8>`, onde `hash8` são os 8
  primeiros caracteres do SHA-256 do conteúdo de `engine/`. A imagem também
  recebe uma label com o hash completo.
- Ao conectar, se a imagem com esse nome não existir, o app monta o contexto
  tar em memória e faz o build pela Docker API (bollard), mostrando o progresso
  na UI.
- Imagens antigas (`ipsec-client-engine:*` com outro hash) podem ser removidas
  pelo diagnóstico.

## Consequências

- O binário e o engine sempre andam juntos. Não existe "imagem desatualizada".
- O primeiro uso exige rede para baixar a base e os pacotes, e leva cerca de
  1 minuto.
- Nenhum registry é necessário. Publicar a imagem num registry fica como
  otimização futura.

## Alternativas consideradas

- **Publicar a imagem num registry (GHCR).** Exige infraestrutura de
  publicação e assinatura. É uma otimização que pode vir depois.
- **Tag fixa (`:latest`).** Não detecta divergência entre app e engine.
