# Guia rápido: conectar na VPN com um perfil recebido

Este guia é para quem **recebeu um arquivo de perfil** (`algo.json`) de um
colega e quer conectar na VPN pelo Linux. Leva uns 15 minutos na primeira vez.

Você vai precisar de três coisas, que **não** vêm no arquivo:

- a **PSK do grupo** (a "chave do grupo"): peça a quem te passou o perfil, por
  um canal seguro;
- o seu **usuário** da VPN (ex.: matrícula);
- a sua **senha** da VPN.

---

## 1. Instalar o que é preciso (uma vez)

**Fedora:**
```bash
sudo dnf install -y docker rustup git dbus-devel gcc
sudo systemctl enable --now docker
sudo usermod -aG docker $USER
rustup-init -y
```

**Ubuntu/Debian:**
```bash
sudo apt install -y docker.io git libdbus-1-dev build-essential curl
sudo systemctl enable --now docker
sudo usermod -aG docker $USER
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

Depois disso, **saia da sessão e entre de novo** (ou reinicie), para valer o
grupo `docker`.

## 2. Baixar e instalar o programa (uma vez)

```bash
git clone https://github.com/lucastelso/ipsec-client.git
cd ipsec-client
. "$HOME/.cargo/env"
cargo build --release -p ipsec-client-cli
install -Dm755 target/release/ipsec-client-cli ~/.local/bin/ipsec-client-cli
```

> Enquanto o repositório for privado, peça acesso a quem te passou o perfil.

Confira se está tudo pronto:
```bash
ipsec-client-cli doctor
```
Tem que terminar com **"✔ Pronto para conectar"**. Se alguma linha mostrar
✘, ela traz a dica de como resolver.

## 3. Importar o perfil (uma vez)

Troque o caminho pelo arquivo que você recebeu:
```bash
ipsec-client-cli profiles import ~/Downloads/trabalho.json
```
Aparece algo como `✔ Perfil 'trabalho' importado`. O nome entre aspas é o
**id do perfil**, que você usa nos próximos comandos.

## 4. Guardar suas credenciais (uma vez)

```bash
ipsec-client-cli secrets set trabalho
```
O programa pergunta:
```
PSK do grupo:                     ← a chave do grupo (não aparece ao digitar)
Usuário XAuth (ex.: matrícula):   ← seu usuário
Senha XAuth:                      ← sua senha (não aparece ao digitar)
```
Fica guardado no chaveiro do sistema. Você não precisa digitar de novo.

## 5. Conectar (todo dia)

```bash
ipsec-client-cli connect trabalho
```
Na primeira vez demora cerca de 1 minuto (ele prepara o container). Quando
aparecer **"✔ Conectado"**, a VPN está ativa: abra o Remmina (ou o que você
usa) normalmente.

**Para desconectar:** `Ctrl+C` nesse terminal.

Se quiser liberar o terminal:
```bash
ipsec-client-cli connect trabalho --detach   # conecta e devolve o terminal
ipsec-client-cli status                      # mostra se está conectado
ipsec-client-cli disconnect                  # desconecta
```

---

## Deu erro?

| Mensagem | O que fazer |
|---|---|
| "usuário ou senha XAuth incorretos" | `ipsec-client-cli secrets set trabalho` e digite de novo |
| "o gateway recusou a autenticação da fase 1" | a PSK do grupo está errada: `secrets set` de novo |
| "o gateway não respondeu" | confira a sua internet; algumas redes (hotel, celular) bloqueiam VPN |
| "Portas UDP 500/4500 em uso" | outra VPN IPsec está ligada; desligue-a antes |
| "o Docker não está acessível" | você saiu e entrou na sessão depois do `usermod`? |
| `cargo: command not found` | rode `. "$HOME/.cargo/env"` (ou abra um terminal novo) |

**Não use `sudo`** com o `ipsec-client-cli`.

Se ainda assim não funcionar, mande a saída de
`ipsec-client-cli doctor` e do `connect`. Elas não mostram senha nem PSK.

---

## Para quem vai passar o perfil adiante

```bash
ipsec-client-cli profiles export trabalho trabalho.json
```
O arquivo **não tem** senha, usuário nem PSK. Mande o arquivo e o link deste
guia. A PSK do grupo vai **por outro canal**, nunca junto com o arquivo nem
em grupo de chat aberto.
