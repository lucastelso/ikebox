#!/usr/bin/env bash
# Teste do engine embutido de verdade (sem Docker no caminho da VPN), para o
# mantenedor rodar no próprio computador:
#
#   1. monta o pacote do componente de rede, compilando o strongSwan embutido
#      e o helper (como usuário);
#   2. instala o pacote, como o app faz (PEDE SENHA: sudo);
#   3. roda o E2E completo do CLI forçando o engine embutido
#      (IPSEC_CLIENT_BACKEND=native), contra o gateway de teste local.
#
# O passo 3 também testa o polkit de verdade: rode num terminal da sua sessão
# gráfica (sessão local ativa). Para desinstalar depois:
#   sudo /opt/ipsec-client/libexec/install-native.sh uninstall

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo"
# O cargo do rustup (respeita o rust-toolchain.toml), mesmo com outro no PATH.
if [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck source=/dev/null
    . "$HOME/.cargo/env"
fi

bundle="$repo/target/network-component"

echo "==> 1/3 building the network component bundle"
scripts/stage-network-component.sh --out "$bundle"

echo "==> 2/3 installing the network component (sudo)"
sudo "$bundle/install-native.sh" install --bundle "$bundle" --origin manual

echo "==> 3/3 CLI E2E with the embedded engine"
IPSEC_CLIENT_BACKEND=native tests/cli/e2e.sh
echo
echo "✔ embedded engine validated. It stays installed and becomes the app default."
echo "  Para voltar ao Docker: sudo /opt/ipsec-client/libexec/install-native.sh uninstall"
