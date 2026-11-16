# Guia rápido: conectar na VPN com um perfil recebido

Este guia é para quem **recebeu um arquivo de perfil** (`algo.json`) de um
colega e quer conectar na VPN pelo Linux. Leva uns 5 minutos na primeira vez.

Você vai precisar de:

- o **instalador do IPsec Client** (`.rpm`, `.deb` ou `.AppImage`): da página
  Releases do repositório ou de quem te passou o perfil;
- o **arquivo de perfil** (`algo.json`);
- a **PSK do grupo** (a "chave do grupo"): peça a quem te passou o perfil, por
  um canal seguro;
- o seu **usuário** e a sua **senha** da VPN;
- a **senha de administrador** do computador, uma vez só.

---

## 1. Instalar o IPsec Client (uma vez)

Na pasta onde está o instalador:

- **Fedora:** `sudo dnf install ./ipsec-client-*.x86_64.rpm`
- **Ubuntu/Debian:** `sudo apt install ./ipsec-client_*_amd64.deb`
- **Outras distribuições:** use o AppImage, conforme a seção 2 do
  [manual de instalação](instalacao.md#outras-distribuições-appimage).

Depois disso, o **IPsec Client** aparece no menu de aplicativos.

## 2. Instalar o componente de rede (uma vez)

Abra o **IPsec Client** pelo menu. Na tela principal aparece *"Falta um passo:
instalar o componente de rede"*. Clique em **Instalar agora** e digite a
**senha de administrador**. Depois disso, conectar não pede mais essa senha.

## 3. Importar o perfil (uma vez)

Vá em **Perfis → Importar** e escolha o arquivo que você recebeu, ou
**arraste o arquivo** para a janela. Confira a prévia e clique em
**Importar**.

## 4. Conectar (todo dia)

Em **Conexão**, clique em **Conectar**. Na primeira vez, o app pede:

- **PSK do grupo:** a chave do grupo;
- **Usuário:** o seu usuário da VPN (ex.: matrícula);
- **Senha:** a sua senha da VPN.

Marque **Lembrar no chaveiro do sistema** para não digitar de novo. Quando
aparecer **Conectado**, a VPN está ativa: abra o Remmina (ou o que você usa)
normalmente.

**Para desconectar:** botão **Desconectar** (ou pelo ícone na bandeja).
Fechar a janela não desconecta.

---

## Deu erro?

| Mensagem | O que fazer |
|---|---|
| "Usuário ou senha XAuth incorretos" | Clique em **Informar credenciais** no aviso e digite de novo |
| "O gateway recusou a autenticação da fase 1" | A PSK do grupo está errada: **Informar credenciais** e digite de novo |
| "O gateway não respondeu" | Confira a sua internet. Algumas redes (hotel, celular) bloqueiam VPN |
| "Portas UDP 500/4500 em uso" | Outra VPN IPsec está ligada; desligue-a antes |
| "O computador ainda não está pronto para conectar" | Clique em **Instalar componente de rede** no próprio aviso |

Outros casos estão no [manual de instalação](instalacao.md#problemas-comuns).
A aba **Sistema → Diagnóstico** mostra o que falta, com a dica de como
resolver. Se precisar pedir ajuda, mande o que aparece ali e o log da tela de
conexão: eles não mostram senha nem PSK.

---

## Para quem vai passar o perfil adiante

Em **Perfis**, clique no botão de exportar (a seta para baixo) ao lado do
perfil e salve o arquivo. Ele **não tem** senha, usuário nem PSK.

Mande o arquivo, o instalador e o link deste guia. A PSK do grupo vai **por
outro canal**, nunca junto com o arquivo nem em grupo de chat aberto.

> Prefere o terminal? Existe o `ipsec-client-cli`, com os mesmos comandos:
> veja [docs/cli.md](cli.md).
