#!/usr/bin/env bash
# check-private.sh — keeps private data (the maintainer's or their
# organization's) out of the repository.
#
# The patterns are NOT in the repository: they are read from a local file, by
# default `.git/info/private-patterns` (one ERE per line, `#` for comments),
# or from the path in $IPSEC_CLIENT_PRIVATE_PATTERNS.
#
# Usage:
#   scripts/check-private.sh              # checks the staged content (the index)
#   scripts/check-private.sh --tracked    # checks the tracked working tree
#   scripts/check-private.sh --history    # checks the whole history (before a push)
#   scripts/check-private.sh --message F  # checks a commit message
#   scripts/check-private.sh --artifacts D  # scans the release packages in D
#                                           # (AppImage, deb, rpm), inside
#
# Exit: 0 = clean (or no patterns configured), 1 = private data found,
#       2 = error.
# It never prints the matching text, only where it is, so the private data
# does not spread through terminal or CI logs.

set -euo pipefail

die() {
    printf 'check-private: %s\n' "$*" >&2
    exit 2
}

repo_root=$(git rev-parse --show-toplevel 2>/dev/null) || die "not a git repository"
cd "$repo_root"

patterns_file=${IPSEC_CLIENT_PRIVATE_PATTERNS:-$(git rev-parse --git-path info/private-patterns)}

mode=${1:---staged}

# Strips comments and empty lines into a private temporary file.
umask 077
clean_patterns=$(mktemp)
work=""
trap 'rm -f "$clean_patterns"; [[ -n $work ]] && rm -rf "$work"' EXIT
if [[ -s "$patterns_file" ]]; then
    grep -vE '^[[:space:]]*(#|$)' "$patterns_file" >"$clean_patterns" || true
fi

if [[ ! -s "$clean_patterns" && $mode != --artifacts ]]; then
    # CI does not have (and must not have) the private patterns: exit quietly.
    if [[ -z "${CI:-}" ]]; then
        printf 'check-private: warning: no patterns in %s (see CONTRIBUTING.md)\n' \
            "$patterns_file" >&2
    fi
    exit 0
fi

fail() {
    printf '\ncheck-private: PRIVATE DATA FOUND (%s).\n' "$1" >&2
    printf 'Replace it with a generic value (e.g. 192.0.2.0/24, vpn.example.com).\n' >&2
    exit 1
}

# scan <label> <location fields> <git grep args...>
# git grep exits with 0 when it finds something, 1 when it does not and >1 on
# error.
scan() {
    local label=$1 fields=$2 out rc=0
    shift 2
    out=$(git grep -I -n -i -E -f "$clean_patterns" "$@" 2>&1) || rc=$?
    case "$rc" in
    0)
        printf '%s\n' "$out" | cut -d: -f"$fields" | sort -u | sed 's/^/  - /' >&2
        fail "$label"
        ;;
    1) ;;
    *) die "git grep failed (invalid pattern in $patterns_file?)" ;;
    esac
}

# extract_package <package> <destination>: the whole content, including the
# strongSwan (engine.tar.gz) and the helper (.gz) that go compressed inside.
extract_package() {
    local pkg=$1 dest=$2 data nested
    mkdir -p "$dest"
    case $pkg in
    *.deb)
        (cd "$dest" && ar x "$pkg" && for data in control.tar.* data.tar.*; do
            tar -xf "$data" && rm -f "$data"
        done) || die "could not extract $pkg"
        ;;
    *.rpm)
        # Ubuntu 22.04's rpm2cpio exits with 1 even after extracting everything.
        (cd "$dest" && { rpm2cpio "$pkg" || true; } | cpio -idm --quiet) ||
            die "could not extract $pkg"
        ;;
    *.AppImage)
        (cd "$dest" && "$pkg" --appimage-extract >/dev/null) || die "could not extract $pkg"
        ;;
    esac
    while IFS= read -r -d '' nested; do
        mkdir -p "$nested.d"
        tar -xzf "$nested" -C "$nested.d" || die "could not extract $nested"
    done < <(find "$dest" -type f -name '*.tar.gz' -print0)
    while IFS= read -r -d '' nested; do
        gzip -dc "$nested" >"${nested%.gz}.decompressed" || die "could not decompress $nested"
    done < <(find "$dest" -type f -name '*.gz' ! -name '*.tar.gz' -print0)
}

