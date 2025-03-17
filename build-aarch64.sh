#!/bin/bash
set -e

echo "===== Cross-compilation for aarch64-unknown-linux-gnu (location only) - RELEASE BUILD ====="

# Build the Docker image
echo "Step 1: Building Docker image..."
docker build -t rust-cross -f location/Dockerfile .
echo "Docker image built successfully!"

# Verify Rust toolchain in the container
echo "Step 2: Verifying Rust toolchain in the container..."
docker run rust-cross rustup show

# Check system libraries and pkg-config paths in the container
echo "Step 3: Checking system libraries and pkg-config setup..." g
docker run rust-cross bash -c "ls -la /usr/lib/aarch64-linux-gnu/pkgconfig/ | grep -E '(alsa|sound)' || echo 'No ALSA pkg-config files found'"
docker run rust-cross bash -c "find /usr -name 'libclang.so*' || echo 'No libclang found'"


# Run the cross-compilation for location only
echo "Step 4: Running cross-compilation for location only (release build)..."
docker run -v "$PWD":/workdir -e LIBCLANG_PATH=/usr/lib/llvm-11/lib rust-cross cargo build --release --verbose --target aarch64-unknown-linux-gnu -p orb-location

echo "===== Release build completed! =====" 
