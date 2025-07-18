# Orb Registration Script

A python script for generating and registering Orb devices across both Pearl and Diamond platforms with MongoDB and Core-App.

## Overview

This script (`orb-registration.py`) is a rewrite of the original `gen-orb-id.sh` and `register-mongo.sh` that supports both Pearl and Diamond orb platforms in a single, dependency-free Python implementation. 

**Key Features:**
- **Dual Platform Support**: Handles both Pearl (with artifact generation) and Diamond (registration-only) workflows
- **Zero Dependencies**: Uses only Python standard library - no external packages required

## Requirements

**Python**: Python 3.6 or higher (uses only standard library)

**System Dependencies** (must be available in PATH):
- `ssh-keygen` - SSH key generation
- `mke2fs` - ext4 filesystem creation  
- `tune2fs` - filesystem tuning
- `mount/umount` - image mounting capabilities
- `install` - file installation with permissions
- `setfacl` - ACL support
- `sync` - filesystem synchronization
- `cloudflared` - Cloudflare Access authentication

**Environment Variables**:
- `FM_CLI_ORB_MANAGER_INTERNAL_TOKEN` - MongoDB bearer token (can be overridden with `--mongo-token`)
- `HARDWARE_TOKEN_PRODUCTION` - Core-App bearer token (can be overridden with `--core-token`)

## Installation

1. **Make the script executable:**
   ```bash
   chmod +x orb-registration.py
   ```

2. **Ensure system dependencies are installed:**
   ```bash
   # Ubuntu/Debian
   sudo apt-get install e2fsprogs acl cloudflared


3. **Set up environment variables:**
   ```bash
   export FM_CLI_ORB_MANAGER_INTERNAL_TOKEN="your_mongo_token_here"
   export HARDWARE_TOKEN_PRODUCTION="your_core_app_token_here"
   ```

## Usage

### Basic Command Structure

```bash
./orb-registration.py --platform {pearl|diamond} --backend {stage|prod} --release {dev|prod} --hardware-version HARDWARE_VERSION [additional options]
```

### Required Arguments

- `--platform`: Platform type (`pearl` or `diamond`)
- `--backend`: Backend environment (`stage` or `prod`)
- `--release`: Release type (`dev` or `prod`)
- `--hardware-version`: Hardware version with platform prefix (e.g., `PEARL_EVT1`, `DIAMOND_EVT2`)

### Optional Arguments

- `--channel`: Channel for orb registration (default: `general` for prod, `internal-testing` for stage)
- `--mongo-token`: MongoDB bearer token (overrides environment variable)
- `--core-token`: Core-App bearer token (overrides environment variable)

### Pearl Platform Options

- `--count`: Number of orbs to generate (default: 1)

### Diamond Platform Options

- `--input-file`: Input file containing orb IDs or orb ID+name pairs
- `--input-format`: Format of input file (`ids` or `pairs`, default: `ids`)
- `orb_ids`: Direct orb IDs as positional arguments (alternative to `--input-file`)

## Platform-Specific Workflows

### Pearl Platform

Pearl orbs require complete artifact generation including SSH keys, persistent images, and registration in both systems.

**What Pearl workflow does:**
1. Generates SSH keypair and derives orb-id (SHA256 hash)
2. Registers orb in MongoDB Management API
3. Sets orb channel
4. Retrieves orb token
5. Creates persistent filesystem images (1MB and 10MB variants)
6. Installs baseline configuration files
7. Generates per-orb artifacts with embedded orb-name and token
8. Registers orb in Core-App

**Generated artifacts** (stored in `artifacts/{orb-id}/`):
- `uid` - Private SSH key
- `uid.pub` - Public SSH key
- `orb-name` - Assigned orb name
- `token` - Orb authentication token
- `persistent.img` - 1MB persistent filesystem image
- `persistent-journaled.img` - 10MB persistent filesystem image with journal

### Diamond Platform

Diamond orbs only require registration without artifact generation.

**What Diamond workflow does:**
1. Processes orb IDs from input (file or CLI arguments)
2. Registers each orb in MongoDB Management API (if using IDs-only format)
3. Registers each orb in Core-App

**Input formats:**
- **IDs format**: File contains one orb ID per line, script gets orb-name from MongoDB
- **Pairs format**: File contains `orb-id orb-name` pairs, script skips MongoDB registration

## Detailed Usage Examples

### Pearl Platform Examples

#### Generate Single Pearl Orb (Stage Environment)
```bash
./orb-registration.py \
    --platform pearl \
    --backend stage \
    --release dev \
    --hardware-version PEARL_EVT1
```

#### Generate Multiple Pearl Orbs (Production Environment)
```bash
./orb-registration.py \
    --platform pearl \
    --backend prod \
    --release prod \
    --hardware-version PEARL_EVT1 \
    --count 10 \
    --channel production-batch-1
```

**Output:**
- Creates 10 separate artifact directories
- Uses custom channel "production-batch-1"
- Registers all orbs in production systems

#### Pearl with Custom Tokens
```bash
./orb-registration.py \
    --platform pearl \
    --backend prod \
    --release dev \
    --hardware-version PEARL_EVT1 \
    --count 5 \
    --mongo-token "custom_mongo_token" \
    --core-token "custom_core_token" \
    --channel development
```

### Diamond Platform Examples

#### Register Diamond Orbs from File (IDs Only)
```bash
# Create input file
cat > diamond_orbs.txt << EOF
abc123def456
ghi789jkl012
mno345pqr678
EOF

