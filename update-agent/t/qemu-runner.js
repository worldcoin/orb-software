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

const FEDORA_CLOUD_QCOW2_URL = 'https://mirror.us.mirhosting.net/fedora/linux/releases/42/Cloud/x86_64/images/Fedora-Cloud-Base-Generic-42-1.1.x86_64.qcow2';
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
        const result = spawnSync('cp', ['-r', 'mock-usr-persistent/*', usrPersistentDir], { shell: true });
        if (result.status !== 0) {
            Logger.debug('No mock-usr-persistent directory found, creating empty structure');
        }
    } catch (error) {
        Logger.debug('Creating empty usr_persistent structure');
    }
}

async function populateMockMnt(dir) {
    const mntDir = join(dir, 'mnt');
    await fs.mkdir(mntDir, { recursive: true });
    
    const rootImg = join(mntDir, 'root.img');
    const fedoraCloudImage = join(dir, 'fedora-cloud.qcow2');
    
    // Use the Fedora qcow2 image we already downloaded as the root image
    Logger.info('Using Fedora qcow2 image as root image...');
    await fs.copyFile(fedoraCloudImage, rootImg);
    
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
    
    // Create 64GB sparse file using native Bun file operations
    Logger.info('Creating mock disk image...');
    const diskSize = 64 * 1024 * 1024 * 1024; // 64GB in bytes
    const fileHandle = await fs.open(diskPath, 'w');
    await fileHandle.truncate(diskSize);
    await fileHandle.close();
    
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


async function createImageFromDirectory(sourceDir, imagePath, sizeInMB) {
    // Create filesystem image using native Bun file operations
    const imageHandle = await fs.open(imagePath, 'w');
    await imageHandle.truncate(sizeInMB * 1024 * 1024);
    await imageHandle.close();
    
    // Format as ext4 and populate with directory contents
    const result = spawnSync('mkfs.ext4', ['-F', '-d', sourceDir, imagePath]);
    if (result.status !== 0) {
        throw new Error(`mkfs.ext4 failed for ${imagePath}: ${result.stderr?.toString()}`);
    }
}

async function createMockFilesystems(dir) {
    // Create filesystem images for mounting
    const efivarsImg = join(dir, 'efivars.img');
    const usrPersistentImg = join(dir, 'usr_persistent.img');
    const mntImg = join(dir, 'mnt.img');
    
    const efivarsSource = join(dir, 'efivars');
    const usrPersistentSource = join(dir, 'usr_persistent');
    const mntSource = join(dir, 'mnt');
    
    // Create each filesystem image from its corresponding directory
    await createImageFromDirectory(efivarsSource, efivarsImg, 10); // 10MB
    await createImageFromDirectory(usrPersistentSource, usrPersistentImg, 100); // 100MB
    await createImageFromDirectory(mntSource, mntImg, 1024); // 1GB
    
    return { efivarsImg, usrPersistentImg, mntImg };
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
    
    try {
        const response = await fetch(FEDORA_CLOUD_QCOW2_URL);
        
        if (!response.ok) {
            throw new Error(`HTTP error! status: ${response.status}`);
        }
        
        // Stream the response to file
        const fileHandle = await fs.open(cloudImagePath, 'w');
        const writer = fileHandle.createWriteStream();
        
        let downloadedBytes = 0;
        const contentLength = parseInt(response.headers.get('content-length') || '0');
        
        for await (const chunk of response.body) {
            writer.write(chunk);
            downloadedBytes += chunk.length;
            
            if (contentLength > 0) {
                const progress = ((downloadedBytes / contentLength) * 100).toFixed(1);
                Logger.info(`Download progress: ${progress}% (${downloadedBytes}/${contentLength} bytes)`);
            }
        }
        
        await writer.end();
        await fileHandle.close();
        
        Logger.info('Fedora Cloud image downloaded successfully');
    } catch (error) {
        throw new Error(`Failed to download Fedora Cloud image: ${error.message}`);
    }
    
    return cloudImagePath;
}

async function createCloudInit(dir, programPath) {
    const cloudInitDir = join(dir, 'cloud-init');
    await fs.mkdir(cloudInitDir, { recursive: true });
    
    const userData = `#cloud-config
package_update: false
package_upgrade: false
users:
  - name: fedora
    sudo: ALL=(ALL) NOPASSWD:ALL
    ssh_authorized_keys: []
  - name: worldcoin
    sudo: ALL=(ALL) NOPASSWD:ALL
    lock_passwd: false
ssh_pwauth: true
chpasswd:
  list: |
    worldcoin:dontshipdevorbs
  expire: false
write_files:
  - path: /etc/systemd/system/update-agent.service
    content: |
      [Unit]
      Description=Update Agent Service
      After=cloud-init.target
      
      [Service]
      Type=oneshot
      ExecStart=/var/mnt/program/update-agent
      RemainAfterExit=no
      StandardOutput=journal+kmsg
      StandardError=journal+kmsg
      Environment=RUST_BACKTRACE=1
      
      [Install]
      WantedBy=multi-user.target
  - path: /etc/orb_update_agent.conf
    content: |
      versions = "/usr/persistent/versions.json"
      components = "/usr/persistent/components.json"
      cacert = "/etc/ssl/worldcoin-staging-ota.pem"
      clientkey = "/etc/ssl/private/worldcoin-staging-ota-identity.key"
      update_location = "/mnt/claim.json"
      workspace = "/mnt/scratch"
      downloads = "/mnt/scratch/downloads"
      download_delay = 0
      recovery = false
      nodbus = false
      noupdate = false
      skip_version_asserts = true
      verify_manifest_signature_against = "stage"
      id = "qemu-mock"
  - path: /etc/os-release
    content: |
      NAME="Orb OS"
      VERSION="6.3.0-LL-prod"
      ID=orb
      VERSION_ID="6.3.0"
      PRETTY_NAME="Orb OS 6.3.0-LL-prod"
      ORB_OS_RELEASE_TYPE="stage"
runcmd:
  - mkdir -p /sys/firmware/efi/efivars
  - mkdir -p /usr/persistent
  - mkdir -p /var/mnt
  - mkdir -p /mnt
  - mount /dev/vdb /mnt
  - mount /dev/vdc /sys/firmware/efi/efivars
  - mount /dev/vdd /usr/persistent
  - mount /dev/vde /var/mnt
  - mkdir -p /var/mnt/program
  - mount -t 9p -o trans=virtio,version=9p2000.L program /var/mnt/program
  - systemctl daemon-reload
  - systemctl start update-agent.service
  - journalctl -fu update-agent.service
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
            if (output.includes('Finished update-agent.service')) {
                Logger.info('Service completed successfully');
                resolve();
                return;
            }
            
            setTimeout(checkCompletion, 1000);
        };
        
        // Forward stdin to QEMU process
        process.stdin.on('data', (data) => {
            qemuProcess.stdin.write(data);
        });
        
        qemuProcess.stdout.on('data', (data) => {
            const dataStr = data.toString();
            output += dataStr;
            process.stdout.write(dataStr);
        });
        
        qemuProcess.stderr.on('data', (data) => {
            process.stderr.write(data.toString());
        });
        
        qemuProcess.onExit((proc, exitCode, signalCode, error) => {
            if (exitCode !== 0) {
                reject(new Error(`QEMU exited with code ${code}`));
            }
            if (signalCode !== 0) {
                reject(new Error(`QEMU exited with signal ${code}`));
            }
        });
        
        checkCompletion();
    });
}

async function runQemu(programPath, mockPath) {
    const absoluteProgramPath = resolve(programPath);
    const absoluteMockPath = resolve(mockPath);
    
    // Use pre-created files from mock step
    const cloudImagePath = join(absoluteMockPath, 'fedora-cloud.qcow2');
    const cloudInitIso = join(absoluteMockPath, 'cloud-init.iso');
    const efivarsImg = join(absoluteMockPath, 'efivars.img');
    const usrPersistentImg = join(absoluteMockPath, 'usr_persistent.img');
    const mntImg = join(absoluteMockPath, 'mnt.img');
    
    // Create a directory with the program for mounting
    const programDir = join(absoluteMockPath, 'program');
    await fs.mkdir(programDir, { recursive: true });
    await fs.copyFile(absoluteProgramPath, join(programDir, 'update-agent'));
    
    const qemuArgs = [
        '-machine', 'q35',
        '-cpu', 'host',
        '-enable-kvm',
        '-m', QEMU_MEMORY,
        '-nographic',
        '-snapshot',
        '-drive', `file=${cloudImagePath},format=qcow2,if=virtio`,
        '-drive', `file=${join(absoluteMockPath, 'disk.img')},format=raw,if=virtio`,
        '-drive', `file=${cloudInitIso},format=raw,if=virtio,readonly=on`,
        '-drive', `file=${efivarsImg},format=raw,if=virtio`,
        '-drive', `file=${usrPersistentImg},format=raw,if=virtio`,
        '-drive', `file=${mntImg},format=raw,if=virtio,readonly=on`,
        '-netdev', 'user,id=net0',
        '-device', 'virtio-net-pci,netdev=net0',
        '-virtfs', `local,path=${programDir},mount_tag=program,security_model=passthrough,id=program`,
        '-serial', 'mon:stdio'
    ];
    
    Logger.info('Starting QEMU with Fedora Cloud...');
    const qemuProcess = spawn('qemu-system-x86_64', qemuArgs, {
        stdio: ['pipe', 'pipe', 'pipe']
    });
    
    // Enable raw mode for stdin to pass through key presses
    if (process.stdin.isTTY) {
        process.stdin.setRawMode(true);
    }
    
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
    
    // Use guestfish or similar tool to extract partition and compare
    // For now, just log that check would happen here
    Logger.info('Result verification would happen here');
    
    return true;
}

// Command handlers
async function handleMock(mockPath) {
    Logger.info(`Creating mock environment at ${mockPath}`);
    
    await fs.mkdir(mockPath, { recursive: true });
    await downloadFedoraCloudImage(mockPath);
    await populateMockEfivars(mockPath);
    await populateMockUsrPersistent(mockPath);
    await populateMockMnt(mockPath);
    await createMockDisk(mockPath);

    // Create cloud-init ISO (without program path since it's not available yet)
    const cloudInitIso = await createCloudInit(mockPath, null);
    
    // Create filesystem images
    await createMockFilesystems(mockPath);
    
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
