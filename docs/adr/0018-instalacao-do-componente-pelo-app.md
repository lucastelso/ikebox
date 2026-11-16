# ADR-0018: O app instala o componente de rede (pacote embutido + pkexec)

- **Status:** Aceito. Implementado no branch `feat/setup-appimage` e validado
  pelo mantenedor com o pedido de senha real (passo 30).
- **Data:** 2026-11-16

## Contexto

O engine embutido (ADR-0013) depende de um componente instalado como root: o
helper, o strongSwan compilado, as units do systemd e a policy do polkit
(ADR-0014). Nos pacotes rpm/deb, o gerenciador de pacotes instala esse
componente. No **AppImage**, ninguém instala: o usuário só baixa um arquivo e
o executa.

Há também uma limitação técnica: o root não consegue ler o ponto de montagem
FUSE do AppImage, que pertence ao usuário. Por isso o instalador não pode
rodar direto de dentro do AppImage.

## Decisão

- **Pacote do componente dentro do app.**
  - `scripts/stage-network-component.sh` monta um diretório com:
    - os arquivos: helper, tarball do strongSwan, `entrypoint.sh`, `updown`,
      configuração, units, policy e o próprio instalador;
    - `VERSION`, com a versão do app e a do strongSwan;
    - `SHA256SUMS`, com o hash de todos os outros arquivos.
  - O build de release (`npm run bundle`, que usa `tauri.release.conf.json`)
    coloca esse diretório entre os recursos do app, em `<recursos>/network`.
  - Os builds de desenvolvimento não trazem o pacote. Para testar, aponte um
    com `IPSEC_CLIENT_COMPONENT_BUNDLE`, que só funciona em build de debug.
- **Instalação pelo app** (`ipsec-client-core::component`):
  1. O app copia o pacote para um diretório novo, com modo 0700, em
     `$XDG_RUNTIME_DIR` (tmpfs, só do usuário). Na cópia, define as
     permissões e confere os hashes.
  2. Roda `pkexec <cópia>/install-native.sh install --bundle <cópia>
     --origin app`, com o ambiente limpo: nada de `LD_LIBRARY_PATH` do
     AppImage chegando ao root. O sistema pede a senha de administrador uma
     vez.
  3. O instalador, já como root, faz sua parte:
     - copia o pacote para um diretório **só dele**;
     - recusa links simbólicos e arquivos especiais;
     - confere o `SHA256SUMS` e exige a lista completa de arquivos;
     - recusa tarball com caminhos fora de `opt/ipsec-client/engine`;
     - só então instala.
- **Registro da instalação** em `/opt/ipsec-client`:
  - `VERSION` e `SHA256SUMS`, copiados do pacote;
  - `ORIGIN`, que diz quem instalou: `app`, `package` ou `manual`.

  O app compara o SHA-256 do `SHA256SUMS` instalado com o do pacote que ele
  traz. Se forem diferentes, o componente está **desatualizado** e o app
  oferece "Atualizar".
- **Remoção** com a cópia do instalador que fica em
  `/opt/ipsec-client/libexec/install-native.sh`. Antes de chamar o `pkexec`,
  o app confere que esse arquivo e os diretórios acima dele pertencem ao root
  e que mais ninguém pode alterá-los. A remoção nunca executa um arquivo do
  usuário como root.
- **Componente instalado por pacote do sistema** (`ORIGIN=package`): o app
  não instala nem remove. Ele avisa que isso se faz pelo gerenciador de
  pacotes.
- **Na interface:**
  - na tela Sistema, o cartão "Componente de rede" mostra o estado e as
    versões, com os botões Instalar/Atualizar/Reinstalar e Remover (este com
    confirmação);
  - na tela principal, um aviso de primeiro uso aparece quando falta algo;
  - no erro de pré-requisito, o botão "Instalar componente de rede".

  Com uma conexão ativa, instalar e remover são recusados, porque derrubariam
  o túnel.
- **No terminal:** `ipsec-client-cli component status|install|uninstall`.

## Consequências

- O usuário do AppImage instala tudo pelo próprio app, com a senha de
  administrador uma vez. Depois disso, conectar não pede senha (ADR-0014).
- Atualizar o AppImage traz um componente novo, e o app percebe a diferença
  sozinho.
- O mesmo instalador e o mesmo pacote servem para três caminhos: o app, o
  `scripts/test-native.sh` (com `--origin manual`) e, depois, o pós-instalação
  dos pacotes rpm/deb (com `--origin package`).
- `tests/packaging/install-test.sh` testa o instalador como root num container
  descartável, inclusive pacotes adulterados, e roda na CI.
- **Risco residual:** um programa malicioso rodando como o próprio usuário
  poderia trocar a cópia do pacote entre a conferência feita pelo app e a
  leitura pelo root. Esse programa já tem acesso a tudo do usuário, e nenhum
  AppImage consegue se proteger dele, porque o próprio arquivo baixado é a
  base da confiança. Os hashes protegem contra corrupção e contra outros
  usuários. Quem quiser a confiança do gerenciador de pacotes deve usar o rpm
  ou o deb.

## Alternativas consideradas

- **Executar o instalador direto do AppImage.** Impossível: o root não lê a
  montagem FUSE do usuário.
- **Policy do polkit própria para a instalação.** Daria uma mensagem mais
  bonita no pedido de senha, mas a policy precisaria já estar instalada. É um
  problema do ovo e da galinha.
- **Baixar o componente da internet no primeiro uso.** Exige rede, servidor e
  verificação de assinatura, e o app deixaria de funcionar offline.
- **Instalar em `/usr/lib/ipsec-client`**, como o ADR-0014 previa. Trocado por
  `/opt/ipsec-client`, que não colide com arquivos dos pacotes da
  distribuição e é fácil de remover por inteiro.
