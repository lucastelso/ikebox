# ADR-0007: Segredos só em memória ou no chaveiro do sistema

- **Status:** Aceito. A implementação está descrita em docs/arquitetura.md
  (`core::secrets`).
- **Data:** 2026-11-15

## Contexto

Uma VPN legada típica tem três segredos:

- a PSK do grupo, compartilhada pela equipe;
- o usuário XAuth;
- a senha XAuth, pessoal.

No fluxo manual, os três ficavam em texto puro num `swanctl.conf`.

## Decisão

- **Em memória**, os segredos usam `secrecy`/`zeroize`, que zeram a memória no
  drop. `Debug` e `Display` são redigidos (`***`) em todos os tipos que contêm
  segredo.
- **Persistência opcional** ("lembrar credenciais"): crate `keyring`, com
  backend Secret Service (GNOME Keyring/KWallet). O serviço é `ipsec-client` e
  a chave é `<perfil>:<campo>`.
- Os segredos **nunca** vão para:
  - arquivos de perfil (JSON);
  - variáveis de ambiente ou argumentos do container;
  - `docker inspect`;
  - logs do app ou do charon. O nível de log do charon fica ≤ 2, e nesses
    níveis ele não imprime segredos.
- O usuário XAuth é mascarado em toda saída. Exemplo: `ab***yz`.
- Perfil importado com PSK pré-preenchida: a PSK vai para o chaveiro e o JSON
  salvo fica sem ela.

## Consequências

- Sem "lembrar", o usuário digita as credenciais a cada conexão.
- Sem Secret Service (ex.: sessão sem chaveiro), "lembrar" fica desabilitado e
  o diagnóstico explica o motivo.

## Alternativas consideradas

- **Arquivo cifrado próprio.** Reinventa o chaveiro e cria o problema de onde
  guardar a chave mestra.
- **Variáveis de ambiente do container.** Aparecem em `docker inspect` e em
  `/proc/<pid>/environ`.
