# Como gerar e publicar uma versão

Guia do mantenedor para gerar os pacotes (AppImage, deb e rpm) e publicá-los
no **GitHub Releases**. Para instalar o app, veja o
[manual de instalação](instalacao.md).

## 1. Preparar a versão

1. Atualize a versão em todos os lugares:

   ```bash
   sed -i '0,/^version = ".*"/s//version = "X.Y.Z"/' Cargo.toml          # workspace
   sed -i 's/ipsec-client-proto = { version = "[^"]*"/ipsec-client-proto = { version = "X.Y.Z"/' crates/core/Cargo.toml
   sed -i 's/^  "version": ".*",/  "version": "X.Y.Z",/' src-tauri/tauri.conf.json
   npm version X.Y.Z --no-git-tag-version                                # package.json e lock
   cargo update -w
   ```

2. No `CHANGELOG.md`, crie a seção `## [X.Y.Z] - AAAA-MM-DD`.
3. Escreva as notas da versão em `docs/releases/vX.Y.Z.md`: é o texto da
   página da release. Use a anterior como modelo.
4. `npm run check` verde, merge `--no-ff` em `main`.

## 2. Criar a tag

No `main`, já com o merge:

```bash
git tag -a vX.Y.Z -m "IPsec Client X.Y.Z"
```

## 3. Gerar os pacotes

```bash
npm run release:build -- vX.Y.Z      # ou: scripts/build-release.sh vX.Y.Z
```

- Roda num container **Ubuntu 22.04** (`packaging/release/Dockerfile`).
  Assim, o AppImage funciona em distribuições com glibc 2.35 ou mais nova, e
  o `.deb` declara as bibliotecas com os nomes do Ubuntu/Debian.
- O código vem **da tag** (`git archive`). O que não está commitado fica de
  fora, e nenhum caminho do seu computador entra nos binários.
- Precisa de Docker. A primeira vez leva mais tempo, porque monta a imagem e
  compila tudo.

No fim, o build roda `scripts/verify-release.sh`, que **recusa** os pacotes
se:

- o componente de rede dentro de algum deles não bater com o próprio
  `SHA256SUMS`. O app recusaria instalar, e isso já aconteceu: o linuxdeploy
  reescreve executáveis dentro do AppImage;
- o AppImage exigir uma glibc mais nova que a do Ubuntu 22.04;
- o strongSwan embutido não tiver os patches de segurança do commit
  (ADR-0019).

Depois, ainda no seu computador, roda `scripts/check-private.sh --artifacts`.
Ele extrai os três pacotes, inclusive o strongSwan e o helper compactados
dentro deles, e procura neles:

- os seus padrões privados (`.git/info/private-patterns`);
- o caminho da sua home;
- o nome do computador.

Mostra só o arquivo, nunca o trecho. Nas bibliotecas do Ubuntu que o AppImage
traz, procura só a home e o hostname: padrões curtos casam por acaso dentro
delas (as mesmas ocorrências existem nas bibliotecas originais).

A saída fica em `target/release-artifacts/X.Y.Z/`:

| Arquivo | Para |
|---|---|
| `ipsec-client_X.Y.Z_amd64.AppImage` | Qualquer distribuição |
| `ipsec-client_X.Y.Z_amd64.deb` | Ubuntu, Debian, Mint |
| `ipsec-client-X.Y.Z-1.x86_64.rpm` | Fedora, RHEL, openSUSE |
| `SHA256SUMS` | Conferir os downloads |

## 4. Conferir antes de publicar

A conferência automática já rodou no build. Para repeti-la, e para olhar os
metadados:

```bash
npm run release:verify -- target/release-artifacts/X.Y.Z
scripts/check-private.sh --artifacts target/release-artifacts/X.Y.Z
cd target/release-artifacts/X.Y.Z
sha256sum -c SHA256SUMS
rpm -qip ipsec-client-*.rpm | grep -E '^(Name|Version)'
dpkg-deb -f ipsec-client_*.deb Package Version Depends 2>/dev/null || \
  (ar p ipsec-client_*.deb control.tar.zst | tar --zstd -xO ./control)
```

E instale o pacote da sua distribuição por cima do atual (seção 4 do
[manual](instalacao.md#4-atualizar)): abra pelo menu, atualize o componente
se o app pedir e conecte.

## 5. Publicar no GitHub

Primeiro, envie o código e a tag:

```bash
git push origin main
git push origin vX.Y.Z
```

### Pelo site

1. Na página do repositório, clique em **Releases** (coluna da direita) e
   depois em **Draft a new release**.
2. Em **Choose a tag**, escolha `vX.Y.Z`.
3. **Release title:** `IPsec Client X.Y.Z`.
4. **Descrição:** cole o conteúdo de `docs/releases/vX.Y.Z.md`.
5. Arraste os quatro arquivos de `target/release-artifacts/X.Y.Z/` para
   **Attach binaries by dropping them here**.
6. Enquanto o projeto for pré-alfa, marque **Set as a pre-release**.
7. Clique em **Publish release**. **Save draft** guarda sem publicar, para
   revisar antes.

### Pelo terminal (GitHub CLI)

Com o [GitHub CLI](https://cli.github.com/) (`sudo dnf install gh` e
`gh auth login`, uma vez):

```bash
gh release create vX.Y.Z target/release-artifacts/X.Y.Z/* \
  --title "IPsec Client X.Y.Z" \
  --notes-file docs/releases/vX.Y.Z.md \
  --prerelease
```

Acrescente `--draft` para revisar no site antes de publicar.

### Quem consegue baixar

- **Repositório privado:** só quem tem acesso ao repositório vê a release.
  Para um colega baixar, adicione-o em **Settings → Collaborators → Add
  people**, ou mande os arquivos direto para ele.
- **Repositório público:** qualquer pessoa vê.

### Assinar os checksums (opcional)

Se você tem uma chave GPG, assine o `SHA256SUMS` e anexe também o `.asc`:

```bash
gpg --armor --detach-sign target/release-artifacts/X.Y.Z/SHA256SUMS
```

Quem baixa confere com `gpg --verify SHA256SUMS.asc SHA256SUMS`, depois de
importar a sua chave pública.

## 6. Depois de publicar

- Atualize o `docs/retomar.md`, o roadmap e o diário.
- **Antes de tornar o repositório público,** o plano prevê reescrever o
  e-mail dos commits (`git filter-repo --mailbox`). Isso muda os hashes:
  recrie as tags no commit novo (`git tag -fa vX.Y.Z ...`) e envie com
  `git push --force origin vX.Y.Z`. A release continua ligada ao **nome** da
  tag, com os mesmos arquivos.
