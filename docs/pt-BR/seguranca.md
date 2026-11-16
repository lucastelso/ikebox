# Segurança

Português · **[English](../../SECURITY.md)**

## Reportando vulnerabilidades

Quando o repositório for público, use o recurso **Security Advisories** do
GitHub ("Report a vulnerability"). Não abra issue pública para
vulnerabilidades.

## Modelo de ameaças (resumo)

### O que o app protege

| Ativo | Proteção |
|---|---|
| PSK, usuário e senha XAuth | Ficam em memória (`secrecy`/`zeroize`) ou no chaveiro do sistema. Vão do app direto ao charon pelo socket VICI; o componente de rede (root) nunca as recebe. Nunca vão para disco, env, argumentos ou logs (ADR-0007; os ADRs estão em inglês, em [docs/adr](../adr/README.md)) |
| Socket de controle VICI | Socket Unix `0600`, dono = uid da sessão, em `/run/ipsec-client/<uid>` (`0711`, do root). Só o dono da sessão fala com o charon (ADRs 0003, 0014 e 0019) |
| Comandos executados como root | Quem fala VICI **não escolhe** o `updown` nem o `if_id`: o charon embutido tem um patch que os força para os valores definidos pelo root, e os argumentos do `updown` vão num arquivo, nunca por um shell (ADR-0019) |
| Componente de rede (root) | API mínima por socket: aceita só números, o id do perfil e o DNS, validados, nunca caminhos, comandos ou segredos. Cada pedido é autorizado pelo polkit (`allow_active`: só a sessão local ativa) e o uid vem do kernel (`SO_PEERCRED`). Parar a VPN e ler o log só para o dono da sessão; conexões atendidas em paralelo, com limite (ADRs 0014 e 0019) |
| Host durante a conexão | O charon roda como serviço transitório do systemd com sandbox: sistema somente leitura, `/proc/sys` e `/sys` somente leitura, sem dispositivos reais, sem ver outros processos, filtro de syscalls, só as capabilities de rede e sem novos privilégios (ADRs 0014 e 0019) |
| Instalação do componente | O instalador confere o `SHA256SUMS` numa cópia só do root e recusa links e caminhos fora do lugar; a remoção só executa o instalador que pertence ao root (ADR-0018) |
| Rede do host | Rotas, VIP e DNS só na interface dedicada `ipsc0`, sem flush global de XFRM e sem tocar na interface física (ADR-0004) |
| Dados da organização do usuário | Nunca entram no repositório; há um guarda automático, que também varre os pacotes de release por dentro (ADR-0010) |
| Logs compartilhados | "Copiar anonimizado" troca gateway, IPs e domínios por marcadores antes de o texto sair do app |

### Riscos conhecidos e aceitos

1. **O componente de rede roda como root.** É o mínimo para criar o túnel
   (XFRM, rotas, portas 500/4500). O código com root fica num binário pequeno,
   testado, que só sobe o strongSwan com sandbox. Depois de instalado,
   qualquer usuário na sessão local ativa conecta sem senha, como no
   NetworkManager. O charon ainda roda como root dentro do sandbox. O próximo
   passo é rodá-lo com um usuário próprio e só as capabilities de rede
   (ADR-0019).
2. **O strongSwan é compilado por nós, com um patch nosso.** O código-fonte
   é verificado por SHA-256 e pela assinatura GPG dos autores (ADR-0016). O
   único patch, pequeno e de segurança (ADR-0019), é aplicado depois dessa
   conferência. As correções de
   segurança chegam quando o projeto publica uma versão nova, e não pelo
   gerenciador de pacotes da distribuição. O daemon fica exposto (UDP
   500/4500) **só enquanto a VPN está conectada**.
3. **Criptografia legada.** IKEv1 agressivo com PSK expõe o hash da PSK a quem
   observar o handshake, que fica sujeito a ataque offline de dicionário.
   modp768/modp1024 e SHA1 também são fracos. Isso é uma limitação **da VPN
   legada**, não do app: o app só consegue usar o que o gateway aceita.
   Recomende à equipe de TI da sua organização migrar para IKEv2 com
   certificados ou EAP.
4. **AppImage.** O arquivo baixado é a base da confiança. Um programa
   malicioso rodando como o próprio usuário poderia trocar os arquivos durante
   a instalação do componente (ADR-0018). Além disso, o AppImage leva
   bibliotecas próprias (como o WebKitGTK), que não recebem as atualizações de
   segurança da distribuição. Prefira o `.rpm` ou o `.deb`.
5. **Engine em Docker (só desenvolvimento e testes).** Sem o componente de
   rede, o app ainda usa o engine em container, se houver Docker. Pertencer ao
   grupo `docker` equivale a ter root, e esse engine usa o strongSwan 5.8.2 do
   Ubuntu 20.04, que já saiu do suporte padrão. O modo privilegiado de reserva
   (`--privileged`) vem desligado. O usuário final não precisa do Docker.

### Fora do escopo

- Proteger os dados do usuário de um host já comprometido (root local) ou de
  um programa malicioso rodando como o próprio usuário. **Em escopo:** o
  componente de rede não pode servir de degrau desse programa para o root
  (ADR-0019).
- Kill switch / bloqueio de vazamento fora do túnel. O app faz split tunnel por
  padrão.

## Boas práticas no desenvolvimento

- Nada de dados reais de VPN no repositório. Use as RFCs 5737/2606 nos
  exemplos e configure `.git/info/private-patterns` (ver [CONTRIBUTING.md](../../CONTRIBUTING.md), em inglês).
- Dependências fixadas: Rust via `Cargo.lock` + `cargo deny`, npm via
  `package-lock.json`, imagens base por digest, código-fonte do strongSwan
  embutido por SHA-256 + GPG, pacotes do strongSwan do engine Docker por
  versão exata.
- A CI roda um scanner de segredos (gitleaks) em todo push.