./orb-registration.py \
    --platform diamond \
    --backend prod \
    --release prod \
    --hardware-version DIAMOND_EVT2 \
    --input-file diamond_orbs.txt \
    --channel diamond-production
```

**What happens:**
- Reads orb IDs from `diamond_orbs.txt`
- Registers each in MongoDB to get orb-name
- Registers each in Core-App
- No artifacts generated

#### Register Diamond Orbs with Pre-assigned Names
```bash
# Create input file with orb-id and orb-name pairs
cat > diamond_pairs.txt << EOF
abc123def456 diamond-orb-001
ghi789jkl012 diamond-orb-002
mno345pqr678 diamond-orb-003
EOF

./orb-registration.py \
    --platform diamond \
    --backend prod \
    --release prod \
    --hardware-version DIAMOND_EVT2 \
    --input-file diamond_pairs.txt \
    --input-format pairs \
    --channel diamond-custom
```

**What happens:**
- Reads orb-id and orb-name pairs from file
- Skips MongoDB registration (names already provided)
- Registers directly in Core-App
- Uses custom channel "diamond-custom"

#### Register Diamond Orbs via CLI Arguments
```bash
./orb-registration.py \
    --platform diamond \
    --backend stage \
    --release dev \
    --hardware-version DIAMOND_EVT2 \
    abc123def456 ghi789jkl012 mno345pqr678
```

**What happens:**
- Processes orb IDs provided as CLI arguments
- Registers in stage environment
- Uses default stage channel "internal-testing"

## File Structure

```
gen-device-unique/
├── orb-registration.py              # Main script
├── build/                     # Baseline configuration files
│   ├── components.json
│   ├── calibration.json
│   └── versions.json
├── artifacts/                 # Generated Pearl artifacts
│   └── [orb-id]/             # Per-orb artifact directory
│       ├── uid               # Private SSH key
│       ├── uid.pub           # Public SSH key
│       ├── orb-name          # Assigned orb name
│       ├── token             # Orb authentication token
│       ├── persistent.img    # 1MB filesystem image
│       └── persistent-journaled.img # 10MB filesystem image
└── README.md                 # This file
```

## Channel Configuration

### Stage Environment
- **Fixed Channel**: `internal-testing`
- **Behavior**: Channel cannot be overridden for stage environment
- **Usage**: Primarily for internal testing and development

### Production Environment
- **Default Channel**: `general`
- **Customizable**: Can be overridden with `--channel` argument
- **Usage**: Flexible channel assignment for production deployments

**Channel Examples:**
```bash
# Uses default "general" channel
./orb-registration.py --platform pearl --backend prod --release prod --hardware-version PEARL_EVT1

# Uses custom channel
./orb-registration.py --platform pearl --backend prod --release prod --hardware-version PEARL_EVT1 --channel batch-2024-01

# Stage always uses "internal-testing" regardless of --channel
./orb-registration.py --platform pearl --backend stage --release dev --hardware-version PEARL_EVT1 --channel ignored
```

## Error Handling


### Common Error Scenarios

#### Already Registered Orb
```
[ERROR] Failed to register orb abc123def456 in MongoDB: HTTP 409 Conflict - {"error": "Orb already exists"}
```

#### Invalid Authentication
```
[ERROR] Failed to register orb abc123def456 in MongoDB: HTTP 401 Unauthorized - {"error": "Invalid token"}
```

#### Network Issues
```
[ERROR] Failed to register orb abc123def456 in Core-App: HTTP 500 Internal Server Error - {"error": "Database connection failed"}
```

#### Missing Dependencies
```
[ERROR] Command 'ssh-keygen' not found. Please install OpenSSH client.
```

## API Endpoints

### MongoDB Management API
- **Stage**: `https://management.internal.stage.orb.worldcoin.dev`
- **Production**: `https://management.internal.orb.worldcoin.dev`

**Endpoints used:**
- `POST /api/v1/orbs/{orb_id}` - Register orb
- `POST /api/v1/orbs/{orb_id}/channel` - Set channel
- `POST /api/v1/tokens?orbId={orb_id}` - Get token

### Core-App API
- **Endpoint**: `https://api.operator.worldcoin.org/v1/graphql`
- **Method**: GraphQL mutation `InsertOrb`

## Troubleshooting

### Common Issues

#### Permission Errors
```bash
# Ensure script is executable
chmod +x orb-registration.py

# Check mount permissions
sudo usermod -a -G disk $USER
```

#### Missing Environment Variables
```bash
# Check if tokens are set
echo $FM_CLI_ORB_MANAGER_INTERNAL_TOKEN
echo $HARDWARE_TOKEN_PRODUCTION

# Set if missing
export FM_CLI_ORB_MANAGER_INTERNAL_TOKEN="your_token"
export HARDWARE_TOKEN_PRODUCTION="your_token"
```
Alternatively pass them as input arguments

#### Cloudflared Issues
```bash
# Login to cloudflared
cloudflared access login --quiet https://management.internal.stage.orb.worldcoin.dev

# Check cloudflared status
cloudflared --version
```

#### File System Issues
```bash
# Check available space
df -h

# Check loop device availability
sudo losetup -a
```

### Debug Mode

For detailed debugging, you can modify the script to enable debug logging:

```python
# In orb-registration.py, change:
logger = generate_logger(logging.INFO)
# To:
logger = generate_logger(logging.DEBUG)
```
