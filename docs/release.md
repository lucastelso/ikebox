# How to build and publish a release

Maintainer guide. Releases are built, signed and attested by CI from a
version tag, and published by the maintainer after checking them on their own
computer ([ADR-0030](adr/0030-release-signing-and-provenance.md)). To install
the app, see the [installation guide](installation.md).

## Signing key (once)

An Ed25519 primary key for certification only, kept off-line, and an Ed25519
signing subkey, the only part GitHub ever gets. Use a name only (no personal
e-mail):

```bash
gpg --quick-gen-key "Ikebox releases" ed25519 cert 5y
FPR=$(gpg --list-keys --with-colons "Ikebox releases" | awk -F: '/^fpr/ {print $10; exit}')
gpg --quick-add-key "$FPR" ed25519 sign 2y
SUB=$(gpg --list-keys --with-colons "$FPR" | awk -F: '/^fpr/ {n++; if (n == 2) print $10}')
echo "primary $FPR, signing subkey $SUB"

# The public key goes into the repository (commit it).
gpg --armor --export "$FPR" >packaging/keys/ikebox-release.asc

# The signing subkey alone ("!"), for GitHub; delete the file afterwards.
gpg --armor --export-secret-subkeys "$SUB!" >"$XDG_RUNTIME_DIR/release-subkey.asc"

# A revocation certificate and a backup of the primary key, off-line (a USB
# drive in a drawer): with them, a leaked subkey can be revoked and replaced.
gpg --armor --gen-revoke "$FPR" >revocation.asc
gpg --armor --export-secret-keys "$FPR" >primary-backup.asc
```

Put the primary fingerprint (`$FPR`) in the README (both languages) and in
every release's notes, in place of `<FINGERPRINT>`, so a swapped key would
show. The release workflow refuses to draft a release while `<FINGERPRINT>`
is still there.

## GitHub setup (once)

1. **Settings → Environments → New environment** `release`:
   - **Required reviewers:** yourself;
   - **Deployment branches and tags:** selected: tags `v*`, and `main` (for
     rehearsals);
   - **Environment secrets:** `RELEASE_GPG_KEY` (the content of
     `release-subkey.asc`) and `RELEASE_GPG_PASSPHRASE` (its passphrase);
   - **Environment variables:** `RELEASE_GPG_FINGERPRINT` = `$SUB`.

   Then `shred -u "$XDG_RUNTIME_DIR/release-subkey.asc"`.
2. **Settings → Actions → General:** workflow permissions "Read repository
   contents" (the release job asks for more only where it needs it).
