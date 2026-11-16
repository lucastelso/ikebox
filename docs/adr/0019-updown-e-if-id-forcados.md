# ADR-0019: Quem fala VICI não escolhe o updown nem o if_id (engine embutido)

- **Status:** Aceito. Implementado no branch `fix/engine-updown-forcado` (v0.1.1).
  Corrige uma escalada local de privilégio da v0.1.0 achada na auditoria de
  segurança (passo 32 do diário).
- **Data:** 2026-11-16
- **Complementa:** ADR-0003 (controle via VICI) e ADR-0014 (helper com polkit).

## Contexto

No engine embutido, o charon roda **como root**, numa unidade transitória do
systemd. O usuário da sessão fala com ele direto pelo socket VICI: é assim que
os segredos vão do app ao charon sem passar pelo helper (ADR-0007).

O VICI deixa o cliente definir, em cada CHILD_SA, o **`updown`**. Esse comando
o charon executa via `/bin/sh -c` a cada túnel que sobe ou desce, como root.
Na v0.1.0, quem mandava esse comando era o app, ou seja, o lado sem
privilégio. O resultado é uma escalada de privilégio para root, sem senha:
1. um programa malicioso, rodando como o usuário da sessão ativa, pede ao
   helper para subir o engine (o polkit é `allow_active`);
2. carrega uma conexão com `updown = "<qualquer comando>"`;
3. completa um túnel com um gateway qualquer, que pode ser dele mesmo.

O sandbox da unidade não impedia o pior: `/proc/sys` era gravável
(`kernel.core_pattern`, `kernel.modprobe`), os discos apareciam em `/dev`, e o
uid 0 lê arquivos do root sem precisar de capability.

Pelo mesmo caminho, o cliente também escolhia o `if_id`. Com isso, conseguia
instalar SAs e políticas fora da interface da sessão.

Não é explorável pela rede nem pelo gateway: exige código rodando como o
usuário. O teste `tests/engine/e2e.sh`, cenário 5, reproduz o ataque. Com o
strongSwan da v0.1.0, o comando do cliente roda.

## Decisão

1. **Patch mínimo no strongSwan**
   (`native-engine/patches/0001-vici-forced-updown-if-id.patch`).
   - Com `charon.plugins.vici.force_updown` definido, o parser VICI troca o
     `updown` de toda CHILD_SA pelo valor configurado e registra "ignoring
     updown command supplied by client".
   - Com `force_if_id` definido, força `if_id_in` e `if_id_out` na CHILD_SA e
     na conexão.
   - Sem as opções, o comportamento é o do upstream.
   - O build aplica o patch **depois** de conferir SHA-256 + GPG do
     código-fonte e registra o hash no `BUILDINFO`. O empacotamento, o build
     de release e o `verify-release.sh` recusam um engine sem os patches do
     commit.
2. **O root define o comando, com argumentos que não passam por shell.**
   - O entrypoint escreve as opções na config de runtime, que fica no
     diretório de runtime da unidade (0700, root):
     - `force_updown = /opt/ipsec-client/libexec/updown --args-file <arquivo>`;
     - `force_if_id = <if_id da sessão>`.
   - Os argumentos (interface e DNS) vão **um por linha** num arquivo do
     root. O `updown` lê esse arquivo como dados e valida tudo de novo.
3. **O DNS vai pelo helper.** `StartParams` ganha `dns` (servidores, domínios,
   flags), validado no `ipsec-client-proto` com as mesmas regras do perfil. No
   engine embutido, o `load-conn` vai sem `updown`.
   - Um helper antigo recusa o campo novo. O app então mostra "atualize o
     componente de rede", com o botão "Atualizar componente de rede".
4. **Defesa em profundidade.**
   - **Sandbox do engine:** `PrivateDevices`, `ProtectKernelTunables`
     (`/proc/sys` só leitura), `ProtectKernelLogs`, `ProtectProc=invisible`,
     `PrivateIPC`, `UMask=0077` e `SystemCallFilter=@system-service`.
     - O charon tentava ajustar o `xfrm_acq_expires`, global do host, e agora
       não consegue. Isso é opcional e só afeta políticas de trap, que o app
       não usa.
   - **Socket VICI:** fica `0600` com dono = uid da sessão, num diretório
     `0711` do root. Antes era `root:<gid efetivo>` `0660`, e outros membros
     do grupo também acessavam.
   - **Helper:**
     - `Stop` e `Logs` só para o dono da sessão (ou root). O log tem gateway,
       id do grupo e login;
     - `Status` esconde o perfil de outro usuário;
     - conexões atendidas em paralelo (até 8), com 3 s para o pedido chegar.
       Antes, um cliente parado travava o helper.

## Consequências

- **O usuário deixa de controlar qualquer texto executado como root.** O que
  ele ainda controla pelo VICI (conexões, segredos, iniciar e terminar) só
  afeta a própria sessão, na interface dela.
- **Mantemos um patch no strongSwan.** Ao atualizar a versão, o build falha
  se o patch não aplicar: é preciso portá-lo. As versões de reserva 5.x
  (ADR-0016) não têm o patch portado e por isso não podem ser empacotadas.
- **Risco residual:**
  - o charon continua root dentro do sandbox. O próximo passo de
    endurecimento é rodá-lo com um usuário próprio e capabilities ambientes
    (`NET_ADMIN`, `NET_BIND_SERVICE`, `NET_RAW`), o que depende de validar o
    `resolvectl` sem root;
  - um bug de memória no charon explorável por quem fala VICI continuaria
    valendo root, limitado pelo sandbox.
- **Testes:**
  - `npm run test:native`, cenário 5: o ataque, que passa com o patch e falha
    com o engine da v0.1.0;
  - `npm run test:systemd`: systemd de verdade em container. Cobre o
    instalador real, o helper, o sandbox aplicado, dois usuários (bob não
    para, não lê o log e não acessa o VICI de alice) e o ataque.
- **Quem usa a v0.1.0** precisa atualizar o app **e** clicar em "Atualizar"
  no componente de rede. A correção está no componente.

## Alternativas consideradas

- **Proxy VICI filtrante em Rust.** O usuário falaria com um proxy, que
  recusaria `updown` e `if_id` antes de repassar ao charon. Descartado por
  dois motivos:
  - seria mais código com privilégio analisando, o tempo todo, entrada não
    confiável (o fluxo de eventos inclusive);
  - o filtro precisaria acompanhar cada versão do protocolo.

  O patch tem poucas linhas e fica no ponto exato em que o valor é lido.
- **Só rodar o charon sem root.** Isso reduz o estrago, mas o `updown` do
  cliente ainda rodaria com `NET_ADMIN` e `NET_RAW`: capturar e desviar o
  tráfego do computador inteiro. Fica como passo seguinte, somado ao patch.
- **Validar o texto do `updown` no charon (prefixo fixo + caracteres
  permitidos).** A gramática do shell tem armadilhas (expansão de `~`,
  quebras de linha). Forçar o valor e passar os argumentos como dados é mais
  simples de verificar.

