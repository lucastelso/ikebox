# ADR-0001: Engine em container Docker com strongSwan 5.8.2

- **Status:** Substituído pelo ADR-0013 para o usuário final. O Docker continua nos testes.
- **Data:** 2026-11-15

## Contexto

VPNs IPsec legadas (IKEv1, modo agressivo, PSK de grupo + XAuth, DH
modp768/modp1024, SHA1) ainda são comuns em concentradores corporativos. Nas
distribuições Linux atuais, conectar a elas costuma falhar: as políticas de
criptografia do sistema bloqueiam os algoritmos antigos, e os plugins do
NetworkManager não cobrem bem essa combinação de autenticação.

Um fluxo manual já está validado em produção pelo mantenedor: o strongSwan
5.8.2 do Ubuntu 20.04 roda num container com `--network host`. O daemon IKE
(charon) negocia dentro do container e instala as SAs e políticas no kernel
do **host**, via netlink XFRM.

## Decisão

- O "engine" é uma imagem Docker baseada em `ubuntu:20.04`, **fixada por
  digest**, com os pacotes do strongSwan fixados em `5.8.2-1ubuntu3.6`.
- A lista de pacotes é a mesma do fluxo validado. A única adição é `systemd`
  (sem recommends), para ter o `resolvectl` (ver ADR-0004).
- O container usa `--network host`. O IPsec em si (ESP) é feito pelo kernel do
  host.

## Consequências

- **Positivas.**
  - Funciona com as VPNs legadas sem rebaixar a criptografia do host.
  - O comportamento é reproduzível: a mesma versão do strongSwan roda em
    qualquer distribuição.
  - O host não ganha pacotes de IPsec instalados.
- **Negativas.**
  - Exige Docker, e o usuário precisa estar no grupo `docker`, o que equivale
    a privilégio de root (ver SECURITY.md).
  - O Ubuntu 20.04 saiu do suporte padrão. Novas correções de segurança do
    strongSwan podem não chegar ao arquivo público, e por isso a exposição é
    limitada no tempo (ADR-0002) e na superfície (ADR-0005).
  - Quando o digest da base ou a versão dos pacotes mudar, o build da imagem
    falha de propósito, em vez de mudar em silêncio.

## Alternativas consideradas

- **strongSwan/libreswan do host.** Exige afrouxar as políticas de criptografia
  do sistema inteiro, e as versões variam entre distribuições.
- **NetworkManager-strongswan/l2tp.** Não cobre bem PSK de grupo + XAuth em
  modo agressivo.
- **Compilar o strongSwan 5.x estático no host.** Mais manutenção e mais
  superfície de build. Perde a validação já feita com o pacote do Ubuntu.
- **Podman rootless.** Não consegue configurar XFRM no namespace de rede do
  host sem privilégios equivalentes. Fica para avaliar no futuro.
