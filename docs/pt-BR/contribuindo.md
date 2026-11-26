# Contribuindo

Português · **[English](../../CONTRIBUTING.md)**

Obrigado por ajudar. Issues e pull requests podem ser escritos em português
ou em inglês. Todo mundo aqui segue o [código de conduta](codigo-de-conduta.md);
para dúvidas, veja o [suporte](suporte.md), e relate problemas de segurança
em particular ([segurança](seguranca.md)), nunca numa issue pública.

## Regra nº 1: nenhum dado real de VPN no repositório

Este projeto não pode conter informação que identifique uma organização ou
uma pessoa (ADR-0010). Isso inclui:

- gateway e redes internas;
- id do grupo;
- servidores e domínios DNS internos;
- matrícula ou nome de usuário;
- caminhos locais (`/home/<você>`);
- e, claro, PSKs e senhas.

**Em exemplos, testes e documentação, use só valores reservados:**

| Tipo | Use |
|---|---|
| IPv4 | `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24` (RFC 5737) |
| IPv6 | `2001:db8::/32` (RFC 3849) |
| Domínios | `example.com`, `example.org`, `*.test`, `*.invalid` (RFC 2606) |
| Credenciais de teste | Obviamente falsas, por exemplo `ikebox-test-psk` |

### Configure a guarda de dados privados

Crie o arquivo `.git/info/private-patterns`. Ele fica dentro de `.git/` e
nunca é versionado. Escreva **uma expressão regular por linha**. Exemplo:

```
# meus dados privados (nunca versionados)
vpn\.minhaempresa\.example
203\.0\.113\.10
\bminha-matricula\b
```

Os hooks rodam o `scripts/check-private.sh` sozinhos:

- no **pre-commit**, sobre o conteúdo preparado (staged);
- no **commit-msg**, sobre a mensagem;
- no **pre-push**, sobre todo o histórico.

> Não coloque PSKs nem senhas nesse arquivo. Seria mais uma cópia delas em
> texto puro. A proteção dos segredos é de arquitetura: eles nunca são
> gravados em arquivos do projeto.

## Licença das contribuições

O projeto é GPL-3.0-or-later (ADR-0017). Ao contribuir, você concorda que a
sua contribuição fica sob os mesmos termos (*inbound = outbound*).
Dependências novas precisam de uma licença compatível, e o `cargo deny`
confere isso do lado do Rust.

## Fluxo de trabalho

1. Crie a branch a partir da `main`, com o nome `tipo/descricao`. Exemplos:
   `feat/profiles`, `fix/updown-ipv6`.
2. Faça commits pequenos seguindo o
   [Conventional Commits](https://www.conventionalcommits.org/), em inglês:
   ```
   feat(engine): create a dedicated XFRM interface in the entrypoint
   fix(updown): skip the route when the remote TS is the gateway itself
   docs(adr): record the decision on routing by if_id
   ```
   Tipos aceitos: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `ci`,
   `build`, `perf`, `style`, `revert`. O commitlint confere a mensagem. (Os
   commits anteriores à 0.2.0 estão em português.)
3. Antes do merge, o `npm run check` precisa estar verde.
4. Faça o merge na `main` com `git merge --no-ff` e então **apague a
   branch**, local e remota (`git branch -d <branch>`,
   `git push origin --delete <branch>`). O commit de merge guarda o nome da
   branch e os commits dela no histórico; no repositório ficam só a `main` e
   as branches ainda em andamento.
5. As versões seguem o [SemVer](https://semver.org/lang/pt-BR/).

## Ferramentas das verificações

Para compilar, basta a lista "Para compilar" do README. As verificações e os
testes precisam também de:

- **cargo-deny** (`cargo install --locked cargo-deny`): licenças e alertas de
  segurança das dependências Rust. O `npm run check` para sem ele.
- **shellcheck 0.11.0**, opcional: sem ele, ou com outra versão, o
  `npm run lint:sh` roda a imagem Docker oficial dessa versão (os códigos
  mudam entre versões).
- **Docker**, para os testes de ponta a ponta: eles sobem um gateway IPsec de
  teste e o motor em contêineres.

## Verificações

| Comando | O que faz |
|---|---|
| `npm run check` | Tudo: guarda de dados privados, shellcheck, fmt/clippy/test/deny (Rust) e biome/tsc/vitest (frontend) |
| `npm run lint:sh` | shellcheck em todos os scripts. Sem o shellcheck instalado, usa a imagem Docker oficial |
| `npm run test:engine` | Teste de ponta a ponta do engine com um gateway IPsec local. Precisa do Docker e das portas UDP 500/4500 livres |
| `npm run test:native` / `test:systemd` | O engine embutido de ponta a ponta, e com o systemd de verdade num container (inclui o ataque do ADR-0019) |
| `npm run test:modes` | Todos os modos de IPsec que o perfil aceita (IKEv1 principal/agressivo, PSK com e sem XAuth, IKEv2 + PSK; NAT-T, PFS, VIP, túnel dividido e completo), no computador do `test:systemd`, um gateway de teste por modo (`tests/modes/gateways`) |
| `npm run test:compat` | O `test:systemd` em todas as distribuições de `tests/compat/distros.txt`, com o helper e o CLI compilados na imagem de release. `tests/compat/packages.sh <dir>` instala os pacotes gerados nas mesmas distribuições |
| `scripts/check-private.sh --history` | Varre todo o histórico. Rode antes de publicar |
| `scripts/check-private.sh --artifacts <dir>` | Varre o conteúdo dos pacotes de release (padrões privados, caminho da home, nome da máquina). Roda sozinho no fim do `npm run release:build` |

## Estilo

- **Shell:** `bash` com `set -euo pipefail`, aspas em toda expansão e
  shellcheck limpo. Valide toda entrada externa (variáveis de ambiente,
  argumentos). Não use pipe para `grep -q` quando o resultado importa: com
  `pipefail`, a saída antecipada vira SIGPIPE no produtor. Use
  `grep -q padrão < <(comando)`.
- **Rust:** `cargo fmt`, `clippy -D warnings`, erros com `thiserror` no
  `core`. Nunca derive `Debug` em tipos com segredos sem redação.
- **Idioma:** documentação, identificadores e comentários do código em
  inglês. As mensagens para o usuário são bilíngues: no Rust,
  `tr!("English", "Português")` / `trf!(...)` de `ikebox_core::i18n`; na
  interface, os dicionários em `src/i18n` (o `pt` precisa ter a mesma forma
  do `en`). O CLI, o helper e os scripts falam inglês. A documentação para o
  usuário tem uma cópia em português em `docs/pt-BR/`.
