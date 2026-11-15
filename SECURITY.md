# Segurança

## Reportando vulnerabilidades

Quando o repositório for público, use o recurso **Security Advisories** do
GitHub ("Report a vulnerability"). Não abra issue pública para
vulnerabilidades.

## Modelo de ameaças (resumo)

### O que o app protege

| Ativo | Proteção |
|---|---|
| PSK, usuário e senha XAuth | Ficam em memória (`secrecy`/`zeroize`) ou no chaveiro do sistema. Chegam ao charon pelo socket VICI. Nunca vão para disco, env, argumentos, `docker inspect` ou logs (ADR-0007) |
| Socket de controle VICI | Socket Unix `0660`, `root:<gid do usuário>`, num diretório `0700` dentro de `$XDG_RUNTIME_DIR` (ADR-0003) |
| Rede do host | Rotas, VIP e DNS só na interface dedicada `ipsc0`, sem flush global de XFRM e sem tocar na interface física (ADR-0004) |
| Dados da organização do usuário | Nunca entram no repositório; há um guarda automático (ADR-0010) |

### Riscos conhecidos e aceitos

1. **Grupo `docker` equivale a root.** Quem consegue falar com o daemon Docker
   consegue obter root no host. O app exige essa permissão porque o engine
   roda em container. Isso só deve ser feito em máquinas pessoais e de
   confiança.
2. **strongSwan 5.8.2 é antigo.**
   - O Ubuntu aplica correções de segurança como backport
     (`5.8.2-1ubuntu3.x`), mas o 20.04 saiu do suporte padrão, e correções
     novas podem não chegar ao arquivo público.
   - O daemon fica exposto (UDP 500/4500) **só enquanto a VPN está
     conectada**, porque o container é removido ao desconectar (ADR-0002).
   - O container tem só `NET_ADMIN` (ADR-0005).
3. **Criptografia legada.** IKEv1 agressivo com PSK expõe o hash da PSK a quem
   observar o handshake, que fica sujeito a ataque offline de dicionário.
   modp768/modp1024 e SHA1 também são fracos. Isso é uma limitação **da VPN
   legada**, não do app: o app só consegue usar o que o gateway aceita.
   Recomende à equipe de TI da sua organização migrar para IKEv2 com
   certificados ou EAP.
4. **Modo privilegiado de reserva.** Quando ativado manualmente, o container
   roda com `--privileged`. Ele vem desligado por padrão.

### Fora do escopo

- Proteger o usuário de um host já comprometido (root local).
- Kill switch / bloqueio de vazamento fora do túnel. O app faz split tunnel por
  padrão.

## Boas práticas no desenvolvimento

- Nada de dados reais de VPN no repositório. Use as RFCs 5737/2606 nos
  exemplos e configure `.git/info/private-patterns` (ver CONTRIBUTING.md).
- Dependências fixadas: Rust via `Cargo.lock` + `cargo deny`, npm via
  `package-lock.json`, imagem base por digest, pacotes do strongSwan por
  versão exata.
- A CI roda um scanner de segredos (gitleaks) em todo push.
