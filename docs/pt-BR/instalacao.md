# Manual de instalação

Português · **[English](../installation.md)**

Como instalar o Ikebox como um aplicativo normal: ele aparece no menu
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
| Fedora, openSUSE, RHEL/Rocky/AlmaLinux 10 | **.rpm** | `ikebox-1.0.0-1.x86_64.rpm` |
| Ubuntu, Debian, Linux Mint, Pop!_OS | **.deb** | `ikebox_1.0.0_amd64.deb` |
| Outras (Arch, etc.) | **.AppImage** | `ikebox_1.0.0_amd64.AppImage` |

Os arquivos ficam na página [Releases](https://github.com/lucastelso/ikebox/releases) do repositório no GitHub,
junto com o `SHA256SUMS`, que serve para conferir o download
(`sha256sum -c SHA256SUMS --ignore-missing`).

### Conferir o download

A partir da 1.0, toda versão é assinada e atestada
([ADR-0030](../adr/0030-release-signing-and-provenance.md), em inglês).
Baixe o `SHA256SUMS` e o `SHA256SUMS.asc` junto com o seu arquivo e:

```bash
# A chave do projeto (uma vez); compare a impressão digital com a do README.
curl -fsSLO https://raw.githubusercontent.com/lucastelso/ikebox/main/packaging/keys/ikebox-release.asc
gpg --import ikebox-release.asc
gpg --verify SHA256SUMS.asc SHA256SUMS            # "Good signature"
sha256sum -c SHA256SUMS --ignore-missing          # o seu arquivo: OK
```

Com o [GitHub CLI](https://cli.github.com/), o atestado diz qual commit e
qual execução da CI gerou o arquivo:
`gh attestation verify <arquivo> --repo lucastelso/ikebox`.

O `.rpm` também é assinado: depois de `sudo rpm --import
ikebox-release.asc`, o `dnf` e o `zypper` o conferem sozinhos (e
`rpm -K <arquivo>` mostra).

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
sudo dnf install ./ikebox-*.x86_64.rpm
```

No **openSUSE**: `sudo rpm --import ikebox-release.asc` uma vez (veja
[Conferir o download](#conferir-o-download)), depois
`sudo zypper install ./ikebox-*.x86_64.rpm`. Pacotes anteriores à 1.0
não eram assinados e precisam de `--allow-unsigned-rpm`.

No **AlmaLinux e no Rocky Linux 10**, o WebKitGTK e a biblioteca da bandeja
vêm do EPEL. Ative-o uma vez e depois instale:

```bash
sudo dnf install epel-release
sudo dnf config-manager --set-enabled crb
sudo dnf install ./ikebox-*.x86_64.rpm
```

No próprio RHEL 10, ative o EPEL e o CodeReady Builder como a documentação do
EPEL indica para o RHEL e instale do mesmo jeito.

No **Fedora Silverblue, Kinoite e outros sistemas baseados em imagem**:
`rpm-ostree install ./ikebox-*.x86_64.rpm` e reinicie. O componente de
rede trata o que é diferente ali (`/usr` somente leitura, `/opt` como link
para `/var/opt`); isso foi testado numa simulação, ainda não numa instalação
de verdade, então avise se algo der errado.

### Ubuntu, Debian e afins (.deb)

Em muitos sistemas, **dois cliques** no arquivo abrem o instalador. Se não
abrir (a loja de algumas versões do Ubuntu não instala arquivos `.deb`), use
o terminal, na pasta onde está o arquivo:

```bash
sudo apt install ./ikebox_*_amd64.deb
```

### Outras distribuições (.AppImage)

O AppImage é um arquivo único: não instala nada no sistema.

1. Crie uma pasta para ele, mova o arquivo para lá e dê permissão de
   execução:

   ```bash
   mkdir -p ~/Aplicativos
   mv ~/Downloads/ikebox_*_amd64.AppImage ~/Aplicativos/ikebox.AppImage
   chmod +x ~/Aplicativos/ikebox.AppImage
   ```

2. **Para aparecer no menu**, há dois jeitos:
   - **Mais fácil:** instale o [Gear Lever](https://flathub.org/apps/it.mijorus.gearlever)
     (loja de aplicativos ou Flathub), abra o AppImage com ele e use a opção
     de adicioná-lo ao menu de aplicativos.
   - **À mão:**

     ```bash
     cd ~/Aplicativos
     ./ikebox.AppImage --appimage-extract 'usr/share/icons/hicolor/128x128/apps/ikebox.png'
     install -Dm644 squashfs-root/usr/share/icons/hicolor/128x128/apps/ikebox.png \
       ~/.local/share/icons/hicolor/128x128/apps/ikebox.png
     rm -rf squashfs-root
     cat > ~/.local/share/applications/ikebox.desktop <<EOF
     [Desktop Entry]
     Type=Application
     Name=Ikebox
     Comment=Cliente de VPN IPsec para Linux
     Exec=$HOME/Aplicativos/ikebox.AppImage
     Icon=ikebox
     Terminal=false
     Categories=Network;
     StartupWMClass=ikebox
     EOF
     ```

> O AppImage precisa da biblioteca FUSE 2. Se ele não abrir, instale-a:
> Fedora: `sudo dnf install fuse-libs`; Ubuntu 22.04: `sudo apt install libfuse2`;
> Ubuntu 24.04 ou mais novo: `sudo apt install libfuse2t64`.

## 3. Primeiro uso

1. Abra o **Ikebox** pelo menu de aplicativos (procure por "VPN" ou
   "IPsec"). Ele fala o idioma do seu sistema (português ou inglês); para
   trocar, vá em **Sistema → Aparência → Idioma**.
2. **O componente de rede.** Com o `.rpm` ou o `.deb`, o próprio pacote já
   o instalou, e este passo não aparece. Com o AppImage (ou se o pacote não
   conseguiu instalar), a tela principal mostra *"Falta um passo: instalar o
   componente de rede"*: clique em **Instalar agora** e digite a **senha de
   administrador** quando o sistema pedir (só uma vez).
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
o estado e tem Desconectar. **Fechar a janela mantém o app rodando na
bandeja** (na primeira vez, uma notificação avisa); **Sair do app**, no menu
da bandeja, encerra. Nenhum dos dois desconecta a VPN: use o botão
**Desconectar**. Para o botão de fechar encerrar o app: **Sistema →
Comportamento**.

No GNOME, a bandeja só aparece com a extensão **AppIndicator**: no Fedora,
`sudo dnf install gnome-shell-extension-appindicator`; depois saia e entre na
sessão e ative a extensão no app **Extensões**.

## 4. Atualizar

1. Instale o arquivo novo por cima do antigo, do mesmo jeito da seção 2.
   - Fedora: `sudo dnf install ./ikebox-<nova versão>.x86_64.rpm`
   - Ubuntu: `sudo apt install ./ikebox_<nova versão>_amd64.deb`
   - AppImage: substitua o arquivo em `~/Aplicativos` (mantenha o nome
     `ikebox.AppImage`).
2. Com o `.rpm` ou o `.deb`, o pacote atualiza também o componente de rede.
   Com o AppImage, abra o app: se aparecer **"Atualização do componente de
   rede"**, clique em **Atualizar agora** e digite a senha de administrador.

Seus perfis e credenciais continuam onde estavam.

### Vindo do IPsec Client (0.9 ou anterior)

O Ikebox se chamava IPsec Client até a 0.9
([ADR-0031](../adr/0031-renamed-to-ikebox.md)). Instale o Ikebox do jeito de
sempre:

- o `.rpm` e o `.deb` substituem o pacote `ipsec-client` no mesmo passo (o
  `dnf` e o `apt` avisam que ele é removido);
- o componente de rede do IPsec Client é removido quando o Ikebox instala o
  dele;
- na primeira execução, o Ikebox move seus perfis e ajustes de
  `~/.config/ipsec-client` para `~/.config/ikebox`, e as credenciais
  guardadas para as entradas dele no chaveiro (se o chaveiro estiver
  bloqueado, ele pede para desbloquear, ou move numa execução seguinte).

Com o AppImage, apague o `ipsec-client.AppImage` antigo e a entrada de menu
dele (`~/.local/share/applications/ipsec-client.desktop`), se você criou uma.

## 5. Desinstalar

1. Remova o app:
   - Fedora: `sudo dnf remove ikebox`
   - Ubuntu: `sudo apt remove ikebox`

     A partir da 1.0, remover o pacote remove também o componente de rede
     que ele instalou (a VPN, se estiver conectada, é desconectada).
   - AppImage: antes, no app, com a VPN desconectada, **Sistema →
     Componente de rede → Remover** (pede a senha de administrador). Depois
     apague o arquivo e, se criou à mão, o atalho:
     `rm ~/.local/share/applications/ikebox.desktop`
2. **Opcional, para não deixar rastro:**
   - perfis: `rm -r ~/.config/ikebox`;
   - credenciais guardadas: antes de remover o app, exclua cada perfil em
     **Perfis** marcando **Apagar também as credenciais guardadas**.

Se o componente continuar lá (instalado pelo AppImage, ou por um pacote
anterior à 1.0), remova-o pelo terminal:

```bash
sudo /opt/ikebox/libexec/install-native.sh uninstall
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
   sudo dnf install ./target/release/bundle/rpm/ikebox-*.x86_64.rpm
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
