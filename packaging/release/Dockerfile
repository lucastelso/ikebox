# Ambiente de build dos pacotes de release (AppImage, deb e rpm).
#
# Ubuntu 22.04 (glibc 2.35), a mesma base do builder do engine: o AppImage
# roda em distribuições com glibc igual ou mais nova, e o .deb declara as
# bibliotecas com os nomes do Ubuntu/Debian (ex.: libayatana-appindicator3-1).
# Usado por scripts/build-release.sh; a CI vai usar o mesmo caminho.
FROM ubuntu:22.04@sha256:9abcaffc92a5279fff3a6a41635c0800ea33e528425fb389658806476e7defde

ARG RUST_VERSION=1.100.0
ARG NODE_VERSION=24.18.0

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates cpio curl file git pkg-config rpm2cpio xz-utils \
        libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \
        libxdo-dev libssl-dev libdbus-1-dev \
    && rm -rf /var/lib/apt/lists/*

# Rust: rustup-init conferido pelo .sha256 publicado junto.
RUN set -eu; \
    base=https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu; \
    curl --proto '=https' --tlsv1.2 -fsSLO "$base/rustup-init"; \
    sum=$(curl --proto '=https' --tlsv1.2 -fsSL "$base/rustup-init.sha256" | awk '{print $1}'); \
    echo "$sum  rustup-init" | sha256sum -c -; \
    chmod +x rustup-init; \
    ./rustup-init -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION" \
        --component rustfmt --component clippy; \
    rm rustup-init
ENV PATH=/root/.cargo/bin:$PATH

# Node.js: tarball oficial conferido pelo SHASUMS256.txt.
RUN set -eu; \
    base="https://nodejs.org/dist/v$NODE_VERSION"; \
    file="node-v$NODE_VERSION-linux-x64.tar.xz"; \
    curl --proto '=https' --tlsv1.2 -fsSLO "$base/$file"; \
    curl --proto '=https' --tlsv1.2 -fsSL "$base/SHASUMS256.txt" | grep " $file\$" | sha256sum -c -; \
    tar -xJf "$file" -C /usr/local --strip-components=1 --no-same-owner; \
    rm "$file"; \
    node --version

# O linuxdeploy (usado pelo Tauri no AppImage) é um AppImage: sem FUSE no
# container, ele se extrai e roda.
ENV APPIMAGE_EXTRACT_AND_RUN=1
WORKDIR /build
