#!/usr/bin/env bash
# Teste do engine embutido de verdade (sem Docker no caminho da VPN), para o
# mantenedor rodar no próprio computador:
#
#   1. compila o strongSwan embutido e o helper (como usuário);
#   2. instala o componente de rede (PEDE SENHA: sudo);
#   3. roda o E2E completo do CLI forçando o engine embutido
#      (IPSEC_CLIENT_BACKEND=native), contra o gateway de teste local.
#
# O passo 3 também testa o polkit de verdade: rode num terminal da sua sessão
# gráfica (sessão local ativa). Para desinstalar depois:
#   sudo packaging/install-native.sh uninstall

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo"
if ! command -v cargo >/dev/null 2>&1 && [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck source=/dev/null
    . "$HOME/.cargo/env"
fi

version=$(grep -vE '^\s*(#|$)' native-engine/versions.txt | head -n 1 | awk '{print $1}')
tarball="native-engine/dist/ipsec-client-engine-$version-x86_64.tar.gz"

echo "==> 1/3 compilando o strongSwan $version e o helper"
native-engine/build.sh "$version"
cargo build --release -p ipsec-client-helper

echo "==> 2/3 instalando o componente de rede (sudo)"
sudo packaging/install-native.sh install --engine "$tarball" \
    --helper target/release/ipsec-client-helper --repo "$repo"

echo "==> 3/3 E2E do CLI com o engine embutido"
IPSEC_CLIENT_BACKEND=native tests/cli/e2e.sh
echo
echo "✔ engine embutido validado. Ele fica instalado e passa a ser o padrão do app."
echo "  Para voltar ao Docker: sudo packaging/install-native.sh uninstall"
