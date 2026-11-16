# Segurança

## Reportando vulnerabilidades

Quando o repositório for público, use o recurso **Security Advisories** do
GitHub ("Report a vulnerability"). Não abra issue pública para
vulnerabilidades.

## Modelo de ameaças (resumo)

### O que o app protege

| Ativo | Proteção |
|---|---|
| PSK, usuário e senha XAuth | Ficam em memória (`secrecy`/`zeroize`) ou no chaveiro do sistema. Vão do app direto ao charon pelo socket VICI; o componente de rede (root) nunca as recebe. Nunca vão para disco, env, argumentos ou logs (ADR-0007) |
| Socket de controle VICI | Socket Unix `0660`, dono root e grupo do usuário, em `/run/ipsec-client/<uid>` (`0750`). Só o dono da sessão fala com o charon (ADRs 0003 e 0014) |
| Componente de rede (root) | API mínima por socket: aceita só números validados e o id do perfil, nunca caminhos, comandos ou segredos. Cada pedido é autorizado pelo polkit (`allow_active`: só a sessão local ativa) e o uid vem do kernel (`SO_PEERCRED`) (ADR-0014) |
| Host durante a conexão | O charon roda como serviço transitório do systemd com sandbox: sistema somente leitura, só as capabilities de rede, sem novos privilégios (ADR-0014) |
| Instalação do componente | O instalador confere o `SHA256SUMS` numa cópia só do root e recusa links e caminhos fora do lugar; a remoção só executa o instalador que pertence ao root (ADR-0018) |
| Rede do host | Rotas, VIP e DNS só na interface dedicada `ipsc0`, sem flush global de XFRM e sem tocar na interface física (ADR-0004) |
| Dados da organização do usuário | Nunca entram no repositório; há um guarda automático (ADR-0010) |

### Riscos conhecidos e aceitos

1. **O componente de rede roda como root.** É o mínimo para criar o túnel
   (XFRM, rotas, portas 500/4500). O código com root fica num binário pequeno,
   testado, que só sobe o strongSwan com sandbox. Depois de instalado,
   qualquer usuário na sessão local ativa conecta sem senha, como no
   NetworkManager.
2. **O strongSwan é compilado por nós.** O código-fonte é verificado por
   SHA-256 e pela assinatura GPG dos autores (ADR-0016). As correções de
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
   a instalação do componente (ADR-0018). Quem quiser a verificação do
   gerenciador de pacotes deve usar o `.rpm` ou o `.deb`.
5. **Engine em Docker (só desenvolvimento e testes).** Sem o componente de
   rede, o app ainda usa o engine em container, se houver Docker. Pertencer ao
   grupo `docker` equivale a ter root, e esse engine usa o strongSwan 5.8.2 do
   Ubuntu 20.04, que já saiu do suporte padrão. O modo privilegiado de reserva
   (`--privileged`) vem desligado. O usuário final não precisa do Docker.

### Fora do escopo

- Proteger o usuário de um host já comprometido (root local) ou de um
  programa malicioso rodando como o próprio usuário.
- Kill switch / bloqueio de vazamento fora do túnel. O app faz split tunnel por
  padrão.

## Boas práticas no desenvolvimento

- Nada de dados reais de VPN no repositório. Use as RFCs 5737/2606 nos
  exemplos e configure `.git/info/private-patterns` (ver CONTRIBUTING.md).
- Dependências fixadas: Rust via `Cargo.lock` + `cargo deny`, npm via
  `package-lock.json`, imagens base por digest, código-fonte do strongSwan
  embutido por SHA-256 + GPG, pacotes do strongSwan do engine Docker por
  versão exata.
- A CI roda um scanner de segredos (gitleaks) em todo push.
