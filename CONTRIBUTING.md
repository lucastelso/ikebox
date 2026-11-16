# Contribuindo

## Regra nº 1: nenhum dado real de VPN no repositório

Este projeto não pode conter informação que identifique uma organização ou uma
pessoa (ADR-0010). Isso inclui:

- gateway e redes internas;
- id de grupo;
- servidores DNS e domínios internos;
- matrícula ou usuário;
- caminhos locais (`/home/<você>`);
- e, claro, PSKs e senhas.

**Em exemplos, testes e documentação, use só valores reservados:**

| Tipo | Use |
|---|---|
| IPv4 | `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24` (RFC 5737) |
| IPv6 | `2001:db8::/32` (RFC 3849) |
| Domínios | `example.com`, `example.org`, `*.test`, `*.invalid` (RFC 2606) |
| Credenciais de teste | Obviamente falsas, por exemplo `ipsec-client-test-psk` |

### Configure o guarda de dados privados

Crie o arquivo `.git/info/private-patterns`. Ele fica dentro de `.git/` e nunca
é versionado. Escreva **uma expressão regular por linha**. Exemplo:

```
# meus dados privados (nunca versionados)
vpn\.minhaempresa\.com\.br
203\.0\.113\.10
\bminha-matricula\b
```

Os hooks rodam `scripts/check-private.sh` automaticamente:

- no **pre-commit**, sobre o conteúdo staged;
- no **commit-msg**, sobre a mensagem;
- no **pre-push**, sobre o histórico inteiro.

> Não coloque PSKs ou senhas nesse arquivo. Isso criaria mais uma cópia delas
> em texto puro. A proteção para segredos é arquitetural: eles nunca são
> escritos em arquivos do projeto.

## Licença das contribuições

O projeto é GPL-3.0-or-later (ADR-0017). Ao contribuir, você concorda que a
sua contribuição é licenciada nos mesmos termos (*inbound = outbound*).
Dependências novas precisam ter licença compatível, e o `cargo deny` verifica
isso no lado Rust.

## Fluxo de trabalho

1. Crie o branch a partir de `main`, com o nome `tipo/descricao`. Exemplos:
   `feat/perfis`, `fix/updown-ipv6`.
2. Faça commits pequenos no padrão
   [Conventional Commits](https://www.conventionalcommits.org/), em pt-BR:
   ```
   feat(engine): cria interface XFRM dedicada no entrypoint
   fix(updown): ignora rota quando o TS remoto é o próprio gateway
   docs(adr): registra decisão sobre roteamento por if_id
   ```
   Tipos aceitos: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `ci`,
   `build`, `perf`, `style`, `revert`. O commitlint valida a mensagem.
3. Antes do merge, o `npm run check` precisa estar verde.
4. Faça o merge em `main` com `git merge --no-ff`. O branch fica guardado.
5. Versões seguem o [SemVer](https://semver.org/). O CHANGELOG é gerado a partir
   dos commits (git-cliff).

## Verificações

| Comando | O que faz |
|---|---|
| `npm run check` | Tudo: guarda de dados privados, shellcheck e, conforme os componentes forem entrando, fmt/clippy/test/deny (Rust) e biome/tsc/vitest (frontend) |
| `npm run lint:sh` | shellcheck em todos os scripts. Se o shellcheck não estiver instalado, usa a imagem Docker oficial |
| `npm run test:engine` | Teste ponta a ponta do engine com um gateway IPsec local. Precisa de Docker e das portas UDP 500/4500 livres |
| `scripts/check-private.sh --history` | Varre todo o histórico. Rode antes de publicar |
| `scripts/check-private.sh --artifacts <dir>` | Varre os pacotes de release por dentro (padrões privados, caminho da home, hostname). Roda sozinho no fim do `npm run release:build` |

## Estilo

- **Shell:** `bash` com `set -euo pipefail`, aspas em toda expansão e
  shellcheck limpo. Valide toda entrada externa (variáveis de ambiente,
  argumentos).
- **Rust:** `cargo fmt`, `clippy -D warnings`, erros com `thiserror` no `core`.
  Nunca derive `Debug` em tipos com segredo sem redação.
- **Idioma:** documentação e mensagens para o usuário em pt-BR; identificadores
  e comentários de código em inglês.
