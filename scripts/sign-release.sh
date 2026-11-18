#!/usr/bin/env bash
# Signs a release (ADR-0030): the .rpm (rpmsign, so dnf and zypper can check
# it) and SHA256SUMS (a detached, armored signature: SHA256SUMS.asc), then
# checks both against the public key in the repository.
#
# The signing key must already be in GnuPG (CI imports the signing subkey from
# the protected "release" environment; never a key on a shared machine).
#
#   RELEASE_GPG_FINGERPRINT  fingerprint of the signing (sub)key
#   RELEASE_GPG_PASSFILE     file with its passphrase (optional)
#   RELEASE_PUBLIC_KEY       public key to check with
#                            (default: packaging/keys/ikebox-release.asc)
#
# Usage: scripts/sign-release.sh <dir with the packages and the sources archive>
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
dir=${1:?usage: $0 <dir>}
fpr=${RELEASE_GPG_FINGERPRINT:?set RELEASE_GPG_FINGERPRINT}
passfile=${RELEASE_GPG_PASSFILE:-}
public_key=${RELEASE_PUBLIC_KEY:-$repo/packaging/keys/ikebox-release.asc}

fail() {
    printf '\033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}
ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }

[[ $fpr =~ ^[0-9A-Fa-f]{40}$ ]] || fail "RELEASE_GPG_FINGERPRINT is not a fingerprint"
[[ -f $public_key ]] || fail "public key not found: $public_key"
command -v rpmsign >/dev/null || fail "rpmsign is missing (package rpm)"
shopt -s nullglob
rpms=("$dir"/*.rpm)
((${#rpms[@]} == 1)) || fail "expected one .rpm in $dir"

gpg_args=(--batch --pinentry-mode loopback)
[[ -z $passfile ]] || gpg_args+=(--passphrase-file "$passfile")

# rpmsign through GnuPG without a terminal; the key chosen by fingerprint.
macros=$(mktemp)
trap 'rm -f "$macros"' EXIT
printf '%%__gpg %s\n%%_gpg_name %s\n%%_gpg_sign_cmd_extra_args %s\n' \
    "$(command -v gpg)" "$fpr" "${gpg_args[*]}" >"$macros"
rpmsign --load "$macros" --addsign "${rpms[0]}" >/dev/null
ok "signed ${rpms[0]##*/}"

# The sums come after signing: the signature changes the .rpm.
(
    cd "$dir"
    files=(*.AppImage *.deb *.rpm *-sources.tar)
    sha256sum -- "${files[@]}" >SHA256SUMS
)
rm -f "$dir/SHA256SUMS.asc"
gpg "${gpg_args[@]}" --local-user "$fpr!" --armor --detach-sign \
    --output "$dir/SHA256SUMS.asc" "$dir/SHA256SUMS"
ok "signed SHA256SUMS ($(wc -l <"$dir/SHA256SUMS") files)"

# Checked as a downloader would: against the public key alone.
check=$(mktemp -d)
trap 'rm -f "$macros"; rm -rf "$check"' EXIT
gpg --homedir "$check" --batch --quiet --import "$public_key" 2>/dev/null
gpg --homedir "$check" --batch --verify "$dir/SHA256SUMS.asc" "$dir/SHA256SUMS" 2>/dev/null ||
    fail "SHA256SUMS.asc does not verify with $public_key"
(cd "$dir" && sha256sum --check --quiet SHA256SUMS) || fail "SHA256SUMS does not match"
rpm --dbpath "$check/rpmdb" --initdb
rpmkeys --dbpath "$check/rpmdb" --import "$public_key"
rpmkeys --dbpath "$check/rpmdb" --checksig "${rpms[0]}" | grep -q 'signatures OK\|digests signatures OK' ||
    fail "the .rpm signature does not verify with $public_key"
ok "checked with the public key: SHA256SUMS.asc, the sums and the .rpm signature"
