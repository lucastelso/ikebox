#!/usr/bin/env bash
# Monta o pacote do componente de rede: o diretório que vai dentro do app
# (recurso do Tauri, ADR-0018) e que o instalador usa com --bundle.
#
# Uso:
#   scripts/stage-network-component.sh [--out <dir>] [--engine <tarball>] [--helper <binário>]
#
# Sem --engine/--helper, compila o que faltar (strongSwan embutido via Docker,
# helper com cargo). Saída padrão: target/network-component/
#
# Conteúdo (caminhos relativos ao pacote):
#   VERSION                         version=<app> e strongswan=<versão>
#   SHA256SUMS                      hash de todos os outros arquivos
#   install-native.sh               instalador (roda como root)
#   engine.tar.gz                   strongSwan compilado
#   ipsec-client-helper.gz          helper com privilégio, compactado: o
#                                   linuxdeploy (AppImage) reescreve o RPATH
#                                   de todo ELF solto no pacote, e o hash
#                                   deixaria de bater
#   engine/{entrypoint.sh,updown.sh,strongswan.d/zz-ipsec-client.conf}
#   packaging/systemd/ipsec-client-helper.{socket,service}
#   packaging/polkit/io.github.lucastelso.ipsecclient.policy

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
readonly POLICY=io.github.lucastelso.ipsecclient.policy

die() {
    printf 'stage-network-component: erro: %s\n' "$*" >&2
    exit 1
}

out="$repo/target/network-component"
engine=""
helper=""
while (($# > 0)); do
    case $1 in
    --out) out=${2:?}; shift 2 ;;
    --engine) engine=${2:?}; shift 2 ;;
    --helper) helper=${2:?}; shift 2 ;;
    *) die "argumento desconhecido: $1" ;;
    esac
done

strongswan=$(grep -vE '^\s*(#|$)' "$repo/native-engine/versions.txt" | head -n 1 | awk '{print $1}')
app_version=$(awk -F'"' '/^\[workspace.package\]/ {s = 1} s && /^version *=/ {print $2; exit}' "$repo/Cargo.toml")
[[ -n $strongswan && -n $app_version ]] || die "não consegui ler as versões"

if [[ -z $engine ]]; then
    engine="$repo/native-engine/dist/ipsec-client-engine-$strongswan-x86_64.tar.gz"
    [[ -f $engine ]] || "$repo/native-engine/build.sh" "$strongswan"
fi
if [[ -z $helper ]]; then
    # O cargo do rustup (respeita o rust-toolchain.toml), mesmo com outro no PATH.
    if [[ -f "$HOME/.cargo/env" ]]; then
        # shellcheck source=/dev/null
        . "$HOME/.cargo/env"
    fi
    (cd "$repo" && cargo build --release --locked -p ipsec-client-helper)
    helper="$repo/target/release/ipsec-client-helper"
fi
[[ -f $engine ]] || die "tarball do engine não encontrado: $engine"
[[ -f $helper ]] || die "helper não encontrado: $helper"

# O engine precisa ter exatamente os patches de segurança deste commit
# (ADR-0019): um tarball velho em native-engine/dist/ deixaria o app vulnerável.
expected_patches=()
for patch_file in "$repo"/native-engine/patches/*.patch; do
    [[ -f $patch_file ]] || continue
    expected_patches+=("$(basename "$patch_file"):$(sha256sum "$patch_file" | cut -c1-64)")
done
engine_patches=$(tar -xzOf "$engine" opt/ipsec-client/engine/BUILDINFO 2>/dev/null |
    sed -n 's/^patches=//p')
[[ $engine_patches == "${expected_patches[*]}" ]] ||
    die "o engine $engine não tem os patches deste commit (native-engine/patches): recompile com native-engine/build.sh"

rm -rf "${out:?}"
install -d -m 0755 "$out" "$out/engine/strongswan.d" "$out/packaging/systemd" "$out/packaging/polkit"
install -m 0644 "$engine" "$out/engine.tar.gz"
gzip -9 -n -c "$helper" >"$out/ipsec-client-helper.gz"
chmod 0644 "$out/ipsec-client-helper.gz"
install -m 0755 "$repo/packaging/install-native.sh" "$out/install-native.sh"
install -m 0755 "$repo/engine/entrypoint.sh" "$repo/engine/updown.sh" "$out/engine/"
install -m 0644 "$repo/engine/strongswan.d/zz-ipsec-client.conf" "$out/engine/strongswan.d/"
install -m 0644 "$repo/packaging/systemd/ipsec-client-helper.socket" \
    "$repo/packaging/systemd/ipsec-client-helper.service" "$out/packaging/systemd/"
install -m 0644 "$repo/packaging/polkit/$POLICY" "$out/packaging/polkit/"
printf 'version=%s\nstrongswan=%s\n' "$app_version" "$strongswan" >"$out/VERSION"

# Ordem fixa: o SHA256SUMS também identifica o pacote (o app compara com o
# instalado para saber se há atualização).
# shellcheck disable=SC2094 # o find exclui o próprio SHA256SUMS
(
    cd "$out"
    find . -type f ! -name SHA256SUMS -printf '%P\n' | LC_ALL=C sort |
        xargs -d '\n' sha256sum >SHA256SUMS
)
echo "pacote do componente de rede em $out (app $app_version, strongSwan $strongswan)"
