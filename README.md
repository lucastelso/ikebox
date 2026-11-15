# IPsec Client

Cliente de VPN IPsec para **Linux**, feito para VPNs **legadas**: IKEv1, modo
agressivo, PSK de grupo + XAuth, grupos DH antigos. É o tipo de VPN corporativa
que os clientes atuais do Linux já não conseguem conectar.

> **Status: pré-alfa, em desenvolvimento ativo.** Ainda não há versão
> instalável. O que já existe e o que vem a seguir estão no
> [roadmap](docs/roadmap.md). Os passos já dados estão no
> [diário de desenvolvimento](docs/desenvolvimento.md).

---

## Por que este projeto existe

Muitas empresas e órgãos públicos ainda usam concentradores VPN (por exemplo,
FortiGate) configurados com IKEv1 e algoritmos antigos. No Windows e no macOS,
o fabricante oferece um cliente oficial, como o FortiClient. No Linux, o
suporte foi abandonado. E as ferramentas nativas não ajudam:

- as distribuições atuais, com suas políticas de criptografia, recusam ou já
  não oferecem esses algoritmos;
- o NetworkManager não cobre bem a combinação **PSK de grupo + XAuth em modo
  agressivo**.

A solução que funcionou na prática foi isolar um **strongSwan 5.8.2**
(Ubuntu 20.04) num container Docker que compartilha a rede do host:

- só o daemon IKE, que negocia o túnel, roda no container;
- a criptografia dos pacotes (ESP) continua no kernel do host;
- nada no sistema precisa ser rebaixado.

O IPsec Client transforma esse fluxo manual de scripts e `docker run` num
aplicativo com perfis, credenciais guardadas com segurança e diagnóstico.

## Para quem é

- **Usuários de Linux** que precisam acessar uma VPN IPsec corporativa sem
  cliente oficial para Linux.
- **Equipes** que querem distribuir um perfil pronto para os colegas, com
  gateway, propostas, rotas e DNS. Cada pessoa só informa as próprias
  credenciais.
- **Quem automatiza:** existe uma linha de comando (`ipsec-client-cli`) com a
  mesma lógica da interface gráfica.

### Para quem não é

