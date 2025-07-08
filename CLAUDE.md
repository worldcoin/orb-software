# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

### Building
```bash
# Build all crates (default features)
cargo build --all

# Build for release
cargo build --all --release

# Build with specific profile (for artifacts)
cargo build --all --profile artifact

# Cross-compile for Linux targets
cargo zigbuild --target aarch64-unknown-linux-gnu --all
cargo zigbuild --target x86_64-unknown-linux-gnu --all
```

### Testing
```bash
# Run all tests
cargo test --all --all-features --all-targets

# Run tests excluding platform-specific crates
cargo test --all --all-features --all-targets --exclude <platform-specific-crates>

# Run tests for specific crate
cargo test -p <crate-name>
```

### Linting and Formatting
```bash
# Format code
cargo fmt --all

# Check formatting
cargo fmt --check --all

# Run clippy
cargo clippy --all --all-features --all-targets --no-deps -- -D warnings

# Generate documentation
cargo doc --all --all-features --no-deps --document-private-items
```

### Security and Licensing
```bash
# Check licenses and advisories
cargo deny check
```

### Development Environment
```bash
# Enter nix development shell
nix develop

# Run commands in nix shell
nix develop -c <command>
```

## Architecture Overview

This is a Rust monorepo containing 60+ crates that form the complete software stack for Worldcoin's Orb biometric scanning device. The architecture follows a distributed, agent-based design with strong security boundaries.

### Core Architecture Patterns

**Agent-Based System (agentwire)**
- Custom `agentwire` library provides message-passing agent architecture
- Agents run in separate tasks, threads, or processes with isolated communication
- Central broker manages agent lifecycle and message routing
- Communication via bi-directional ports with shared memory for process-based agents

**D-Bus Communication Layer**
- Extensive use of D-Bus for inter-process communication
- Components expose well-defined D-Bus interfaces
- Custom session bus at `unix:path=/tmp/worldcoin_bus_socket`
- Code generation via `zbus-proxies` for automated D-Bus bindings

**Systemd Service Architecture**
- Each major component runs as systemd service
- Proper service ordering and dependencies
- Privilege separation with minimal required privileges
- Auto-restart policies for critical services

### Main Application Components

**orb-supervisor**
- Central coordinator and device state manager
- Runs as root with elevated permissions
- Manages device health (thermal, fan control)
- Coordinates cross-service operations and shutdown

**orb-core** (External Repository)
- Core signup logic and sensor management
- Communicates via D-Bus with other services
- Manages biometric scanning hardware
- Runs with restricted privileges

**orb-ui**
- User interface and experience management
- LED control (60fps RGB updates via serial to MCU)
- Sound system with multi-language support
- Visual feedback through LED animations

**orb-update-agent**
- Over-the-air update system
- Cryptographically signed updates with verification
- Binary diffing for efficient updates
- A/B slot system for reliable rollback

**orb-attest**
- Hardware attestation and authentication
- SE050 secure element integration
- Challenge-response authentication with backend
- Provides auth tokens to other services

### Hardware Interface Systems

**MCU Communication**
- CAN bus and ISO-TP for microcontroller communication
- `mcu-interface`: High-level MCU communication
- `mcu-util`: MCU management utilities
- `can`: Low-level CAN bus implementation

**Sensor Integration**
- `thermal-cam-ctrl`: SEEK camera integration
- Serial communication via UART interfaces
- Hardware abstraction with platform-specific implementations

**LED and Audio Control**
- Direct serial communication with MCU at 60fps
- GStreamer-based audio pipeline
- Coordinated audio-visual feedback

### Security and Update System

**Hardware Root of Trust**
- SE050 secure element for cryptographic operations
- NXP certificate chain validation
- Separate attestation and signup keys

**Update Security**
- All updates cryptographically signed
- Multi-stage verification process
- Automatic rollback on failure
- Component-based updates with `bidiff` for efficiency

## Key Development Patterns

### Agent Development
- Use `agentwire` framework for inter-component communication
- Implement agents as separate tasks, threads, or processes
- Use broker pattern for message routing
- Leverage `rkyv` serialization for shared memory communication

### D-Bus Integration
- Expose service interfaces via D-Bus
- Use `zbus-proxies` for code generation
- Handle session bus at `/tmp/worldcoin_bus_socket`
- Implement proper error handling for D-Bus operations

### Hardware Abstraction
- Separate hardware-specific code from business logic
- Use trait-based abstractions for hardware interfaces
- Implement platform-specific features with conditional compilation
- Test hardware interfaces with mock implementations

### Security Best Practices
- Run services with minimal required privileges
- Use hardware attestation for authentication
- Implement proper certificate validation
- Never log or expose sensitive data

### Testing Patterns
- Use `test-utils` for common testing infrastructure
- Implement integration tests for cross-component interactions
- Use Docker containers for isolated testing
- Test hardware interfaces with HIL (Hardware-in-Loop) framework

## Workspace Organization

The repository is organized as a Cargo workspace with:
- **Core Services**: `supervisor`, `ui`, `update-agent`, `attest`
- **Hardware Interfaces**: `mcu-interface`, `thermal-cam-ctrl`, `can`
- **Utilities**: `build-info`, `telemetry`, `security-utils`
- **Development Tools**: `hil`, `tools`, `test-utils`
- **External Repositories**: `orb-core`, `orb-firmware`, `orb-messages`

## Development Environment Setup

This project uses Nix for reproducible development environments:
- Run `nix develop` to enter the development shell
- All development tools are provided via Nix
- Cross-compilation support for ARM64 and x86_64 Linux targets
- Rust toolchain version pinned to 1.87.0

## Common Development Workflows

### Adding New Services
1. Create new crate in workspace
2. Add D-Bus interface definition
3. Implement service with proper privilege separation
4. Add systemd service file
5. Update CI/CD pipeline

### Hardware Interface Development
1. Create hardware abstraction traits
2. Implement platform-specific code
3. Add mock implementations for testing
4. Integration with HIL testing framework

### Agent Development
1. Use `agentwire` framework
2. Define message protocols
3. Implement agent logic
4. Add broker integration
5. Test with agent testing framework

### Security Implementation
1. Use hardware attestation where possible
2. Implement proper certificate handling
3. Follow privilege separation principles
4. Add security testing and validation
