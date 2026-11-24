# syntax=docker/dockerfile:1
#
# Isolated build image for RecompLauncher.
#
# Ubuntu 24.04 is the base the project's own Linux CI uses (.github/workflows/ci.yml), and the
# dependency list below is that workflow's list plus what this sandbox itself needs. Nothing is
# taken from the host machine: the compiler, the SDKs and the bundler tools all live in here.
#
# Two details are deliberate:
#   * Node is unpacked from the official tarball and checked against nodejs.org's own
#     SHA256SUMS, instead of `curl | bash` from a setup script. Same idea as the project's own
#     rule that a build should verify what it downloads.
#   * No package is fetched from anywhere but Ubuntu's archive, nodejs.org and rust-lang.org.

FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive

# Pinned so the same image builds the same way twice. Bump deliberately.
ARG NODE_VERSION=22.23.5
ARG NODE_SHA256=cdeb8670a0c81f51b4dc4ac61919f9b1ad917b304bf8221eb939570663d240f8
# `stable` is resolved at image build time; the version that was used is written into
# /opt/build-info.txt, and the build script copies it next to the artifacts.
ARG RUST_VERSION=stable

# Build dependencies. The first group is exactly .github/workflows/ci.yml's Linux set; then the
# tools this sandbox needs: rsync (copies the source out of the read-only mount), strace (records
# connect() calls so an offline build can be shown to be offline), file, xz-utils and curl.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential pkg-config \
        libwebkit2gtk-4.1-dev libgtk-3-dev \
        libayatana-appindicator3-dev librsvg2-dev \
        libudev-dev libssl-dev patchelf \
        cmake ca-certificates curl xz-utils file rsync strace \
    && rm -rf /var/lib/apt/lists/*

# Node.js, verified against the checksum nodejs.org publishes for that exact file.
RUN set -eux; \
    curl -fsSL -o /tmp/node.tar.xz \
        "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz"; \
    echo "${NODE_SHA256}  /tmp/node.tar.xz" | sha256sum -c -; \
    tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1; \
    rm /tmp/node.tar.xz; \
    node --version; npm --version

# Rust. The toolchain lives in the image and stays read-only at run time; the registry cache is
# the writable CARGO_HOME set by the run script (/work/cargo).
ENV RUSTUP_HOME=/opt/rustup
RUN set -eux; \
    curl -fsSL -o /tmp/rustup-init \
        "https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init"; \
    chmod +x /tmp/rustup-init; \
    /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain "${RUST_VERSION}"; \
    rm /tmp/rustup-init; \
    toolchain_bin="$(dirname "$(find /opt/rustup/toolchains -maxdepth 3 -type f -name rustc | head -n1)")"; \
    mkdir -p /opt/toolchain-bin; \
    ln -sf "${toolchain_bin}/cargo" /opt/toolchain-bin/cargo; \
    ln -sf "${toolchain_bin}/rustc" /opt/toolchain-bin/rustc; \
    ln -sf "${toolchain_bin}/rustdoc" /opt/toolchain-bin/rustdoc; \
    /opt/toolchain-bin/rustc --version | tee /opt/build-info.txt; \
    node --version >> /opt/build-info.txt; \
    cmake --version | head -n1 >> /opt/build-info.txt

# The toolchain's own bin directory comes first, not rustup's shims: the shims would want to write
# to RUSTUP_HOME, which is read-only once the container runs.
ENV PATH=/opt/toolchain-bin:/usr/local/bin:/usr/bin:/bin

# Everything the build writes goes under /work (a volume) or /out (a mount); the image filesystem
# itself is mounted read-only by the run script.
ENV HOME=/work/home \
    CARGO_HOME=/work/cargo \
    CARGO_TARGET_DIR=/work/target

COPY entry.sh /usr/local/bin/entry.sh
RUN chmod 0755 /usr/local/bin/entry.sh

WORKDIR /work/src
ENTRYPOINT ["/usr/local/bin/entry.sh"]
