# Guia rápido: conectar na VPN com um perfil recebido

Português · **[English](../quick-start.md)**

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

Depois disso, o **IPsec Client** aparece no menu de aplicativos. Ele fala o
idioma do seu sistema (português ou inglês); para trocar, vá em **Sistema →
Aparência → Idioma**.

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

O que ele pede depende da VPN. Se a sua usa **certificado** (um arquivo
`.p12` ou `.pfx` da sua empresa), na primeira vez o app pede para **escolher
o arquivo** e digitar a senha dele: a chave vai para o chaveiro do sistema,
e depois você pode apagar o arquivo.

Marque **Lembrar no chaveiro do sistema** para não digitar de novo. O topo da
janela mostra o estado pela cor: âmbar enquanto conecta, **verde** com
**Conectado** quando a VPN está ativa (abra o Remmina, ou o que você usa,
normalmente) e **vermelho** quando não conectou: o cartão logo abaixo diz o
que fazer.

Abaixo do estado, um selo pode dizer **Segurança legada** ou **Segurança
fraca**. É um aviso sobre como a VPN está configurada (algoritmos ou modos
antigos), não um erro: a VPN funciona. Os motivos estão em **Perfis**, ao
abrir o perfil; só quem administra o gateway pode mudá-los.

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
resolver. Se precisar pedir ajuda, mande o que aparece ali e o log da conexão
copiado com **Copiar anonimizado**: ele não tem senha nem PSK, e o gateway,
os IPs e os domínios viram marcadores.

---

## Para quem vai passar o perfil adiante

Ainda não tem perfil? Em **Perfis → Novo**, escolha com o que a VPN se parece
(FortiGate, Cisco Easy VPN, IKEv2 com PSK ou em branco) e preencha o que o
administrador do gateway passou: gateway, id do grupo, redes, DNS. O editor
diz o que falta e mostra o selo de segurança enquanto você digita; o lápis ao
lado de um perfil o edita depois.

Em **Perfis**, clique no botão de exportar (a seta para baixo) ao lado do
perfil e salve o arquivo. Ele **não tem** senha, usuário nem PSK.

Mande o arquivo, o instalador e o link deste guia. A PSK do grupo vai **por
outro canal**, nunca junto com o arquivo nem em grupo de chat aberto.

> Prefere o terminal? Existe o `ipsec-client-cli`, com os mesmos comandos:
> veja [docs/cli.md](../cli.md) (em inglês).
