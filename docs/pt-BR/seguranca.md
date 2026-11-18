# Segurança

Português · **[English](../../SECURITY.md)**

## Reportando vulnerabilidades

**Relate em privado:** [Report a vulnerability](https://github.com/lucastelso/ikebox/security/advisories/new)
(o relato privado de vulnerabilidades do GitHub, na aba **Security**). Não
abra issue, discussão nem pull request públicos sobre o assunto.

- Diga o que é afetado (o app, a CLI, o componente de rede, o strongSwan
  embutido ou os patches dele, os pacotes, o processo de release), a versão
  e como reproduzir. Nunca inclua credenciais reais nem o gateway da sua
  organização: use endereços de documentação (`192.0.2.0/24`,
  `example.com`) e o log anonimizado ("Copiar anonimizado"). Pode escrever
  em português.
- **O que esperar:** confirmação em até 7 dias; uma primeira avaliação
  (gravidade para o modelo de ameaças deste app, abaixo) em até 14 dias; e a
  correção, ou um plano com data, combinado com você. O aviso é publicado
  junto com a versão corrigida, com crédito a você, a não ser que prefira o
  contrário; quando cabe, um CVE é pedido pelo GitHub.
- **O próprio strongSwan:** uma falha no código do strongSwan (não nos
  patches nem na configuração deste projeto) vai também para o projeto
  strongSwan, segundo a [política deles](https://www.strongswan.org/security.html);
  relate aqui também quando o motor embutido é afetado, para que um motor
  corrigido saia ([ADR-0026](../adr/0026-strongswan-maintenance-policy.md),
  em inglês).
- **Boa-fé:** pesquisa feita de boa-fé, nos seus próprios sistemas e
  gateways de teste, sem tocar em dados ou VPNs de outras pessoas, é bem-vinda
  e não será perseguida.

### Versões com suporte

Só a **última versão** recebe correções de segurança: atualize o app (com o
`.rpm`/`.deb`, o componente de rede é atualizado junto; com o AppImage, o
app oferece a atualização). Versões anteriores à 1.0 não têm suporte.

| Versão | Correções de segurança |
|---|---|
| última 1.x | ✔ |
| 1.x anteriores | atualize para a última |
| 0.x | ✘ |

## Modelo de ameaças (resumo)

### O que o app protege

| Ativo | Proteção |
|---|---|
| PSK, usuário e senha XAuth | Ficam em memória (`secrecy`/`zeroize`) ou no chaveiro do sistema. Vão do app direto ao charon pelo socket VICI; o componente de rede (root) nunca as recebe. Nunca vão para disco, env, argumentos ou logs (ADR-0007; os ADRs estão em inglês, em [docs/adr](../adr/README.md)) |
| Socket de controle VICI | Socket Unix `0600`, dono = uid da sessão, em `/run/ikebox/<uid>` (`0711`, do root). Só o dono da sessão fala com o charon (ADRs 0003, 0014 e 0019) |
| Comandos executados como root | Quem fala VICI **não escolhe** o `updown` nem o `if_id`: o charon embutido tem um patch que os força para os valores definidos pelo root, e os argumentos do `updown` vão num arquivo, nunca por um shell (ADR-0019) |
| Componente de rede (root) | API mínima por socket: aceita só números, o id do perfil e o DNS, validados, nunca caminhos, comandos ou segredos. Cada pedido é autorizado pelo polkit (`allow_active`: só a sessão local ativa) e o uid vem do kernel (`SO_PEERCRED`). A única interface que ele cria ou remove é a da sessão (`ipsc<N>`, e só se for uma interface XFRM). Parar a VPN e ler o log só para o dono da sessão, e o log traz só as linhas daquela sessão; conexões atendidas em paralelo, com limite por usuário e prazo (ADRs 0014 e 0019) |
| Host durante a conexão | O charon roda **sem root**, como o usuário de sistema `ikebox-engine` e só com `NET_ADMIN` (sem como ganhar outras capabilities), dentro de um serviço transitório do systemd com sandbox: sistema somente leitura, `/proc/sys` e `/sys` somente leitura, sem dispositivos reais, sem ver outros processos, filtro de syscalls e sem novos privilégios. O DNS da VPN é aplicado pela parte root do motor, e só na interface da sessão (ADRs 0014, 0019 e 0028) |
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
   NetworkManager. Desde a 1.0, o charon, a parte que lê o que vem da rede,
   roda com um usuário próprio e só `NET_ADMIN`; um script root pequeno em
   volta dele prepara a interface, aplica o DNS e limpa tudo no fim
   ([ADR-0028](../adr/0028-charon-without-root.md), em inglês).
2. **O strongSwan é compilado por nós, com dois patches nossos.** O
   código-fonte é verificado por SHA-256 e pela assinatura GPG dos autores
   (ADR-0016). Os dois patches são pequenos e de segurança, aplicados depois
   dessa conferência: o cliente VICI não escolhe o `updown` nem o `if_id`
   (ADR-0019), nem faz o charon abrir um arquivo de certificado ou um token
   pelo caminho (ADR-0020). Ele liga o OpenSSL estaticamente para RSA,
   ECDSA e os grupos Diffie-Hellman (ADR-0027); as correções do strongSwan e
   do OpenSSL seguem a política do
   [ADR-0026](../adr/0026-strongswan-maintenance-policy.md), em inglês. As correções de
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
   (o IKEv2 com PSK também é aceito e testado). Quando o strongSwan remover
   o IKEv1, o motor fica na última versão que o tem, e o projeto aplica nela
   as correções de segurança publicadas
   ([ADR-0026](../adr/0026-strongswan-maintenance-policy.md), em inglês).
8. **IKEv2 com usuário e senha (EAP).** O gateway sempre prova quem é pelo
   certificado, conferido contra a CA do perfil e o nome do gateway; não há
   como pular isso. Dois limites, decididos para a 0.7.0: **a revogação não
   é verificada** (nem CRL nem OCSP: isso faria o charon, como root, buscar
   URLs na internet), então um certificado de gateway revogado continua
   funcionando até vencer. Certificados RSA, ECDSA e Ed25519 são conferidos
   ([ADR-0023](../adr/0023-ikev2-eap-mschapv2.md) e
   [ADR-0027](../adr/0027-openssl-in-the-engine.md), em inglês). O MSCHAPv2 só
   é seguro dentro desse túnel conferido: o app nunca o envia a um gateway
   não verificado.
9. **O seu certificado (certificado de cliente, 0.8.0).** A chave privada é
   a credencial mais sensível que o app guarda: quem a tem é você na VPN até
   o certificado vencer. Ela é lida do `.p12` em memória, guardada só no
   chaveiro do sistema (tão protegida quanto as suas senhas guardadas:
   destravada com a sua sessão) e enviada ao charon como dado, nunca como
   arquivo. A senha do `.p12` não é guardada; o app não precisa mais do
   arquivo, e você pode apagá-lo. Os nomes dele (o seu, o seu e-mail, a
   organização) são mascarados no log. Chaves RSA, ECDSA e Ed25519 são
   aceitas ([ADR-0024](../adr/0024-client-certificates.md), em inglês).

   **Com smartcard ou token**, a chave nunca sai do dispositivo: o app (ou a
   CLI) o abre com o módulo PKCS#11, na sua própria sessão, e assina pela
   VPN quando pedido; o charon nunca vê a chave nem o PIN, e o PIN nunca é
   guardado. Enquanto conectado, o charon pode pedir assinaturas, como
   qualquer cliente de VPN que autentica você, e só pelo agente do app, que
   só responde ao charon e a você. Mantenha o app aberto enquanto estiver
   conectado: a renovação das chaves pode precisar de uma nova assinatura
   ([ADR-0029](../adr/0029-smartcards-and-tokens.md), em inglês).

### O que o app envia pela rede

Só a própria VPN (IKE e ESP, para o gateway do perfil) e as consultas de DNS
que o sistema faz. Nada mais, a não ser que **Sistema → Comportamento →
"Avisar quando sair uma versão nova"** esteja ligado (vem desligado): aí,
uma vez por dia, o app pergunta à API do GitHub qual é a última versão
(`api.github.com`, com o nome e a versão do app como user agent; o GitHub vê
o seu endereço IP, como em qualquer visita). Da resposta só se aproveita um
número de versão, a página da versão é montada pelo app, e nada é baixado
nem instalado. Quem empacota o app pode compilá-lo sem essa consulta.

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
  versão exata, e tudo o que o build de release baixa (rustup-init, Node.js,
  ferramentas do AppImage) por SHA-256.
- A CI roda um scanner de segredos (gitleaks) em todo push.
- Os pacotes de release são conferidos antes de publicar: integridade do
  componente de rede, patches de segurança do strongSwan, limite de glibc e
  dados privados (`scripts/verify-release.sh`, `scripts/check-private.sh --artifacts`).
