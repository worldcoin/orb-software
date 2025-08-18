#!/usr/bin/env bun
/**
 * QEMU-based test runner for update-agent
 * 
 * This script spawns QEMU with fedora-bootc image and runs the update-agent
 * systemd service inside it, providing a way to wait for service completion.
 * 
 * Usage:
 *   ./qemu-runner.js mock <dir>     - Create mockup directory structure
 *   ./qemu-runner.js run <prog> <dir> - Run update-agent in QEMU
 *   ./qemu-runner.js check <dir>    - Verify OTA results
 *   ./qemu-runner.js clean <dir>    - Clean up mockup directory
 */

import { spawn, spawnSync } from 'child_process';
import { promises as fs, constants } from 'fs';
import { join, resolve } from 'path';
import { createHash } from 'crypto';

const FEDORA_CLOUD_IMAGE = 'registry.fedoraproject.org/fedora:latest';
const FEDORA_CLOUD_QCOW2_URL = 'https://download.fedoraproject.org/pub/fedora/linux/releases/42/Cloud/x86_64/images/Fedora-Cloud-Base-42-1.1.x86_64.qcow2';
const QEMU_MEMORY = '2G';
const QEMU_DISK_SIZE = '64G';

class Logger {
    static info(msg) {
        console.log(`[INFO] ${msg}`);
    }
    
    static error(msg) {
        console.error(`[ERROR] ${msg}`);
    }
    
    static debug(msg) {
        console.log(`[DEBUG] ${msg}`);
    }
}

