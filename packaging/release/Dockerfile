# Build environment for the release packages (AppImage, deb and rpm).
#
# Ubuntu 22.04 (glibc 2.35), the same base as the engine builder: the AppImage
# runs on distributions with the same or a newer glibc, and the .deb declares
# the libraries with their Ubuntu/Debian names (e.g. libayatana-appindicator3-1).
# Used by scripts/build-release.sh; CI will use the same path.
FROM ubuntu:22.04@sha256:9abcaffc92a5279fff3a6a41635c0800ea33e528425fb389658806476e7defde

ARG RUST_VERSION=1.100.0
# Every download below is pinned by SHA-256 here (audit P4), not checked
# against a sum fetched from the same server. To update one, change the
# version and its hash together, in their own commit.
ARG RUSTUP_VERSION=1.29.3
ARG RUSTUP_SHA256=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55
ARG NODE_VERSION=24.18.0
# From SHASUMS256.txt, whose GPG signature was checked when pinning
# (Node.js releaser key C82FA3AE1CBEDC6BE46B9360C43CEC45C17AB93C).
ARG NODE_SHA256=55aa7153f9d88f28d765fcdad5ae6945b5c0f98a36881703817e4c450fa76742

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates cpio curl file git pkg-config rpm2cpio xz-utils \
        libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \
        libxdo-dev libssl-dev libdbus-1-dev \
    && rm -rf /var/lib/apt/lists/*

# Rust: a fixed rustup-init release, checked against the pinned hash.
RUN set -eu; \
    base="https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/x86_64-unknown-linux-gnu"; \
    curl --proto '=https' --tlsv1.2 -fsSLO "$base/rustup-init"; \
    echo "$RUSTUP_SHA256  rustup-init" | sha256sum -c -; \
    chmod +x rustup-init; \
    ./rustup-init -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION" \
        --component rustfmt --component clippy; \
    rm rustup-init
ENV PATH=/root/.cargo/bin:$PATH

# Node.js: the official tarball, checked against the pinned hash.
RUN set -eu; \
    base="https://nodejs.org/dist/v$NODE_VERSION"; \
    file="node-v$NODE_VERSION-linux-x64.tar.xz"; \
    curl --proto '=https' --tlsv1.2 -fsSLO "$base/$file"; \
    echo "$NODE_SHA256  $file" | sha256sum -c -; \
    tar -xJf "$file" -C /usr/local --strip-components=1 --no-same-owner; \
    rm "$file"; \
    node --version

# AppImage tools. Left to itself, Tauri downloads them on the first bundle,
# unpinned, one of them from linuxdeploy-plugin-appimage's moving
# "continuous" tag. They are placed in Tauri's cache here, under the names
# the Tauri CLI looks for, so it never downloads them; build-release.sh
# fails if the cache holds anything else after the bundle. The linuxdeploy
# version (07333c6) is the one hard-coded in the Tauri CLI: when an update
# of @tauri-apps/cli changes it, the bundle downloads a new file and that
# check stops the build. Tauri zeroes the AppImage magic bytes (offset 8) of
# linuxdeploy after downloading it, so it can run in a container without
# binfmt; the same is done here, and the result is checked too.
ARG TAURI_TOOLS=/root/.cache/tauri
RUN set -eu; \
    mkdir -p "$TAURI_TOOLS"; cd "$TAURI_TOOLS"; \
    fetch() { curl --proto '=https' --tlsv1.2 -fsSL -o "$1" "$2"; echo "$3  $1" | sha256sum -c -; }; \
    fetch linuxdeploy-07333c6-x86_64.AppImage \
        https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy-07333c6/linuxdeploy-x86_64.AppImage \
        36a2d7e274d12e1050d0e9ecfe11d339ed54720b2bec464c286d53f8b07f5c62; \
    fetch AppRun-x86_64 \
        https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64 \
        f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f; \
    fetch linuxdeploy-plugin-appimage.AppImage \
        https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage \
        49d6a17160675a6bd1781699aae6bdf7692d98552e02a3671d2183d10547842e; \
    printf '\0\0\0' | dd of=linuxdeploy-07333c6-x86_64.AppImage bs=1 seek=8 conv=notrunc status=none; \
    echo "bd9521cd5ff3ca351fecb78cd9b236a4e37b3621014ac05cac0c2d767ffa0474  linuxdeploy-07333c6-x86_64.AppImage" | sha256sum -c -; \
    chmod 0755 ./*; \
    sha256sum ./* >/etc/ikebox-tauri-tools.sha256

# linuxdeploy (used by Tauri for the AppImage) is an AppImage itself: without
# FUSE in the container, it extracts itself and runs.
ENV APPIMAGE_EXTRACT_AND_RUN=1
WORKDIR /build
