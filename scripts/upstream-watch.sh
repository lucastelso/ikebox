#!/usr/bin/env bash
# Upstream watch (ADR-0026), run weekly by CI: is there something the embedded
# engine should pick up?
#
#   - a strongSwan release newer than the one pinned in native-engine/versions.txt;
#   - a new revision of OpenSSL or musl in the engine builder's Alpine (they are
#     linked statically: a security fix there means rebuilding the engine),
#     compared with the ones inside the latest published release;
#   - an OpenSSL release in that Alpine other than the one host builds compile
#     (native-engine/openssl.txt, ADR-0032): the two should move together.
#
# Prints what it finds. With --issue (CI, with GH_TOKEN), opens one issue per
# finding, unless an open issue with the same title exists.
#
# Usage: scripts/upstream-watch.sh [--issue]
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
open_issues=0
[[ ${1:-} == --issue ]] && open_issues=1
findings=()

note() { printf '  %s\n' "$*"; }

# Highest X.Y.Z among the lines on standard input.
highest() { grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | sort -Vu | tail -n 1; }

# --- strongSwan ----------------------------------------------------------------
pinned=$(grep -vE '^\s*(#|$)' "$repo/native-engine/versions.txt" | head -n 1 | awk '{print $1}')
latest=$(curl --proto '=https' --tlsv1.2 -fsSL https://download.strongswan.org/ |
    grep -oE 'strongswan-[0-9]+\.[0-9]+\.[0-9]+\.tar\.bz2' | highest || true)
if [[ -z $latest ]]; then
    note "strongSwan: could not read the release list"
elif [[ $(printf '%s\n%s\n' "$pinned" "$latest" | sort -V | tail -n 1) != "$pinned" ]]; then
    note "strongSwan: $latest is out (pinned: $pinned)"
    findings+=("strongSwan $latest is out|The embedded engine pins strongSwan $pinned. Check the release notes and security advisories (https://www.strongswan.org/security.html), then update native-engine/versions.txt in its own branch with the full E2E green (ADR-0016, ADR-0026). If $latest drops IKEv1, ADR-0026 applies.")
else
    note "strongSwan: $pinned is the latest"
fi

# --- OpenSSL and musl in the builder's Alpine -----------------------------------
builder=$(sed -n 's/^FROM \(.*\)$/\1/p' "$repo/native-engine/build/Dockerfile" | head -n 1)
available=$(docker run --rm "$builder" sh -c \
    'apk update -q >/dev/null 2>&1; apk list openssl-libs-static musl-dev 2>/dev/null' || true)
alpine_openssl=$(sed -n 's/^openssl-libs-static-\([^ ]*\) .*/\1/p' <<<"$available" | head -n 1)
alpine_musl=$(sed -n 's/^musl-dev-\([^ ]*\) .*/\1/p' <<<"$available" | head -n 1)
note "Alpine now: OpenSSL ${alpine_openssl:-?}, musl ${alpine_musl:-?}"

# The OpenSSL that host builds compile follows the builder's upstream release.
host_openssl=$(grep -vE '^\s*(#|$)' "$repo/native-engine/openssl.txt" | head -n 1 | awk '{print $1}')
alpine_upstream=${alpine_openssl%-r*}
if [[ -n $alpine_upstream && $alpine_upstream != "$host_openssl" ]]; then
    note "host builds compile OpenSSL $host_openssl; the builder has $alpine_upstream"
    findings+=("OpenSSL $alpine_upstream for host builds|The builder's Alpine moved to OpenSSL $alpine_upstream, while host builds (native-engine/build.sh --host) compile $host_openssl from native-engine/openssl.txt. Pin $alpine_upstream there with its SHA-256, checked together with the official signature (keys/openssl-release.asc), update native-engine/licenses/ if the license text changed, and run tests/engine-host/run.sh (ADR-0032).")
fi

# What the latest release carries (its engine's BUILDINFO), when there is one.
shipped=""
if command -v gh >/dev/null 2>&1 && [[ -n ${GH_TOKEN:-} ]]; then
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    if gh release download --pattern '*.deb' --dir "$work" >/dev/null 2>&1; then
        (cd "$work" && ar x ./*.deb && tar -xf data.tar.* ./usr/lib/ikebox/network/engine.tar.gz) &&
            shipped=$(tar -xzOf "$work/usr/lib/ikebox/network/engine.tar.gz" \
                opt/ikebox/engine/BUILDINFO 2>/dev/null || true)
    fi
fi
if [[ -z $shipped ]]; then
    note "no published release to compare with"
else
    shipped_openssl=$(sed -n 's/^openssl=\([^ ]*\).*/\1/p' <<<"$shipped")
    shipped_musl=$(sed -n 's/^musl=\([^ ]*\).*/\1/p' <<<"$shipped")
    note "latest release: OpenSSL ${shipped_openssl:-?}, musl ${shipped_musl:-?}"
    for pair in "OpenSSL:$shipped_openssl:$alpine_openssl" "musl:$shipped_musl:$alpine_musl"; do
        IFS=: read -r name old new <<<"$pair"
        if [[ -n $old && -n $new && $old != "$new" ]]; then
            findings+=("$name $new in the engine builder|The latest release links $name $old statically; Alpine now has $new. If it carries security fixes, rebuild the engine (native-engine/build.sh), run the E2E and release (ADR-0026).")
        fi
    done
fi

# --- Report ----------------------------------------------------------------------
if ((${#findings[@]} == 0)); then
    note "nothing to do"
    exit 0
fi
for finding in "${findings[@]}"; do
    title="Upstream: ${finding%%|*}"
    body=${finding#*|}
    note "→ $title"
    if ((open_issues)); then
        if gh issue list --state open --search "in:title \"$title\"" --json title --jq '.[].title' |
            grep -qxF "$title"; then
            note "  (an open issue already says so)"
        else
            gh issue create --title "$title" --body "$body" >/dev/null
            note "  issue opened"
        fi
    fi
done
