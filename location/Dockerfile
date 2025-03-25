FROM debian:bullseye

RUN apt-get update && apt-get install -y \
    curl \
    git \
    build-essential \
    gcc-aarch64-linux-gnu \
    libc6-dev-arm64-cross \
    pkg-config \
    libudev-dev:arm64 \
    libssl-dev:arm64 \
    protobuf-compiler \
    libasound2-dev:arm64 \
    libclang-dev

ENV PKG_CONFIG_ALLOW_CROSS=1 \
    PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Install specific Rust toolchain version
RUN rustup install 1.84.1
RUN rustup default 1.84.1

# Add the Linux target for the specific toolchain
RUN rustup target add aarch64-unknown-linux-gnu --toolchain 1.84.1

WORKDIR /workdir
