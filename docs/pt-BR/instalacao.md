# Manual de instalação

Português · **[English](../installation.md)**

Como instalar o IPsec Client como um aplicativo normal: ele aparece no menu
de aplicativos, com ícone, e abre como qualquer outro programa. Você **não**
precisa de Docker, Rust, Node nem terminal para usar.

- [1. Qual arquivo baixar](#1-qual-arquivo-baixar)
- [2. Instalar](#2-instalar)
- [3. Primeiro uso](#3-primeiro-uso)
- [4. Atualizar](#4-atualizar)
- [5. Desinstalar](#5-desinstalar)
- [6. Gerar o pacote a partir do código](#6-gerar-o-pacote-a-partir-do-código)
- [Problemas comuns](#problemas-comuns)

---

## 1. Qual arquivo baixar

| Sua distribuição | Arquivo | Exemplo |
|---|---|---|
| Fedora, openSUSE, RHEL/Rocky/AlmaLinux 10 | **.rpm** | `ipsec-client-0.7.0-1.x86_64.rpm` |
| Ubuntu, Debian, Linux Mint, Pop!_OS | **.deb** | `ipsec-client_0.7.0_amd64.deb` |
| Outras (Arch, etc.) | **.AppImage** | `ipsec-client_0.7.0_amd64.AppImage` |

Os arquivos ficam na página [Releases](https://github.com/lucastelso/ipsec-client/releases) do repositório no GitHub,
junto com o `SHA256SUMS`, que serve para conferir o download
(`sha256sum -c SHA256SUMS --ignore-missing`). Enquanto o repositório for
privado, só quem tem acesso a ele consegue baixar: peça acesso, ou receba os
arquivos de quem te passou o perfil.

> Prefira o **.rpm** ou o **.deb** quando houver para a sua distribuição: o
> app entra no menu sozinho, as atualizações passam pelo gerenciador de
> pacotes e as bibliotecas dele (como o WebKitGTK) recebem as atualizações de
> segurança da sua distribuição. O AppImage leva cópias próprias.

### Distribuições testadas

Os pacotes são instalados e o motor da VPN é testado, de forma automática, em:

| Distribuição | Pacote | DNS da VPN (nomes internos) |
|---|---|---|
| Ubuntu 22.04 e 24.04 | `.deb` | funciona sem ajuste |
| Debian 12 e 13 | `.deb` | precisa do `systemd-resolved` (veja abaixo) |
| Fedora 45 | `.rpm` | funciona sem ajuste |
| openSUSE Tumbleweed | `.rpm` | precisa do `systemd-resolved` (veja abaixo) |
| AlmaLinux 10 (também Rocky Linux e RHEL 10) | `.rpm`, com EPEL | confira em **Sistema → Diagnóstico** |

Requisitos em qualquer distribuição: **systemd** (248 ou mais novo),
**polkit**, glibc 2.35 ou mais nova, WebKitGTK 4.1 e x86 de 64 bits. Sem
systemd o app não funciona. **RHEL, Rocky e AlmaLinux 9 não são
suportados**: não têm o WebKitGTK 4.1 nem uma glibc nova o bastante, então
nem o `.rpm` nem o AppImage rodam neles.

**DNS da VPN.** Os nomes internos da VPN (`servidor.empresa.example`) são
resolvidos pelo **systemd-resolved**. O Ubuntu e o Fedora o usam por padrão.
O Debian e o openSUSE não: neles o túnel funciona, por endereço IP, e
**Sistema → Diagnóstico** avisa. Para ter os nomes, instale e ative-o (isso
muda quem resolve nomes no sistema inteiro, não só na VPN):

```bash
sudo apt install systemd-resolved                        # Debian
sudo zypper install systemd-resolved && sudo systemctl enable --now systemd-resolved   # openSUSE
sudo dnf install systemd-resolved && sudo systemctl enable --now systemd-resolved      # AlmaLinux, Rocky, RHEL
```

## 2. Instalar

### Fedora e afins (.rpm)

Dê **dois cliques** no arquivo: abre a loja de aplicativos (GNOME Software ou
Discover). Clique em **Instalar**. Se não abrir, use o terminal.

Ou pelo terminal, na pasta onde está o arquivo:

```bash
sudo dnf install ./ipsec-client-*.x86_64.rpm
```

No **openSUSE**: `sudo zypper install --allow-unsigned-rpm ./ipsec-client-*.x86_64.rpm`
(o pacote ainda não é assinado; confira o download com o `SHA256SUMS`).

No **AlmaLinux e no Rocky Linux 10**, o WebKitGTK e a biblioteca da bandeja
vêm do EPEL. Ative-o uma vez e depois instale:

```bash
sudo dnf install epel-release
sudo dnf config-manager --set-enabled crb
sudo dnf install ./ipsec-client-*.x86_64.rpm
```

No próprio RHEL 10, ative o EPEL e o CodeReady Builder como a documentação do
EPEL indica para o RHEL e instale do mesmo jeito.

No **Fedora Silverblue, Kinoite e outros sistemas baseados em imagem**:
`rpm-ostree install ./ipsec-client-*.x86_64.rpm` e reinicie. O componente de
rede trata o que é diferente ali (`/usr` somente leitura, `/opt` como link
para `/var/opt`); isso foi testado numa simulação, ainda não numa instalação
de verdade, então avise se algo der errado.

### Ubuntu, Debian e afins (.deb)

Em muitos sistemas, **dois cliques** no arquivo abrem o instalador. Se não
abrir (a loja de algumas versões do Ubuntu não instala arquivos `.deb`), use
o terminal, na pasta onde está o arquivo:

```bash
sudo apt install ./ipsec-client_*_amd64.deb
```

### Outras distribuições (.AppImage)

O AppImage é um arquivo único: não instala nada no sistema.

1. Crie uma pasta para ele, mova o arquivo para lá e dê permissão de
   execução:

   ```bash
   mkdir -p ~/Aplicativos
   mv ~/Downloads/ipsec-client_*_amd64.AppImage ~/Aplicativos/ipsec-client.AppImage
   chmod +x ~/Aplicativos/ipsec-client.AppImage
   ```

2. **Para aparecer no menu**, há dois jeitos:
   - **Mais fácil:** instale o [Gear Lever](https://flathub.org/apps/it.mijorus.gearlever)
     (loja de aplicativos ou Flathub), abra o AppImage com ele e use a opção
     de adicioná-lo ao menu de aplicativos.
   - **À mão:**

     ```bash
     cd ~/Aplicativos
     ./ipsec-client.AppImage --appimage-extract 'usr/share/icons/hicolor/128x128/apps/ipsec-client.png'
     install -Dm644 squashfs-root/usr/share/icons/hicolor/128x128/apps/ipsec-client.png \
       ~/.local/share/icons/hicolor/128x128/apps/ipsec-client.png
     rm -rf squashfs-root
     cat > ~/.local/share/applications/ipsec-client.desktop <<EOF
     [Desktop Entry]
     Type=Application
     Name=IPsec Client
     Comment=Cliente de VPN IPsec para Linux
     Exec=$HOME/Aplicativos/ipsec-client.AppImage
     Icon=ipsec-client
     Terminal=false
     Categories=Network;
     StartupWMClass=ipsec-client
     EOF
     ```

> O AppImage precisa da biblioteca FUSE 2. Se ele não abrir, instale-a:
> Fedora: `sudo dnf install fuse-libs`; Ubuntu 22.04: `sudo apt install libfuse2`;
> Ubuntu 24.04 ou mais novo: `sudo apt install libfuse2t64`.

## 3. Primeiro uso

1. Abra o **IPsec Client** pelo menu de aplicativos (procure por "VPN" ou
   "IPsec"). Ele fala o idioma do seu sistema (português ou inglês); para
   trocar, vá em **Sistema → Aparência → Idioma**.
2. **Instale o componente de rede (só uma vez).** A tela principal mostra
   *"Falta um passo: instalar o componente de rede"*. Clique em **Instalar
   agora** e digite a **senha de administrador** quando o sistema pedir.
   - É a parte que cria o túnel da VPN no sistema. Depois dela, conectar
     **não pede mais senha**, para nenhum usuário deste computador.
   - O mesmo botão fica em **Sistema → Componente de rede**.
   - Precisa de uma conta de administrador (que usa `sudo`) só neste passo.
3. **Importe o perfil da VPN:** vá em **Perfis → Importar** e escolha o
   arquivo `.json` que você recebeu, ou **arraste o arquivo** para a janela.
   Confira a prévia e clique em **Importar**.
4. **Conecte:** em **Conexão**, escolha o perfil e clique em **Conectar**.
   Informe a **PSK do grupo**, o seu **usuário** e a sua **senha**. Marque
   **Lembrar no chaveiro do sistema** para não digitar de novo.
5. Quando aparecer **Conectado**, a VPN está ativa. Abra o Remmina, o
   navegador ou o que você usa normalmente.

Enquanto o app está aberto, o ícone na **bandeja** (perto do relógio) mostra
o estado e tem Conectar/Desconectar. **Fechar a janela fecha o app, mas não
desconecta a VPN.** Ao abrir de novo, ele mostra a conexão ativa. Para
desconectar, use o botão **Desconectar**.

No GNOME, a bandeja só aparece com a extensão **AppIndicator**: no Fedora,
`sudo dnf install gnome-shell-extension-appindicator`; depois saia e entre na
sessão e ative a extensão no app **Extensões**.

## 4. Atualizar

1. Instale o arquivo novo por cima do antigo, do mesmo jeito da seção 2.
   - Fedora: `sudo dnf install ./ipsec-client-<nova versão>.x86_64.rpm`
   - Ubuntu: `sudo apt install ./ipsec-client_<nova versão>_amd64.deb`
   - AppImage: substitua o arquivo em `~/Aplicativos` (mantenha o nome
     `ipsec-client.AppImage`).
2. Abra o app. Se aparecer **"Atualização do componente de rede"**, clique em
   **Atualizar agora** e digite a senha de administrador. Algumas atualizações
   (como a 0.2.0) só valem depois deste passo.

Seus perfis e credenciais continuam onde estavam.

## 5. Desinstalar

1. **No app, com a VPN desconectada:** **Sistema → Componente de rede →
   Remover** (pede a senha de administrador).
2. Remova o app:
   - Fedora: `sudo dnf remove ipsec-client`
   - Ubuntu: `sudo apt remove ipsec-client`
   - AppImage: apague o arquivo e, se criou à mão, o atalho:
     `rm ~/.local/share/applications/ipsec-client.desktop`
3. **Opcional, para não deixar rastro:**
   - perfis: `rm -r ~/.config/ipsec-client`;
   - credenciais guardadas: antes de remover o app, exclua cada perfil em
     **Perfis** marcando **Apagar também as credenciais guardadas**.

Se você removeu o app antes do componente, remova o componente pelo terminal:

```bash
sudo /opt/ipsec-client/libexec/install-native.sh uninstall
```

## 6. Gerar o pacote a partir do código

Para quem tem o código e quer gerar o `.rpm`/`.deb` para testar uma
mudança. Os pacotes de uma versão, para publicar, são gerados de outro jeito:
veja [como gerar e publicar uma versão](../release.md) (em inglês).

1. Instale os pré-requisitos de desenvolvimento do
   [README](../../README.pt-BR.md#pré-requisitos), uma vez. O Docker é usado só para
   compilar o strongSwan embutido, na primeira vez.
2. Na pasta do repositório:

   ```bash
   npm ci
   npm run bundle -- --bundles rpm    # Fedora; no Ubuntu/Debian: --bundles deb
   ```

   Leva alguns minutos na primeira vez. O pacote fica em
   `target/release/bundle/rpm/` (ou `.../deb/`).
3. Instale como na seção 2:

   ```bash
   sudo dnf install ./target/release/bundle/rpm/ipsec-client-*.x86_64.rpm
   ```

   Gerou de novo **com a mesma versão**? Use `sudo dnf reinstall` no lugar de
   `install`.

> Gere o `.deb` num Ubuntu ou Debian: o pacote declara as bibliotecas da
> distribuição onde foi gerado (a da bandeja muda de nome entre as
> distribuições). Um pacote gerado no seu computador contém caminhos da sua
> home: não o distribua.

## Problemas comuns

| Sintoma | O que fazer |
|---|---|
| O app não aparece no menu | Espere alguns segundos ou saia e entre na sessão. No AppImage, confira o atalho em `~/.local/share/applications/` |
| O ícone da bandeja não aparece (GNOME) | Instale e ative a extensão AppIndicator (seção 3) |
| Os nomes internos da VPN não resolvem, mas os endereços IP funcionam | O `systemd-resolved` não está ativo (Debian, openSUSE): veja "DNS da VPN" na seção 1 |
| "O pkexec não foi encontrado" | Instale o `pkexec`: pacote `pkexec` no Debian, no Ubuntu e no openSUSE, `polkit` no Fedora. O `.rpm` e o `.deb` já o trazem como dependência |
| "Autorização cancelada" | A janela de senha foi fechada. Clique em instalar de novo |
| "Não foi possível obter a autorização de administrador" | A sua conta não é de administrador, ou a senha estava errada. Peça para alguém com `sudo` instalar o componente uma vez: depois, qualquer usuário conecta |
| "O computador ainda não está pronto para conectar" | Clique em **Instalar componente de rede** no próprio aviso, ou veja **Sistema → Diagnóstico** |
| "O componente de rede instalado é de uma versão anterior" | Clique em **Atualizar componente de rede** no aviso, ou em **Sistema → Componente de rede → Atualizar** |
| "Portas UDP 500/4500 em uso" | Outra VPN IPsec está ligada; desligue-a antes |
| "Usuário ou senha XAuth incorretos" | Clique em **Informar credenciais** no aviso e digite de novo |
| "O gateway recusou a autenticação da fase 1" | A PSK do grupo está errada: **Informar credenciais** e digite de novo |
| "O gateway não respondeu" | Confira a internet. Algumas redes (hotel, celular) bloqueiam VPN |

**Sistema → Diagnóstico** mostra o que está faltando, com a dica de como
resolver. Se precisar pedir ajuda, mande o que aparece ali e o log da conexão
copiado com **Copiar anonimizado**: ele não tem senha nem PSK, e o gateway,
os IPs e os domínios viram marcadores.
