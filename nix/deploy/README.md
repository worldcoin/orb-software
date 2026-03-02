# HIL Machine Deployment

This directory contains deployment configuration for HIL (Hardware-in-Loop) machines using:
- **deploy-rs**: NixOS deployment tool
- **Teleport**: Secure SSH access
- **GitHub Actions**: CI/CD orchestration

## Architecture

```
GitHub Actions Runner
    ↓
Teleport Proxy (secure tunnel)
    ↓
HIL Machine (via SSH)
    ↓
deploy-rs activates NixOS configuration
```

## Prerequisites

### For GitHub Actions (Automated Deployment)

You need these secrets configured in GitHub repository settings:

1. **TELEPORT_PROXY**: Your Teleport proxy address (e.g., `teleport.example.com:443`)
2. **TELEPORT_IDENTITY**: Teleport identity file content (base64 or raw)
   - Get this from: `tsh login` → `cat ~/.tsh/keys/*/identity`
3. **ORB_GIT_HUB_TOKEN**: GitHub token for private repo access
4. **CACHIX_AUTH_TOKEN**: (Optional) For Nix binary cache

### For Local Deployment

1. Install deploy-rs:
   ```bash
   nix profile install github:serokell/deploy-rs
   ```

2. Setup Teleport:
   ```bash
   # Login to Teleport
   tsh login --proxy=your-proxy.teleport.sh

   # Verify you can SSH to a HIL machine
   tsh ssh worldcoin@worldcoin-hil-munich-0
   ```

3. Configure SSH config (`~/.ssh/config`):
   ```
   Host worldcoin-hil-*
       ProxyCommand tsh proxy ssh %r@%h:%p
       StrictHostKeyChecking no
   ```

## Usage

### Via GitHub Actions (Recommended)

1. Go to **Actions** tab in GitHub
2. Select **"Deploy to HIL Machines"** workflow
3. Click **"Run workflow"**
4. Choose:
   - **Target**: Which machine (or "all")
   - **Dry run**: Test without activating

### Via Command Line

Deploy to a single machine:
```bash
# From repo root
deploy .#worldcoin-hil-munich-0
```

Deploy to all machines:
```bash
deploy .
```

Dry run (build but don't activate):
```bash
deploy --dry-activate .#worldcoin-hil-munich-0
```

Build locally, deploy remotely:
```bash
deploy --remote-build .#worldcoin-hil-munich-0
```

## How It Works

### 1. Build Phase
- deploy-rs builds the NixOS system closure
- Can build locally or on target machine
- Result is copied to target via SSH

### 2. Activation Phase
- deploy-rs runs `nixos-rebuild switch` on target
- New configuration is activated
- Services are restarted as needed

### 3. Rollback Safety
- **Magic Rollback**: Automatically reverts if activation fails
- **Auto Rollback**: Reverts if system becomes unreachable
- Previous generation remains bootable

### 4. SSH via Teleport
- GitHub Actions → Teleport Proxy → HIL Machine
- No public SSH ports exposed
- Uses Teleport's audit log and access controls

## Configuration

### Adding a New HIL Machine

Edit `nix/deploy/flake-outputs.nix`:

```nix
hilMachines = [
  # ... existing machines ...
  "worldcoin-hil-new-location"
];
```

And create the NixOS configuration:
```bash
mkdir -p nix/machines/worldcoin-hil-new-location
# Add configuration.nix and hardware-configuration.nix
```

### Changing Deployment Options

In `nix/deploy/flake-outputs.nix`, you can configure:

```nix
mkHilDeploy = { hostname }: {
  profiles.system = {
    user = "root";          # User to activate as
    sshUser = "worldcoin";  # User to SSH as
    magicRollback = true;   # Auto-rollback on failure
    autoRollback = true;    # Revert if unreachable
    sshOpts = [ ... ];      # Extra SSH options
  };
};
```

## Troubleshooting

### Deployment Fails

Check the deploy-rs output for errors:
```bash
deploy --debug-logs .#worldcoin-hil-munich-0
```

### SSH Connection Issues

Test Teleport connectivity:
```bash
tsh ssh worldcoin@worldcoin-hil-munich-0 echo "Connection OK"
```

Test with deploy-rs:
```bash
deploy --dry-activate .#worldcoin-hil-munich-0
```

### Rollback After Failed Deploy

If a machine is in a bad state after deployment:

1. SSH to the machine:
   ```bash
   tsh ssh worldcoin@worldcoin-hil-munich-0
   ```

2. List generations:
   ```bash
   sudo nix-env --list-generations --profile /nix/var/nix/profiles/system
   ```

3. Rollback:
   ```bash
   sudo nixos-rebuild switch --rollback
   ```

### Check Deployment Status

View system generation info:
```bash
tsh ssh worldcoin@worldcoin-hil-munich-0 \
  'nixos-version && nix-env --list-generations --profile /nix/var/nix/profiles/system | tail -5'
```

## Security Considerations

1. **Teleport Identity**: Keep the identity file secret. Rotate regularly.
2. **SSH Keys**: Authorized keys are in `nix/machines/ssh-keys.nix`
3. **Sudo Access**: deploy-rs needs passwordless sudo for `nixos-rebuild`
4. **Audit**: All deployments are logged in GitHub Actions and Teleport

## Updating orb-hil Version

To update the orb-hil version installed on HIL machines:

1. Edit `nix/overlays/orb-hil.nix`:
   ```nix
   version = "0.0.3-beta.0";  # Update version
   sha256 = "...";            # Update hash
   ```

2. Commit and push to main branch

3. Deploy via GitHub Actions or manually:
   ```bash
   deploy .
   ```

## Monitoring

After deployment, verify:

1. System is running new generation:
   ```bash
   tsh ssh worldcoin@worldcoin-hil-munich-0 nixos-version
   ```

2. orb-hil is the correct version:
   ```bash
   tsh ssh worldcoin@worldcoin-hil-munich-0 orb-hil --version
   ```

3. Services are healthy:
   ```bash
   tsh ssh worldcoin@worldcoin-hil-munich-0 systemctl status
   ```

## References

- [deploy-rs Documentation](https://github.com/serokell/deploy-rs)
- [Teleport Documentation](https://goteleport.com/docs/)
- [NixOS Manual - Upgrading](https://nixos.org/manual/nixos/stable/#sec-upgrading)
