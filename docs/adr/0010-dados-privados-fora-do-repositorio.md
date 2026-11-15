# ADR-0010: Nenhum dado real de VPN no repositório

- **Status:** Aceito
- **Data:** 2026-11-15

## Contexto

O projeto nasceu para acessar a VPN do trabalho do mantenedor e vai ser
publicado como código aberto. O repositório não pode conter nada que
identifique a organização ou o mantenedor:

- gateway;
- id do grupo;
- DNS e domínios internos;
- redes internas;
- nomes de usuário e caminhos locais;
- credenciais, obviamente.

Esses dados precisam ser informados pelo próprio usuário, no app.

## Decisão

1. **Exemplos e testes usam só valores reservados para documentação:**
   - IPs da RFC 5737 (`192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`);
   - domínios da RFC 2606 (`example.com`, `*.test`).
   As credenciais de teste são obviamente falsas.
2. **Dados reais ficam fora do git:**
   - `presets.local/` está no `.gitignore`;
   - material privado do mantenedor fica listado em `.git/info/exclude`, que é
     local e nunca versionado.
3. **Guarda automático** (`scripts/check-private.sh`):
   - lê padrões privados de `.git/info/private-patterns`, que também fica fora
     do git;
   - bloqueia commits cujo conteúdo staged ou cuja mensagem case com algum
     padrão;
   - antes de publicar, `--history` varre o histórico inteiro;
   - a saída mostra só a localização, nunca o trecho, para não vazar em logs.
4. **Na CI**, um scanner de segredos genérico (gitleaks) complementa o guarda,
   porque a CI não tem os padrões privados.

## Consequências

- Cada desenvolvedor configura os próprios padrões privados (CONTRIBUTING.md).
- Testes ponta a ponta com a VPN real acontecem só localmente, com o perfil em
  `presets.local/` ou no diretório de configuração do usuário.
- O teste automatizado do engine usa um gateway IPsec local de mentira
  (`engine/tests/`), que serve para qualquer pessoa e para a CI.

## Alternativas consideradas

- **Repositório privado com os dados reais.** Impede a publicação futura e
  deixa os dados no histórico para sempre.
- **Só disciplina manual.** Um único commit descuidado já vaza os dados.