# scan_patterns <path>...: files in which the private patterns appear.
# In ELF executables, only readable text of 6+ characters counts: a short
# pattern matches by chance bytes that are not text (it happened in 0.1.1: 3
# isolated bytes in the app's .text section; and in 0.9.0: a sequence of 4
# characters, a brace and three letters, in .rodata, which existed in no
# source, asset or dependency). Real data embedded in the binary (gateway,
# domain, IP) is readable text, and longer than that.
# Compressed files (.gz, .tar.gz) are skipped: their bytes are pseudo-random,
# so a short pattern matches by chance (it happened in 0.2.0, in the
# engine.tar.gz bytes), and extract_package already unpacks them next to the
# original (.d/ and .decompressed), where the real content is scanned.
scan_patterns() {
    local file
    while IFS= read -r -d '' file; do
        if [[ $(head -c 4 "$file" | od -An -tx1 | tr -d ' \n') == 7f454c46 ]]; then
            # Process substitution: only grep decides. A pipe under pipefail
            # would turn a match into SIGPIPE on strings and hide the file.
            grep -q -i -E -f "$clean_patterns" < <(strings -a -n 6 "$file") && printf '%s\n' "$file"
        else
            grep -q -a -i -E -f "$clean_patterns" "$file" && printf '%s\n' "$file"
        fi
    done < <(find "$@" -type f ! -name '*.gz' -print0)
    return 0
}

case "$mode" in
--staged)
    scan "staged content" 1,2 --cached -- .
    ;;
--tracked)
    scan "tracked files" 1,2 -- .
    ;;
--history)
    mapfile -t revs < <(git rev-list --all 2>/dev/null || true)
    ((${#revs[@]} > 0)) || exit 0
    scan "git history" 1-3 "${revs[@]}" -- .
    bad_commits=()
    for rev in "${revs[@]}"; do
        msg=$(git log -1 --format=%B "$rev")
        if grep -q -i -E -f "$clean_patterns" <<<"$msg"; then
            bad_commits+=("$rev")
        fi
    done
    if ((${#bad_commits[@]} > 0)); then
        printf '  - commit message %s\n' "${bad_commits[@]}" >&2
        fail "commit messages"
    fi
    ;;
--message)
    msg_file=${2:-}
    [[ -f "$msg_file" ]] || die "usage: $0 --message <file>"
    # Ignore the comment lines git puts in the editor.
    msg=$(grep -v '^#' "$msg_file" || true)
    if grep -q -i -E -f "$clean_patterns" <<<"$msg"; then
        fail "commit message"
    fi
    ;;
--artifacts)
    dir=$(cd "${2:?usage: $0 --artifacts <directory with the packages>}" && pwd)
    shopt -s nullglob
    packages=("$dir"/*.AppImage "$dir"/*.deb "$dir"/*.rpm)
    ((${#packages[@]} > 0)) || die "no packages in $dir"
    # Besides the private patterns: the home path and this machine's name
    # (generic names are left out, to avoid false positives).
    fixed=("/home/$(id -un)")
    host=$(hostname 2>/dev/null || true)
    case ${host,,} in
    "" | localhost* | fedora | ubuntu | debian | linux | toolbox) ;;
    *) ((${#host} >= 4)) && fixed+=("$host") ;;
    esac
    work=$(mktemp -d)
    found=()
    for pkg in "${packages[@]}"; do
        dest="$work/$(basename "$pkg")"
        extract_package "$pkg" "$dest"
        # The private patterns apply to OUR files. The AppImage also carries
        # Ubuntu 22.04 libraries, copied by linuxdeploy: short patterns match
        # by chance inside them (checked: the same matches exist in the
        # original libraries), so in them only the home path and the hostname
        # are looked for.
        own=("$dest")
        if [[ $pkg == *.AppImage ]]; then
            own=()
            for path in usr/bin usr/lib/ipsec-client usr/share/applications usr/share/icons; do
                [[ -e $dest/squashfs-root/$path ]] && own+=("$dest/squashfs-root/$path")
            done
            for path in "$dest"/squashfs-root/*.desktop; do own+=("$path"); done
        fi
        hits=()
        if [[ -s $clean_patterns ]]; then
            mapfile -t -O "${#hits[@]}" hits < <(scan_patterns "${own[@]}")
        fi
        mapfile -t -O "${#hits[@]}" hits < <(grep -r -a -l -F -i "${fixed[@]/#/-e}" "$dest" || true)
        if ((${#hits[@]} > 0)); then
            printf '%s\n' "${hits[@]#"$work"/}" | sort -u | sed 's/^/  - /' >&2
            found+=("$(basename "$pkg")")
        else
            printf '  ✔ %s: no private data\n' "$(basename "$pkg")"
        fi
    done
    if [[ ! -s $clean_patterns ]] && [[ -z ${CI:-} ]]; then
        printf 'check-private: warning: no patterns in %s; only the home path and the hostname were searched\n' \
            "$patterns_file" >&2
    fi
    ((${#found[@]} == 0)) || fail "inside the packages: ${found[*]}"
    ;;
-h | --help)
    sed -n '2,20p' "$0"
    ;;
*)
    die "unknown option: $mode (use --help)"
    ;;
esac