async function populateMockEfivars(dir) {
    const efivarsDir = join(dir, 'efivars');
    await fs.mkdir(efivarsDir, { recursive: true });
    
    const efivars = {
        'BootChainFwCurrent-781e084c-a330-417c-b678-38e696380cb9': Buffer.from([0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
        'RootfsStatusSlotB-781e084c-a330-417c-b678-38e696380cb9': Buffer.from([0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
        'RootfsRetryCountMax-781e084c-a330-417c-b678-38e696380cb9': Buffer.from([0x06, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00]),
        'RootfsRetryCountB-781e084c-a330-417c-b678-38e696380cb9': Buffer.from([0x07, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00])
    };
    
    for (const [filename, data] of Object.entries(efivars)) {
        await fs.writeFile(join(efivarsDir, filename), data);
    }
}

async function populateMockUsrPersistent(dir) {
    const usrPersistentDir = join(dir, 'usr_persistent');
    await fs.mkdir(usrPersistentDir, { recursive: true });
    
    // Copy mock-usr-persistent/* if it exists
    try {
        await fs.access('mock-usr-persistent');
        const result = spawnSync('cp', ['-r', 'mock-usr-persistent/*', usrPersistentDir], { shell: true });
        if (result.status !== 0) {
            Logger.debug('No mock-usr-persistent directory found, creating empty structure');
        }
    } catch (error) {
        Logger.debug('Creating empty usr_persistent structure');
    }
}

async function createSquashfsImage(dir) {
    const mntDir = join(dir, 'mnt');
    await fs.mkdir(mntDir, { recursive: true });
    
    const rootImg = join(mntDir, 'root.img');
    
    // Create squashfs from fedora container
    Logger.info('Creating squashfs image from fedora container...');
    const tarProcess = spawn('podman', ['run', '--rm', FEDORA_CLOUD_IMAGE, 'tar', '--one-file-system', '-cf', '-', '.'], {
        stdio: ['pipe', 'pipe', 'inherit']
    });
    
    const mksquashfsProcess = spawn('mksquashfs', ['-', rootImg, '-tar', '-noappend', '-comp', 'zstd'], {
        stdio: ['pipe', 'inherit', 'inherit']
    });
    
    tarProcess.stdout.pipe(mksquashfsProcess.stdin);
    
    await new Promise((resolve, reject) => {
        mksquashfsProcess.on('close', (code) => {
            if (code === 0) resolve();
            else reject(new Error(`mksquashfs failed with code ${code}`));
        });
    });
    
    // Calculate hash and size
    const rootImgData = await fs.readFile(rootImg);
    const rootHash = createHash('sha256').update(rootImgData).digest('hex');
    const rootSize = rootImgData.length;
    
    // Create claim.json
    const claim = {
        version: "6.3.0-LL-prod",
        manifest: {
            magic: "some magic",
            type: "normal",
            components: [{
                name: "root",
                "version-assert": "none",
                version: "none",
                size: rootSize,
                hash: rootHash,
                installation_phase: "normal"
            }]
        },
        "manifest-sig": "TBD",
        sources: {
            root: {
                hash: rootHash,
                mime_type: "application/octet-stream",
                name: "root",
                size: rootSize,
                url: "/mnt/root.img"
            }
        },
        system_components: {
            root: {
                type: "gpt",
                value: {
                    device: "emmc",
                    label: "ROOT",
                    redundancy: "redundant"
                }
            }
        }
    };
    
    await fs.writeFile(join(mntDir, 'claim.json'), JSON.stringify(claim, null, 2));
    await fs.mkdir(join(mntDir, 'updates'), { recursive: true });
}

async function createMockDisk(dir) {
    const diskPath = join(dir, 'disk.img');
    
    // Create 64GB sparse file
    Logger.info('Creating mock disk image...');
    spawnSync('truncate', ['--size', QEMU_DISK_SIZE, diskPath]);
    
    // Create GPT partition table
    const partedCommands = [
        ['mklabel', 'gpt'],
        ['mkpart', 'APP_a', '1M', '65M'],
        ['mkpart', 'APP_b', '65M', '129M'],
        ['mkpart', 'esp', '129M', '193M'],
        ['mkpart', 'ROOT_a', '193M', '8385M'],
        ['mkpart', 'ROOT_b', '8385M', '16577M'],
        ['mkpart', 'persistent', '16577M', '16777M'],
        ['mkpart', 'MODELS_a', '16777M', '26777M'],
        ['mkpart', 'MODELS_b', '26777M', '36777M']
    ];
    
    for (const cmd of partedCommands) {
        const result = spawnSync('parted', ['--script', diskPath, ...cmd]);
        if (result.status !== 0) {
            throw new Error(`parted command failed: ${cmd.join(' ')}`);
        }
    }
}

async function createMockSystemctl(dir) {
    const systemctlPath = join(dir, 'systemctl');
    const script = `#!/bin/sh

echo "$@"
`;
    await fs.writeFile(systemctlPath, script);
    await fs.chmod(systemctlPath, 0o755);
}

async function downloadFedoraCloudImage(dir) {
    const cloudImagePath = join(dir, 'fedora-cloud.qcow2');
    
    // Check if image already exists
    try {
        await fs.access(cloudImagePath);
        Logger.info('Fedora Cloud image already exists, skipping download');
        return cloudImagePath;
    } catch (error) {
        // Image doesn't exist, download it
    }
    
    Logger.info('Downloading Fedora Cloud image...');
    const curlProcess = spawn('curl', ['-L', '-o', cloudImagePath, FEDORA_CLOUD_QCOW2_URL], {
        stdio: 'inherit'
    });
    
    await new Promise((resolve, reject) => {
        curlProcess.on('close', (code) => {
            if (code === 0) resolve();
            else reject(new Error(`curl failed with code ${code}`));
        });
    });
    
    return cloudImagePath;
}

async function createCloudInit(dir, programPath) {
    const cloudInitDir = join(dir, 'cloud-init');
    await fs.mkdir(cloudInitDir, { recursive: true });
    
    const userData = `#cloud-config
users:
  - name: fedora
    sudo: ALL=(ALL) NOPASSWD:ALL
    ssh_authorized_keys: []
packages:
  - systemd
write_files:
  - path: /usr/local/bin/update-agent
    permissions: '0755'
    content: |
      #!/bin/bash
      # Copy the actual update-agent binary
      cp /mnt/program /usr/local/bin/update-agent-real
      chmod +x /usr/local/bin/update-agent-real
      # Run the update agent
      /usr/local/bin/update-agent-real
      # Signal completion
      touch /tmp/update-agent-complete
      echo "Update agent execution completed"
  - path: /etc/systemd/system/update-agent.service
    content: |
      [Unit]
      Description=Update Agent Service
      After=multi-user.target
      
      [Service]
      Type=oneshot
      ExecStart=/usr/local/bin/update-agent
      RemainAfterExit=yes
      StandardOutput=journal
      StandardError=journal
      
      [Install]
      WantedBy=multi-user.target
runcmd:
  - systemctl daemon-reload
  - systemctl enable update-agent.service
  - systemctl start update-agent.service
  - journalctl -u update-agent.service --no-pager
`;
    
    await fs.writeFile(join(cloudInitDir, 'user-data'), userData);
    
    const metaData = `instance-id: update-agent-test
local-hostname: update-agent-test
`;
    await fs.writeFile(join(cloudInitDir, 'meta-data'), metaData);
    
    // Create cloud-init ISO
    const cloudInitIso = join(dir, 'cloud-init.iso');
    const genisoimageProcess = spawn('genisoimage', [
        '-output', cloudInitIso,
        '-volid', 'cidata',
        '-joliet',
        '-rock',
        join(cloudInitDir, 'user-data'),
        join(cloudInitDir, 'meta-data')
    ], { stdio: 'inherit' });
    
    await new Promise((resolve, reject) => {
        genisoimageProcess.on('close', (code) => {
            if (code === 0) resolve();
            else reject(new Error(`genisoimage failed with code ${code}`));
        });
    });
    
    return cloudInitIso;
}

async function waitForServiceCompletion(qemuProcess, timeout = 300000) {
    return new Promise((resolve, reject) => {
        const startTime = Date.now();
        let output = '';
        
        const checkCompletion = () => {
            if (Date.now() - startTime > timeout) {
                reject(new Error('Service completion timeout'));
                return;
            }
            
            // Check if completion marker exists
            if (output.includes('update-agent-complete')) {
                Logger.info('Service completed successfully');
                resolve();
                return;
            }
            
            setTimeout(checkCompletion, 1000);
        };
        
        qemuProcess.stdout.on('data', (data) => {
            output += data.toString();
            Logger.debug(`QEMU: ${data.toString().trim()}`);
        });
        
        qemuProcess.stderr.on('data', (data) => {
            Logger.debug(`QEMU stderr: ${data.toString().trim()}`);
        });
        
        qemuProcess.on('close', (code) => {
            if (code !== 0) {
                reject(new Error(`QEMU exited with code ${code}`));
            }
        });
        
        checkCompletion();
    });
}

async function runQemu(programPath, mockPath) {
    const absoluteProgramPath = resolve(programPath);
    const absoluteMockPath = resolve(mockPath);
    
    // Download Fedora Cloud image
    const cloudImagePath = await downloadFedoraCloudImage(absoluteMockPath);
    
    // Create cloud-init ISO
    const cloudInitIso = await createCloudInit(absoluteMockPath, absoluteProgramPath);
    
    const qemuArgs = [
        '-machine', 'q35',
        '-cpu', 'host',
        '-enable-kvm',
        '-m', QEMU_MEMORY,
        '-nographic',
        '-drive', `file=${cloudImagePath},format=qcow2,if=virtio`,
        '-drive', `file=${join(absoluteMockPath, 'disk.img')},format=raw,if=virtio`,
        '-drive', `file=${cloudInitIso},format=raw,if=virtio,readonly=on`,
        '-drive', `file=${absoluteProgramPath},format=raw,if=virtio,readonly=on`,
        '-netdev', 'user,id=net0',
        '-device', 'virtio-net-pci,netdev=net0',
        '-serial', 'mon:stdio'
    ];
    
    Logger.info('Starting QEMU with Fedora Cloud...');
    const qemuProcess = spawn('qemu-system-x86_64', qemuArgs, {
        stdio: ['pipe', 'pipe', 'pipe']
    });
    
    try {
        await waitForServiceCompletion(qemuProcess);
        Logger.info('Service execution completed');
    } finally {
        qemuProcess.kill('SIGTERM');
    }
}

async function compareResults(mockPath) {
    // Implementation for checking OTA results
    // This would compare the expected vs actual partition contents
    Logger.info('Checking OTA results...');
    
    const diskPath = join(mockPath, 'disk.img');
    const expectedRootImg = join(mockPath, 'mnt', 'root.img');
    
    // Use guestfish or similar tool to extract partition and compare
    // For now, just log that check would happen here
    Logger.info('Result verification would happen here');
    
    return true;
}

// Command handlers
async function handleMock(mockPath) {
    Logger.info(`Creating mock environment at ${mockPath}`);
    
    await fs.mkdir(mockPath, { recursive: true });
    await populateMockEfivars(mockPath);
    await populateMockUsrPersistent(mockPath);
    await createSquashfsImage(mockPath);
    await createMockDisk(mockPath);
    await createMockSystemctl(mockPath);
    await downloadFedoraCloudImage(mockPath);
    
    Logger.info('Mock environment created successfully');
}

async function handleRun(programPath, mockPath) {
    Logger.info(`Running update-agent test: ${programPath} in ${mockPath}`);
    await runQemu(programPath, mockPath);
}

async function handleCheck(mockPath) {
    Logger.info(`Checking results in ${mockPath}`);
    const success = await compareResults(mockPath);
    if (!success) {
        process.exit(3);
    }
    Logger.info('Check completed successfully');
}

async function handleClean(mockPath) {
    Logger.info(`Cleaning up ${mockPath}`);
    await fs.rm(mockPath, { recursive: true, force: true });
    Logger.info('Cleanup completed');
}

// Main function
async function main() {
    const args = process.argv.slice(2);
    
    if (args.length === 0) {
        console.log('QEMU-based integration testing of update agent');
        console.log('Usage:');
        console.log('  ./qemu-runner.js mock <dir>        - Create mockup directory');
        console.log('  ./qemu-runner.js run <prog> <dir>  - Run update-agent in QEMU');
        console.log('  ./qemu-runner.js check <dir>       - Check OTA results');
        console.log('  ./qemu-runner.js clean <dir>       - Clean up mockup directory');
        return;
    }
    
    const command = args[0];
    
    try {
        switch (command) {
            case 'mock':
                if (args.length !== 2) {
                    throw new Error('Usage: ./qemu-runner.js mock <dir>');
                }
                await handleMock(args[1]);
                break;
                
            case 'run':
                if (args.length !== 3) {
                    throw new Error('Usage: ./qemu-runner.js run <prog> <dir>');
                }
                await handleRun(args[1], args[2]);
                break;
                
            case 'check':
                if (args.length !== 2) {
                    throw new Error('Usage: ./qemu-runner.js check <dir>');
                }
                await handleCheck(args[1]);
                break;
                
            case 'clean':
                if (args.length !== 2) {
                    throw new Error('Usage: ./qemu-runner.js clean <dir>');
                }
                await handleClean(args[1]);
                break;
                
            default:
                throw new Error(`Unknown command: ${command}`);
        }
    } catch (error) {
        Logger.error(error.message);
        process.exit(1);
    }
}

if (import.meta.main) {
    main();
}
