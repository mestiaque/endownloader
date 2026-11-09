# Reproducible Ubuntu 22.04 build (and headless test) environment for ENSave.
#   docker build -t endl-build scripts/docker
#   docker run --rm -v "$PWD":/src -w /src endl-build scripts/build-linux.sh
FROM ubuntu:22.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential curl ca-certificates git pkg-config file patchelf cmake \
      libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libssl-dev libxdo-dev \
      libsecret-1-dev aria2 python3 ffmpeg \
      gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav \
      xvfb xdotool imagemagick dbus-x11 fonts-dejavu-core fonts-noto-core at-spi2-core \
    && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - && apt-get install -y --no-install-recommends nodejs && rm -rf /var/lib/apt/lists/* \
    && npm install -g pnpm@9.15.0
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
    && chmod -R a+w /opt/rustup /opt/cargo