| Caso | Use |
|---|---|
| VPN SSL do FortiGate (SSL-VPN) | [openfortivpn](https://github.com/adrienverge/openfortivpn) |
| L2TP/IPsec, OpenVPN, WireGuard | Fora do escopo: há bons clientes nativos |
| Windows / macOS | O cliente oficial do fabricante |

## Como funciona

```
┌──────────────────────────────── host Linux ─────────────────────────────────┐
│  IPsec Client (GUI)  ou  ipsec-client-cli                                   │
│     │ perfis: ~/.config/ipsec-client        credenciais: chaveiro do sistema │
│     │ Docker API                      │ VICI (socket Unix 0660, só seu)     │
│     ▼                                 ▼                                     │
│  ┌──── container "engine": Ubuntu 20.04 + strongSwan 5.8.2 ──────────────┐  │
│  │  charon (IKE)  ──  updown.sh → rotas + DNS (resolvectl via D-Bus)     │  │
│  └──────────── --network host · só NET_ADMIN · recriado a cada conexão ──┘  │
│     │ SAs e políticas XFRM (com if_id)                                      │
│     ▼                                                                       │
│  kernel: interface ipsc0 (XFRM, MTU 1400) ═══ ESP/UDP 4500 ═══► gateway VPN │
└─────────────────────────────────────────────────────────────────────────────┘
```

- **Conectar:** o app cria o container e entrega o perfil e as credenciais ao
  strongSwan, em memória, pelo socket de controle (VICI). Depois inicia a
  negociação.
- **Conectado:**
  - o IP virtual, as rotas e o DNS da VPN ficam na interface dedicada `ipsc0`;
  - a sua interface Wi-Fi/Ethernet não é tocada;
  - outras VPNs (WireGuard, Tailscale) não são afetadas.
- **Desconectar:** o container é removido, e a interface `ipsc0` some levando
  junto rotas, IP e DNS.

Os detalhes estão em [docs/arquitetura.md](docs/arquitetura.md). O porquê de
cada escolha está nos [ADRs](docs/adr/README.md).

## Segurança e privacidade

- **Credenciais** (PSK, usuário e senha XAuth):
  - não vão para disco, variáveis de ambiente, `docker inspect` nem logs;
  - "lembrar credenciais" usa o chaveiro do sistema (GNOME Keyring/KWallet).
- **Container:** recebe só `NET_ADMIN`, sem `--privileged`. O strongSwan só
  escuta nas portas UDP 500/4500 **enquanto a VPN está conectada**.
- **Repositório:** não contém dados de nenhuma VPN real. Perfis com dados da
  sua organização ficam em `presets.local/`, que é ignorado pelo git, ou são
  criados e importados pelo próprio app.
- **Riscos conhecidos:**
  - pertencer ao grupo `docker` equivale a ter root;
  - o strongSwan 5.8.2 é antigo;
  - VPNs legadas usam criptografia fraca por natureza.

  Tudo isso está explicado em [SECURITY.md](SECURITY.md).

## Requisitos (uso)

- Linux x86_64 com kernel ≥ 4.19 e o módulo `xfrm_interface` (padrão nas
  distribuições atuais).
- `systemd-resolved` ativo, para o DNS split.
- Docker, com o seu usuário no grupo `docker`.
- Secret Service, que é opcional e serve para lembrar credenciais.

## Desenvolvimento

### Pré-requisitos

Fedora:

```bash
sudo dnf install -y rustup webkit2gtk4.1-devel openssl-devel curl wget file \
  libappindicator-gtk3-devel librsvg2-devel libxdo-devel shellcheck
sudo dnf group install -y c-development
rustup-init -y --component rustfmt,clippy
```

Debian/Ubuntu:

```bash
sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev shellcheck
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Também é preciso Node.js ≥ 22, Docker e o cargo-deny
(`cargo install --locked cargo-deny`).

### Comandos

```bash
npm install                 # instala as ferramentas e os git hooks (lefthook)
npm run check               # tudo o que precisa estar verde antes de um merge
npm run test:engine         # teste ponta a ponta do engine com um gateway IPsec local
scripts/check-private.sh    # procura dados privados no conteúdo staged
```

O teste do engine (`tests/engine/e2e.sh`) faz o seguinte:

1. sobe um **gateway IPsec de mentira** num container;
2. conecta o engine nele usando IKEv1 agressivo, PSK + XAuth e
   modp1024/modp768, a mesma família das VPNs legadas;
3. verifica a interface, as rotas, o DNS split, um ping pelo túnel e se os
   logs e o `docker inspect` estão sem segredos;
4. desconecta e confere se tudo foi limpo;
5. mata o engine com `SIGKILL` e confere que o próximo engine limpa os restos
   e reconecta;
6. testa o túnel completo (`0.0.0.0/0`) num namespace de rede isolado, sem
   mexer na rede do host.

Ele não usa nenhuma VPN real e leva cerca de 10 s.

### Estrutura do repositório

```
engine/          imagem do engine: Dockerfile, entrypoint, updown, config do strongSwan
tests/engine/    teste ponta a ponta do engine com gateway IPsec local
scripts/         utilitários de desenvolvimento (guarda de dados privados, shellcheck)
docs/            arquitetura, roadmap, diário de desenvolvimento, ADRs
crates/core/     núcleo em Rust: perfis, segredos, engine, sessão, diagnóstico (em construção)
crates/cli/      ipsec-client-cli (em construção)
src-tauri/ src/  (em breve) app desktop Tauri v2 + React
presets/         (em breve) exemplos genéricos de perfis
```

### Fluxo de trabalho

- Um branch por funcionalidade.
- Commits no padrão [Conventional Commits](https://www.conventionalcommits.org/)
  em pt-BR.
- Merge `--no-ff` em `main` com o `npm run check` verde.

Os detalhes estão em [CONTRIBUTING.md](CONTRIBUTING.md).

## Documentação

- **[Estado atual e como retomar](docs/retomar.md)**
- [Arquitetura](docs/arquitetura.md)
- [Decisões de arquitetura (ADRs)](docs/adr/README.md)
- [Roadmap](docs/roadmap.md)
- [Diário de desenvolvimento](docs/desenvolvimento.md)
- [Segurança](SECURITY.md) · [Contribuindo](CONTRIBUTING.md) · [Changelog](CHANGELOG.md)

## Publicação no GitHub (mantenedor)

O repositório começa privado. Para enviá-lo:

```bash
scripts/check-private.sh --history          # varre o histórico inteiro antes
git remote add origin git@github.com:<usuario>/ipsec-client.git
git push -u origin --all && git push --tags
```

## Licença

A definir antes de o repositório se tornar público.
