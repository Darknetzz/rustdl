# Optional CI image: Linux GUI deps + MinGW Windows GNU toolchain.
# Build/push on the GitLab runner host:
#   docker build -t registry.roste.org/kriss/rustdl/ci:latest -f ci/Dockerfile .
#   docker push registry.roste.org/kriss/rustdl/ci:latest
# Then set GitLab CI/CD variable CI_RUST_IMAGE=registry.roste.org/kriss/rustdl/ci:latest
FROM rust:latest

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        cmake \
        clang \
        libclang-dev \
        nasm \
        python3 \
        curl \
        ca-certificates \
        libgtk-3-dev \
        libglib2.0-dev \
        libxkbcommon-dev \
        libxcb-render0-dev \
        libxcb-shape0-dev \
        libxcb-xfixes0-dev \
        libwayland-dev \
        libasound2-dev \
        gcc-mingw-w64-x86-64 \
        g++-mingw-w64-x86-64 \
        binutils-mingw-w64-x86-64 \
    && rustup component add rustfmt clippy \
    && rustup target add x86_64-pc-windows-gnu \
    && rm -rf /var/lib/apt/lists/*

ENV CARGO_HOME=/usr/local/cargo
ENV CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
