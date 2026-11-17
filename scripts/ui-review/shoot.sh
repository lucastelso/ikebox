#!/usr/bin/env bash
# UI review screenshots (phase 5 of the post-0.2.0 program): builds the app
# with fake IPC (scripts/ui-review/main.tsx) and takes screenshots with a
# headless Firefox, in a throwaway profile, of every scenario in English and
# Portuguese, light and dark, at the window's default size (440×720) and at
# its minimum width (380 px).
#
#   scripts/ui-review/shoot.sh [scenario ...]
#
# Output: target/ui-review/shots/<scenario>-<lang>-<theme>[-narrow].png
# Nothing here touches the real app, the network or the user's Firefox.

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
here="$repo/scripts/ui-review"
out="$repo/target/ui-review"
readonly PORT=4173
ALL=(first-use idle credentials connecting connected error outdated profiles profile-open system editor editor-problems)
NARROW=(idle connected error system profile-open)
if (($# > 0)); then ALL=("$@"); NARROW=("$@"); fi

command -v firefox >/dev/null || { echo "firefox is required" >&2; exit 1; }
(cd "$repo" && npx vite build --config "$here/vite.config.ts" --logLevel error)
mkdir -p "$out/shots"

profile=$(mktemp -d)
python3 -I "$here/serve.py" "$out/site" "$PORT" &
server=$!
trap 'kill "$server" 2>/dev/null; rm -rf "$profile"' EXIT
sleep 0.5

shot() {
    local name=$1 url=$2 size=$3
    firefox --headless --no-remote --profile "$profile" --window-size="$size" \
        --screenshot "$out/shots/$name.png" "$url" >/dev/null 2>&1
    printf '  %s\n' "$name"
}

for scenario in "${ALL[@]}"; do
    for lang in en pt; do
        for theme in light dark; do
            shot "$scenario-$lang-$theme" \
                "http://127.0.0.1:$PORT/?scenario=$scenario&lang=$lang&theme=$theme" 440,720
        done
    done
done
# Minimum width: Portuguese, the longer language.
for scenario in "${NARROW[@]}"; do
    shot "$scenario-pt-light-narrow" "http://127.0.0.1:$PORT/?scenario=$scenario&lang=pt&theme=light" 380,720
done
echo "screenshots in ${out#"$repo"/}/shots"
