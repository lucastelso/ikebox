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
| Componente de rede (root) | API mínima por socket: aceita só números, o id do perfil e o DNS, validados, nunca caminhos, comandos ou segredos. Cada pedido é autorizado pelo polkit (`allow_active`: só a sessão local ativa) e o uid vem do kernel (`SO_PEERCRED`). A única interface que ele cria ou remove é a da sessão (`ipsc<N>`, e só se for uma interface XFRM). Parar a VPN e ler o log só para o dono da sessão, e o log traz só as linhas daquela sessão; conexões atendidas em paralelo, com limite por usuário e prazo (ADRs 0014 e 0019) |
| Host durante a conexão | O charon roda como serviço transitório do systemd com sandbox: sistema somente leitura, `/proc/sys` e `/sys` somente leitura, sem dispositivos reais, sem ver outros processos, filtro de syscalls, só as capabilities de rede e sem novos privilégios (ADRs 0014 e 0019) |
| Instalação do componente | O instalador confere o `SHA256SUMS` numa cópia só do root e recusa links e caminhos fora do lugar. Com o `.rpm`/`.deb`, ele roda a partir do diretório do próprio app, que pertence ao root: nenhum arquivo que o usuário possa alterar é executado como root. A remoção só executa o instalador que pertence ao root (ADR-0018) |
| Rede do host | Rotas, VIP e DNS só na interface dedicada `ipsc0`, sem flush global de XFRM e sem tocar na interface física (ADR-0004) |
| Dados da organização do usuário | Nunca entram no repositório; há um guarda automático, que também varre os pacotes de release por dentro (ADR-0010) |
| Logs compartilhados | As identidades (usuário XAuth, id do grupo, identidade do gateway) são mascaradas onde o charon as imprime, conhecidas ou não pelo app. O "Copiar anonimizado" também troca gateway, IPs e domínios por marcadores antes de o texto sair do app. Caracteres de controle no log (o gateway consegue pôr texto ali) são trocados antes de ele ser mostrado ou impresso |
| Memória de um processo que travou | Sem core dumps: o app, a CLI, o componente de rede e o charon rodam com o limite de core em zero, então um travamento não grava credenciais nem chaves de sessão em disco |

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

   **Uma PSK de grupo não prova quem é o gateway.** Todos no grupo têm a
   mesma PSK, e quem a tem consegue se passar pelo gateway para outro membro:
   receber o usuário e a senha XAuth e enviar rotas e servidores DNS. É assim
   que o IKEv1 com PSK de grupo e XAuth funciona, em qualquer cliente. Trate
   a PSK do grupo como segredo e desconfie de conectar a partir de redes em
   que alguém possa interceptar o tráfego até o gateway. O app avisa nesses
   perfis: o selo de segurança deles é no máximo "legada", e os detalhes do
   perfil explicam o motivo
   ([nível de segurança](../profiles.md#security-level), em inglês).
4. **AppImage.** O arquivo baixado é a base da confiança. Um programa
   malicioso rodando como o próprio usuário poderia trocar os arquivos durante
   a instalação do componente (ADR-0018): o root não lê o AppImage, então o
   instalador roda a partir de uma cópia no diretório de runtime do usuário.
   O `.rpm` e o `.deb` não têm essa janela. Além disso, o AppImage leva
   bibliotecas próprias (como o WebKitGTK), que não recebem as atualizações de
   segurança da distribuição. Prefira o `.rpm` ou o `.deb`.
5. **Engine em Docker (só desenvolvimento e testes).** As versões de
   release do app não têm o engine em Docker. As de desenvolvimento
   (`npm run dev:app`) caem nele quando o componente de rede não está
   instalado: pertencer ao grupo `docker` equivale a ter root, e esse engine
   usa o strongSwan do Debian, com todos os plugins da distribuição. O modo
   privilegiado de reserva (`--privileged`) vem desligado.
6. **O que outros usuários do mesmo computador conseguem ver.** Que há uma
   VPN ativa, o IP virtual, as rotas, os servidores DNS e domínios, e o id do
   perfil: o Linux mostra o estado da rede a todo usuário local. Eles não
   conseguem ler o log da conexão, falar com o strongSwan dela nem
   desconectá-la. O journal do sistema guarda o log sem máscara para os
   administradores (root e os grupos do journal).
7. **O IKEv1 está sendo aposentado pelo strongSwan.** A versão 6.1.0 desativa
   o IKEv1 por padrão (o build embutido o habilita) e anunciou a remoção numa
   versão futura. A maioria das VPNs para as quais o app foi feito é IKEv1
   (o IKEv2 com PSK também é aceito e testado); como manter o IKEv1
   funcionando com correções de segurança é uma decisão em aberto (veja a
   [auditoria da 0.2.0](../audits/v0.2.0.md), em inglês, item P1).
8. **IKEv2 com usuário e senha (EAP).** O gateway sempre prova quem é pelo
   certificado, conferido contra a CA do perfil e o nome do gateway; não há
   como pular isso. Dois limites, decididos para a 0.7.0: **a revogação não
   é verificada** (nem CRL nem OCSP: isso faria o charon, como root, buscar
   URLs na internet), então um certificado de gateway revogado continua
   funcionando até vencer; e o motor verifica **só RSA e Ed25519**, então um
   gateway com certificado ECDSA ainda não pode ser usado
   ([ADR-0023](../adr/0023-ikev2-eap-mschapv2.md), em inglês). O MSCHAPv2 só
   é seguro dentro desse túnel conferido: o app nunca o envia a um gateway
   não verificado.
9. **O seu certificado (certificado de cliente, 0.8.0).** A chave privada é
   a credencial mais sensível que o app guarda: quem a tem é você na VPN até
   o certificado vencer. Ela é lida do `.p12` em memória, guardada só no
   chaveiro do sistema (tão protegida quanto as suas senhas guardadas:
   destravada com a sua sessão) e enviada ao charon como dado, nunca como
   arquivo. A senha do `.p12` não é guardada; o app não precisa mais do
   arquivo, e você pode apagá-lo. Os nomes dele (o seu, o seu e-mail, a
   organização) são mascarados no log. Vale o mesmo limite RSA/Ed25519 do
   gateway; ainda sem smartcards ou tokens (PKCS#11)
   ([ADR-0024](../adr/0024-client-certificates.md), em inglês).

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
