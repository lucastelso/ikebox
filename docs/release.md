# How to build and publish a release

Maintainer guide to build the packages (AppImage, deb and rpm) and publish
them on **GitHub Releases**. To install the app, see the
[installation guide](installation.md).

## 1. Prepare the version

1. Update the version everywhere:

   ```bash
   sed -i '0,/^version = ".*"/s//version = "X.Y.Z"/' Cargo.toml          # workspace
   sed -i 's/ipsec-client-proto = { version = "[^"]*"/ipsec-client-proto = { version = "X.Y.Z"/' crates/core/Cargo.toml
   sed -i 's/^  "version": ".*",/  "version": "X.Y.Z",/' src-tauri/tauri.conf.json
   npm version X.Y.Z --no-git-tag-version                                # package.json and lock
   cargo update -w
   ```

2. In `CHANGELOG.md`, add the `## [X.Y.Z] - YYYY-MM-DD` section.
3. Write the release notes in `docs/releases/vX.Y.Z.md` (English) and
   `docs/releases/vX.Y.Z.pt-BR.md` (Portuguese): they are the text of the
   release page. Use the previous ones as a template.
4. `npm run check` green, `--no-ff` merge into `main`.

## 2. Create the tag

On `main`, with the merge done:

```bash
git tag -a vX.Y.Z -m "IPsec Client X.Y.Z"
```

## 3. Build the packages

```bash
npm run release:build -- vX.Y.Z      # or: scripts/build-release.sh vX.Y.Z
```

- It runs in an **Ubuntu 22.04** container (`packaging/release/Dockerfile`).
  That way, the AppImage works on distributions with glibc 2.35 or newer, and
  the `.deb` declares the libraries with their Ubuntu/Debian names.
- The code comes **from the tag** (`git archive`). Whatever is not committed
  stays out, and no path from your computer gets into the binaries.
- If the engine in `native-engine/dist/` does not have the patches of the
  commit, it is rebuilt first.
- It needs Docker. The first time takes longer, because it builds the image
  and compiles everything.

At the end, the build runs `scripts/verify-release.sh`, which **refuses** the
packages if:

- the network component inside any of them does not match its own
  `SHA256SUMS`. The app would refuse to install it, and that already happened:
  linuxdeploy rewrites executables inside the AppImage;
- the AppImage requires a glibc newer than Ubuntu 22.04's;
- the embedded strongSwan lacks the security patches of the commit
  (ADR-0019).

Then, still on your computer, it runs `scripts/check-private.sh --artifacts`.
It extracts the three packages, including the strongSwan and the helper
compressed inside them, and looks for:

- your private patterns (`.git/info/private-patterns`);
- your home path;
- the computer name.

It shows only the file, never the matched text. In executables, the patterns
only count in readable text (`strings`), because short patterns match machine
code by chance. In the Ubuntu libraries the AppImage carries, it only looks
for the home path and the hostname (the same chance matches exist in the
original libraries).

The output goes to `target/release-artifacts/X.Y.Z/`:

| File | For |
|---|---|
| `ipsec-client_X.Y.Z_amd64.AppImage` | Any distribution |
| `ipsec-client_X.Y.Z_amd64.deb` | Ubuntu, Debian, Mint |
| `ipsec-client-X.Y.Z-1.x86_64.rpm` | Fedora, RHEL, openSUSE |
| `SHA256SUMS` | Checking the downloads |

## 4. Check before publishing

The automatic checks already ran in the build. To repeat them, and to look at
the metadata:

```bash
npm run release:verify -- target/release-artifacts/X.Y.Z
scripts/check-private.sh --artifacts target/release-artifacts/X.Y.Z
cd target/release-artifacts/X.Y.Z
sha256sum -c SHA256SUMS
rpm -qip ipsec-client-*.rpm | grep -E '^(Name|Version)'
```

Then run the **compatibility matrix** on the new packages. It installs the
`.deb`/`.rpm` with each distribution's package manager in a clean container
(Ubuntu 22.04 and 24.04, Debian 12 and 13, Fedora, openSUSE Tumbleweed,
AlmaLinux 10 with EPEL),
updating from the previous version, and fails if a library or a tool the app
needs was not brought by the package:

```bash
tests/compat/packages.sh target/release-artifacts/X.Y.Z --from target/release-artifacts/<previous>
npm run test:compat      # the engine with real systemd and polkit, on the same seven
```

Finally, install the package for your distribution over the current one
(section 4 of the [installation guide](installation.md#4-update)): open it
from the menu, update the component if the app asks and connect.

A build from `HEAD` or from a commit (`npm run release:build`, without the
tag) goes to `target/release-artifacts/X.Y.Z-dev-<commit>/`, so a test build
never replaces the packages of a published version.

## 5. Publish on GitHub

First, push the code and the tag:

```bash
git push origin main
git push origin vX.Y.Z
```

### On the website

1. On the repository page, click **Releases** (right column) and then **Draft
   a new release**.
2. In **Choose a tag**, pick `vX.Y.Z`.
3. **Release title:** `IPsec Client X.Y.Z`.
4. **Description:** paste the content of `docs/releases/vX.Y.Z.md`; the
   Portuguese notes can follow, after a separator.
5. Drag the four files from `target/release-artifacts/X.Y.Z/` onto **Attach
   binaries by dropping them here**.
6. While the project is pre-alpha, check **Set as a pre-release**.
7. Click **Publish release**. **Save draft** keeps it unpublished, to review
   first.

### From the terminal (GitHub CLI)

With the [GitHub CLI](https://cli.github.com/) (`sudo dnf install gh` and
`gh auth login`, once):

```bash
gh release create vX.Y.Z target/release-artifacts/X.Y.Z/* \
  --title "IPsec Client X.Y.Z" \
  --notes-file docs/releases/vX.Y.Z.md \
  --prerelease
```

Add `--draft` to review on the website before publishing.

### Who can download

- **Private repository:** only people with access to the repository see the
  release. For a colleague to download it, add them in **Settings →
  Collaborators → Add people**, or send them the files directly.
- **Public repository:** anyone.

### Signing the checksums (optional)

If you have a GPG key, sign `SHA256SUMS` and attach the `.asc` too:

```bash
gpg --armor --detach-sign target/release-artifacts/X.Y.Z/SHA256SUMS
```

Downloaders check it with `gpg --verify SHA256SUMS.asc SHA256SUMS`, after
importing your public key.

## 6. After publishing

- Update `docs/resume.md`, the roadmap and the development log.
- If the history is ever rewritten (as for 0.2.0, to replace the commit
  email), the hashes change: recreate the tags on the new commits and push
  each one **by name** (`git push --force origin vX.Y.Z`). Never use
  `--tags`, `--all` or `--mirror`: they would also push any stale local tag
  that points to the old history (this happened with `v0.1.1`, which a fetch
  brought back). A release stays bound to the tag **name**, with the same
  files.
