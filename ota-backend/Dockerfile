ARG CARGO_FEATURES=""
ARG CARGO_PROFILE="artifact"


# Make sure its the same as rust-toolchain.toml
FROM rust:1.84.1-bullseye AS chef
COPY rust-toolchain.toml .
RUN cargo install --locked cargo-chef@0.1.71


FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json


FROM chef AS builder

ARG CARGO_FEATURES
ARG CARGO_PROFILE

# Install necessary build tools and libraries
RUN apt-get update && apt-get install -y \
    build-essential \
    gcc-x86-64-linux-gnu \
    wget \
    unzip \
    && rm -rf /var/lib/apt/lists/*

RUN --mount=type=secret,id=GITHUB_TOKEN git config --global url."https://${GITHUB_TOKEN}:x-oauth-basic@github.com/worldcoin/".insteadOf "https://github.com/worldcoin/"

# Cache cargo dependencies
COPY --from=planner /recipe.json recipe.json
RUN cargo chef cook \
    -p orb-ota-backend \
    --profile "${CARGO_PROFILE}" \
    --no-default-features \
    --features "${CARGO_FEATURES}" \
    --recipe-path recipe.json

# Do the actual cargo build
COPY . .
RUN cargo build \
    -p orb-ota-backend \
    --profile "${CARGO_PROFILE}" \
    --no-default-features \
    --features "${CARGO_FEATURES}"


FROM gcr.io/distroless/cc-debian12:nonroot

WORKDIR /opt

COPY --from=builder --chown=0:10001 --chmod=0555 target/artifact/orb-ota-backend /opt/orb-ota-backend

EXPOSE 3000
ENTRYPOINT ["/opt/orb-ota-backend"]