3. A rehearsal: **Actions → Release → Run workflow** on `main`. It builds,
   waits for your approval, signs and checks the signatures, and publishes
   nothing (the files are in the run's artifacts).

## 1. Prepare the version

1. Set the version, with the real numbers (for example `1.0.1`):

   ```bash
   scripts/set-version.sh 1.0.1
   ```

   The version also appears in `README.md`, `README.pt-BR.md` and the file
   names of `docs/installation.md` and `docs/pt-BR/instalacao.md`.
2. In `CHANGELOG.md`, add the `## [X.Y.Z] - YYYY-MM-DD` section. Add the
   same version to the AppStream metadata
   (`src-tauri/linux/io.github.lucastelso.ikebox.metainfo.xml`, a
   `<release>` at the top of `<releases>`); `verify-release.sh` refuses
   packages without it.
3. Write the release notes in `docs/releases/vX.Y.Z.md` (English) and
   `docs/releases/vX.Y.Z.pt-BR.md` (Portuguese): they become the release
   page. Use the previous ones as a template, with the key fingerprint and
   how to check the download.
4. `npm run check` green, `--no-ff` merge into `main`, `git push origin main`.

## 2. Tag: CI builds, you approve the signature

```bash
git tag -a vX.Y.Z -m "Ikebox X.Y.Z"
git push origin vX.Y.Z           # by name: never --tags, --all or --mirror
```

**Actions → Release** then:

1. **build:** the packages in the Ubuntu 22.04 image
   (`scripts/build-release.sh`: the network component against its own
   `SHA256SUMS`, the security patches of the engine, the glibc limit, one
   version everywhere, and a scan for the runner's home path and hostname),
   the reproducibility check of the network component, and the engine's
   corresponding source;
2. **sign:** waits for your approval (**Review deployments**); signs the
   `.rpm` and `SHA256SUMS` and checks both with the public key;
3. **publish:** attestations (public repository) and a **draft** release.

## 3. Check the draft on your computer

Your private patterns never go to CI (ADR-0010): the scan for them runs here.

```bash
dir=target/release-check/X.Y.Z && mkdir -p "$dir"
gh release download vX.Y.Z --dir "$dir"
scripts/check-private.sh --artifacts "$dir"
(cd "$dir" && gpg --verify SHA256SUMS.asc SHA256SUMS && sha256sum -c SHA256SUMS)
gh attestation verify "$dir"/ikebox-X.Y.Z-1.x86_64.rpm --repo lucastelso/ikebox
```

Then compare with a build of yours: `npm run release:build -- vX.Y.Z`. The
network component must be the same (the `SHA256SUMS` inside
`/usr/lib/ikebox/network` of both `.rpm`); the packages themselves carry
build dates and may differ.

Finally, install the package for your distribution over the current one,
open it from the menu and connect.

## 4. Publish

On the draft release page: check the text, then **Publish release** (not a
pre-release from 1.0 on). Or: `gh release edit vX.Y.Z --draft=false`.

## Building by hand (without CI)

The same build runs on any computer with Docker, which is how the releases
before 1.0 were made and how the draft is compared:

```bash
npm run release:build -- vX.Y.Z      # or: scripts/build-release.sh vX.Y.Z
scripts/release-sources.sh X.Y.Z     # the engine's corresponding source
```

- It runs in an **Ubuntu 22.04** container (`packaging/release/Dockerfile`,
  every download pinned by SHA-256), so the AppImage works on distributions
  with glibc 2.35 or newer and the `.deb` declares Ubuntu/Debian library
  names.
- The code comes **from the tag** (`git archive`): what is not committed stays
  out, and no path of your computer gets into the binaries.
- If the engine in `native-engine/dist/` lacks the patches of the commit, it
  is rebuilt first.
- At the end it runs `scripts/verify-release.sh` and then
  `scripts/check-private.sh --artifacts`, which extracts the packages
  (including the compressed strongSwan and helper) and looks for your private
  patterns (`.git/info/private-patterns`), your home path and the hostname,
  showing only the file, never the match. In executables only readable
  strings of 6 characters or more count.

Output in `target/release-artifacts/X.Y.Z/` (from a tag) or
`X.Y.Z-dev-<commit>/` (anything else, so a test build never replaces a
release):

| File | For |
|---|---|
| `ikebox_X.Y.Z_amd64.AppImage` | Any distribution |
| `ikebox_X.Y.Z_amd64.deb` | Ubuntu, Debian, Mint |
| `ikebox-X.Y.Z-1.x86_64.rpm` | Fedora, openSUSE, RHEL 10 |
| `SHA256SUMS` | Checking the downloads |

Signing by hand, with the signing key on this computer:
`RELEASE_GPG_FINGERPRINT=<subkey> scripts/sign-release.sh <dir>` (the
sources archive must be in the directory too).

The compatibility matrix runs weekly in CI (`weekly.yml`); by hand:

```bash
tests/compat/packages.sh target/release-artifacts/X.Y.Z --from target/release-artifacts/<previous>
npm run test:compat
```

## After publishing

- Update `docs/resume.md`, the roadmap and the development log.
- If the history is ever rewritten (as for 0.2.0, to replace the commit
  email), the hashes change: recreate the tags on the new commits and push
  each one **by name** (`git push --force origin vX.Y.Z`). Never use
  `--tags`, `--all` or `--mirror`: they would also push any stale local tag
  that points to the old history (this happened with `v0.1.1`, which a fetch
  brought back). A release stays bound to the tag **name**, with the same
  files.
