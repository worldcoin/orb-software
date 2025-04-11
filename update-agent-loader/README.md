# Update Agent Loader

A loader for the update-agent that downloads and executes a binary from a URL directly from memory without writing to disk.

## Setup

### Key Configuration

Before building, you need to create a `keys` directory with a 32-byte Ed25519 public key file:

```bash
mkdir -p keys
# Generate a key pair if you don't have one
# openssl genpkey -algorithm ed25519 -out private_key.pem
# openssl pkey -in private_key.pem -pubout -out public_key.pem
# Extract the raw 32-byte public key
openssl pkey -in private_key.pem -pubout -outform DER | tail -c 32 > keys/public_key.bin
```

Then build the project:

```bash
cargo build --release
```

## Usage

```
# Download and execute a binary from a URL
update-agent-loader --url https://example.com/path/to/executable

# Download and execute with arguments
update-agent-loader --url https://example.com/path/to/executable --args arg1 arg2 arg3

# Show help
update-agent-loader --help
```

## Features

- Downloads executables directly into memory
- Executes binaries without writing to disk using `fexecve`
- Supports passing arguments to the executed binary
- Uses secure TLS 1.3 with built-in root certificates
- Verifies Ed25519 digital signatures before execution

## Signature Verification

The loader expects downloaded binaries to have an Ed25519 signature appended, with the following format:

```
[executable data][64-byte Ed25519 signature][4-byte signature size (little-endian)]
```

The signature verification process:

1. Reads the last 4 bytes to get the signature size (expected to be 64 for Ed25519)
2. Extracts the signature from the end of the file
3. Truncates the file to remove the signature
4. Verifies the signature against the public key

This ensures that only authorized binaries are executed.

## Development

Enable HTTP downloads for testing (not available in release mode):

```bash
cargo build --features allow_http
```