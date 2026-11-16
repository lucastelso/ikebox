# ADR-0016: strongSwan 6.1.0 no engine embutido, com a 5.8.2 de reserva

- **Status:** Aceito
- **Data:** 2026-11-16

## Contexto

O mantenedor pediu "a versão mais nova que funcionar". O spike compilou e
testou quatro versões com a mesma configuração: 6.1.0 (atual), 6.0.7, 5.9.14 e
5.8.2 (a versão validada no fluxo manual).

Todas passaram nos **26 testes** do E2E contra o gateway de teste (que roda a
5.8.2 do Ubuntu, como as VPNs legadas). A 6.1.0 não registrou nenhum aviso de
algoritmo fraco ou obsoleto.

Os cenários cobertos foram:

- IKEv1 agressivo com PSK de grupo + XAuth e modp1024;
- PFS só com modp768 na fase 2;
- interface XFRM e socket VICI acessível pelo usuário;
- DNS split;
- túnel completo;
- recuperação após `SIGKILL`.

## Decisão

- O engine embutido usa o **strongSwan 6.1.0**.
- O `native-engine/versions.txt` mantém 6.0.7, 5.9.14 e **5.8.2** compiláveis
  e testáveis (`native-engine/spike/run-e2e.sh <versão>`). A 5.8.2 é a reserva
  se a VPN real de algum usuário mostrar diferença de comportamento.
- Cada versão é fixada por SHA-256 e conferida pela assinatura GPG oficial. A
  chave pública fica em `native-engine/keys/` (fingerprint
  `948F158A4E76A27BF3D07532DF42C170B34DBA77`).
- Atualizações do strongSwan entram por PR próprio, com o E2E completo verde.

## Consequências

- O app ganha as correções de segurança do strongSwan desde 2020.
- O comportamento com gateways reais pode diferir da 5.8.2. Isso é mitigado
  pelo E2E e pela validação na VPN do mantenedor antes do release. Se algo
  falhar, basta voltar à 5.8.2, sem mudar código.
- Sem OpenSSL, não há ECP (ADR-0013). O preset IKEv2 de exemplo passou a usar
  `curve25519`.
