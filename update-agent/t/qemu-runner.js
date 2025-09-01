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

import { $ } from "bun";
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

function detectOvmfPaths() {
    try {
        // Check if we're on Ubuntu (Debian-based)
        const osRelease = require('fs').readFileSync('/etc/os-release', 'utf8');
        if (osRelease.includes('ubuntu') || osRelease.includes('Ubuntu')) {
            return {
                codePath: '/usr/share/OVMF/OVMF_CODE_4M.fd',
                varsPath: '/usr/share/OVMF/OVMF_VARS_4M.fd'
            };
        } else {
            // Default to Fedora/RHEL path
            return {
                codePath: '/usr/share/edk2/ovmf/OVMF_CODE_4M.qcow2',
                varsPath: '/usr/share/edk2/ovmf/OVMF_VARS_4M.qcow2'
            };
        }
    } catch (error) {
        // Fallback to Fedora path if we can't read os-release
        return {
            codePath: '/usr/share/edk2/ovmf/OVMF_CODE_4M.qcow2',
            varsPath: '/usr/share/edk2/ovmf/OVMF_VARS_4M.qcow2'
        };
    }
}

async function copyOvmfFiles(dir) {
    const ovmfPaths = detectOvmfPaths();
    
    // Determine file extensions based on source format
    const isUbuntu = ovmfPaths.codePath.includes('OVMF') && ovmfPaths.codePath.endsWith('.fd');
    const codeExt = isUbuntu ? '.fd' : '.qcow2';
    const varsExt = isUbuntu ? '.fd' : '.qcow2';
    
    const ovmfCodeDestPath = join(dir, `OVMF_CODE_4M${codeExt}`);
    const ovmfVarsDestPath = join(dir, `OVMF_VARS_4M${varsExt}`);
    
    Logger.info(`Copying OVMF code from ${ovmfPaths.codePath} to mock directory...`);
    await fs.copyFile(ovmfPaths.codePath, ovmfCodeDestPath);
    
    Logger.info(`Copying OVMF vars from ${ovmfPaths.varsPath} to mock directory...`);
    await fs.copyFile(ovmfPaths.varsPath, ovmfVarsDestPath);
    
    return { ovmfCodeDestPath, ovmfVarsDestPath, isUbuntu };
}

async function createMockUsrPersistent(dir) {
    const usrPersistentDir = join(dir, 'usr_persistent');
    await fs.mkdir(usrPersistentDir, { recursive: true });

    await $`cp -r mock-usr-persistent/* ${usrPersistentDir}`;

    // Create filesystem image directly
    const usrPersistentImg = join(dir, 'usr_persistent.img');
    await createImageFromDirectory(usrPersistentDir, usrPersistentImg, 100); // 100MB
    
    return usrPersistentImg;
}

async function createClaimJson(path){

}

async function populateMockMnt(dir) {
    const mntDir = join(dir, 'mnt');
    await fs.mkdir(mntDir, { recursive: true });
    
    const rootImg = join(mntDir, 'root.img');
    const fedoraCloudImage = join(dir, 'fedora-cloud.qcow2');
    
    // Convert the Fedora qcow2 image to raw format for the root image
    Logger.info('Converting Fedora qcow2 image to raw format...');
    const qemuImgResult = Bun.spawnSync(['qemu-img', 'convert', '-f', 'qcow2', '-O', 'raw', fedoraCloudImage, rootImg]);
    if (!qemuImgResult.success) {
        throw new Error(`Failed to convert qcow2 to raw: ${qemuImgResult.stderr?.toString()}`);
    }
    
    Logger.info('Calculating hash of root.img...');
    // Calculate hash and size using chunked reads
    const rootImgHandle = await fs.open(rootImg, 'r');
    const rootImgStats = await rootImgHandle.stat();
    const rootSize = rootImgStats.size;
    
    const hasher = new Bun.CryptoHasher('sha256');
    const chunkSize = 64 * 1024 * 1024; // 64MB chunks
    let bytesRemaining = rootSize;
    let currentOffset = 0;
    
    while (bytesRemaining > 0) {
        const currentChunkSize = Math.min(chunkSize, bytesRemaining);
        const buffer = Buffer.alloc(currentChunkSize);
        
        await rootImgHandle.read(buffer, 0, currentChunkSize, currentOffset);
        hasher.update(buffer);
        
        bytesRemaining -= currentChunkSize;
        currentOffset += currentChunkSize;
    }
    
    await rootImgHandle.close();
    const rootHash = hasher.digest('hex');
    
    const claimData = {
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
    
    const claimJs = JSON.stringify(claimData, null, 2);
    
    await fs.writeFile(join(mntDir, 'claim.json'), claimJs);
    await fs.mkdir(join(mntDir, 'updates'), { recursive: true });
}


async function createMockDisk(dir, persistent) {
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
        const result = Bun.spawnSync(['parted', '--script', diskPath, ...cmd]);
        if (!result.success) {
            const stderr = result.stderr?.toString() || '';
            const stdout = result.stdout?.toString() || '';
            throw new Error(`parted command failed: ${cmd.join(' ')}\nstdout: ${stdout}\nstderr: ${stderr}`);
        }
    }

    // Find offset of persistent partition in the disk image
    const diskInfoResult = Bun.spawnSync(['parted', '--json', '--script', diskPath, 'unit B print']);
    if (!diskInfoResult.success) {
        throw new Error(`Failed to get partition info: ${diskInfoResult.stderr?.toString()}`);
    }
    
    const diskInfo = JSON.parse(diskInfoResult.stdout.toString());
    let start = null;
    
    for (const partition of diskInfo.disk.partitions) {
        if (partition.name === 'persistent') {
            // Remove 'B' suffix from start offset and convert to number
            start = parseInt(partition.start.replace('B', ''));
            break;
        }
    }
    
    if (start === null) {
        throw new Error('Could not find persistent partition');
    }
    
    // Copy persistent file content into the partition at the calculated offset
    Logger.info(`Copying persistent file content to disk at offset ${start}`);
        
    // Read the persistent file content
    const persistentData = await fs.readFile(persistent);
        
    // Open disk image for writing at the specific offset
    const diskHandle = await fs.open(diskPath, 'r+');
    await diskHandle.write(persistentData, 0, persistentData.length, start);
    await diskHandle.close();
        
    Logger.info(`Copied ${persistentData.length} bytes to persistent partition`);
}


async function createImageFromDirectory(sourceDir, imagePath, sizeInMB) {
    // Create filesystem image using native Bun file operations
    const imageHandle = await fs.open(imagePath, 'w');
    await imageHandle.truncate(sizeInMB * 1024 * 1024);
    await imageHandle.close();
    
    // Format as ext4 and populate with directory contents
    const result = Bun.spawnSync(['mkfs.ext4', '-F', '-d', sourceDir, imagePath]);
    if (!result.success) {
        throw new Error(`mkfs.ext4 failed for ${imagePath}: ${result.stderr?.toString()}`);
    }
}

async function createMockFilesystems(dir) {
    // Create filesystem images for mounting
    const mntImg = join(dir, 'mnt.img');
    const mntSource = join(dir, 'mnt');

    // Create mnt filesystem image
    await createImageFromDirectory(mntSource, mntImg, 20480); // 20GB

    return { mntImg };
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
            
            if (contentLength > 0 && process.stdout.isTTY) {
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
package_update: true
package_upgrade: false
packages:
  - efivar
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
  - path: /etc/systemd/system/worldcoin-update-agent.service
    content: |
      [Unit]
      Description=Update Agent Service

      [Service]
      Type=simple
      ExecStart=/mnt/program/update-agent --nodbus
      RemainAfterExit=no
      StandardOutput=journal+console
      StandardError=journal+console
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
      ORB_OS_RELEASE_TYPE="dev"
      ORB_OS_PLATFORM_TYPE="diamond"
      ORB_OS_EXPECTED_MAIN_MCU_VERSION=v3.0.17
      ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.0.17
runcmd:
  - mkdir -p /usr/persistent
  - mount /dev/disk/by-partlabel/persistent /usr/persistent
  - mount /dev/vdd /mnt
  - mkdir -p /mnt/program
  - mount -t 9p -o trans=virtio,version=9p2000.L program /mnt/program
  - printf '\\x00\\x00\\x00\\x00' > /tmp/efi_bootchain && efivar -n 781e084c-a330-417c-b678-38e696380cb9-BootChainFwCurrent -w -f /tmp/efi_bootchain
  - printf '\\x00\\x00\\x00\\x00' > /tmp/efi_rootfs_status && efivar -n 781e084c-a330-417c-b678-38e696380cb9-RootfsStatusSlotB -w -f /tmp/efi_rootfs_status
  - printf '\\x03\\x00\\x00\\x00' > /tmp/efi_retry_max && efivar -n 781e084c-a330-417c-b678-38e696380cb9-RootfsRetryCountMax -w -f /tmp/efi_retry_max
  - printf '\\x03\\x00\\x00\\x00' > /tmp/efi_retry_b && efivar -n 781e084c-a330-417c-b678-38e696380cb9-RootfsRetryCountB -w -f /tmp/efi_retry_b
  - systemctl daemon-reload
  - setenforce 0
  - systemctl start worldcoin-update-agent.service
  - journalctl -fu worldcoin-update-agent.service
`;
    
    await fs.writeFile(join(cloudInitDir, 'user-data'), userData);
    
    const metaData = `instance-id: update-agent-test
local-hostname: update-agent-test
`;
    await fs.writeFile(join(cloudInitDir, 'meta-data'), metaData);
    
    // Create cloud-init ISO
    const cloudInitIso = join(dir, 'cloud-init.iso');
    const genisoimageProcess = Bun.spawnSync(['genisoimage',
        '-output', cloudInitIso,
        '-volid', 'cidata',
        '-joliet',
        '-rock',
        join(cloudInitDir, 'user-data'),
        join(cloudInitDir, 'meta-data')
                                             ], { stdout: 'inherit', stderr: 'inherit'});
    
    if (!genisoimageProcess.success) {
        throw new Error(`genisoimage failed with code ${genisoimageProcess.status}`);
    }
    
    return cloudInitIso;
}

async function waitForServiceCompletion(qemuProcess) {
    // Happy path: wait for service completion
    const happyPath = new Promise(async (resolve, reject) => {
        let output = '';
        
        // Forward stdin to QEMU process
        process.stdin.on('data', (data) => {
            qemuProcess.stdin.write(data);
        });
        
        // Read from stdout using ReadableStream
        const stdoutReader = qemuProcess.stdout.getReader();
        const stderrReader = qemuProcess.stderr.getReader();
        
        // Process stdout stream
        const processStdout = async () => {
            try {
                while (true) {
                    const { done, value } = await stdoutReader.read();
                    if (done) break;
                    
                    const dataStr = new TextDecoder().decode(value);
                    output += dataStr;
                    process.stdout.write(dataStr);
                    
                    // Check if completion marker exists
                    if (output.includes('Finished worldcoin-update-agent.service')) {
                        Logger.info('Service completed successfully');
                        resolve('service-completed');
                        return;
                    }
                }
            } catch (error) {
                reject(error);
            }
        };
        
        // Process stderr stream
        const processStderr = async () => {
            try {
                while (true) {
                    const { done, value } = await stderrReader.read();
                    if (done) break;
                    
                    const dataStr = new TextDecoder().decode(value);
                    process.stderr.write(dataStr);
                }
            } catch (error) {
                // Stderr errors are non-fatal
                Logger.debug(`stderr read error: ${error.message}`);
            }
        };
        
        // Start both stream processors
        Promise.all([processStdout(), processStderr()]).catch(reject);
    });

    // Wait for either the service to complete or the process to exit
    await Promise.any([happyPath, qemuProcess.exited]);
}

async function runQemu(programPath, mockPath) {
    const absoluteProgramPath = resolve(programPath);
    const absoluteMockPath = resolve(mockPath);
    
    // Use pre-created files from mock step
    const cloudImagePath = join(absoluteMockPath, 'fedora-cloud.qcow2');
    const mntImg = join(absoluteMockPath, 'mnt.img');
    
    // Recreate cloud-init ISO with the actual program path
    const cloudInitIso = await createCloudInit(absoluteMockPath, absoluteProgramPath);
    
    // Create a directory with the program and claim for mounting
    const programDir = join(absoluteMockPath, 'program');
    await fs.mkdir(programDir, { recursive: true });
    await fs.copyFile(absoluteProgramPath, join(programDir, 'update-agent'));
    
    // Detect if we're using Ubuntu format files
    const ovmfCodePath = join(absoluteMockPath, 'OVMF_CODE_4M.fd');
    const ovmfVarsPath = join(absoluteMockPath, 'OVMF_VARS_4M.fd');
    const ovmfCodePathQcow2 = join(absoluteMockPath, 'OVMF_CODE_4M.qcow2');
    const ovmfVarsPathQcow2 = join(absoluteMockPath, 'OVMF_VARS_4M.qcow2');
    
    let actualCodePath, actualVarsPath, ovmfFormat;
    
    try {
        await fs.access(ovmfCodePath);
        actualCodePath = ovmfCodePath;
        actualVarsPath = ovmfVarsPath;
        ovmfFormat = 'raw';
    } catch {
        actualCodePath = ovmfCodePathQcow2;
        actualVarsPath = ovmfVarsPathQcow2;
        ovmfFormat = 'qcow2';
    }
    
    const qemuArgs = [
        '-machine', 'q35',
        '-cpu', 'host',
        '-enable-kvm',
        '-m', QEMU_MEMORY,
        '-nographic',
        '-drive', `if=pflash,format=${ovmfFormat},file=${actualCodePath},readonly=on`,
        '-drive', `if=pflash,format=${ovmfFormat},file=${actualVarsPath}`,
        '-drive', `file=${cloudImagePath},format=qcow2,if=virtio,snapshot=on`,
        '-drive', `file=${join(absoluteMockPath, 'disk.img')},format=raw,if=virtio`,
        '-drive', `file=${cloudInitIso},format=raw,if=virtio,readonly=on`,
        '-drive', `file=${mntImg},format=raw,if=virtio,snapshot=on`,
        '-netdev', 'user,id=net0',
        '-device', 'virtio-net-pci,netdev=net0',
        '-virtfs', `local,path=${programDir},mount_tag=program,security_model=passthrough,id=program`,
        '-serial', 'mon:stdio'
    ];
    
    Logger.info('Starting QEMU with Fedora Cloud...');
    const qemuProcess = Bun.spawn(['qemu-system-x86_64', ...qemuArgs], {
        stdio: ['pipe', 'pipe', 'pipe']//,
        //timeout: 300000
    });
    
    // Enable raw mode for stdin to pass through key presses
    if (process.stdin.isTTY) {
        process.stdin.setRawMode(true);
    }
    
    try {
        await waitForServiceCompletion(qemuProcess);
        Logger.info('Service execution completed');
    } finally {
        if (process.stdin.isTTY) {
            process.stdin.setRawMode(false);
        }
        await qemuProcess.kill('SIGTERM');
    }
}

async function compareResults(mockPath) {
    Logger.info('Checking OTA results...');
    
    const diskPath = join(mockPath, 'disk.img');
    const fedoraCloudPath = join(mockPath, 'mnt/root.img');
    
    // Find offset of ROOT_b partition in the disk image
    const diskInfoResult = Bun.spawnSync(['parted', '--json', '--script', diskPath, 'unit B print']);
    if (!diskInfoResult.success) {
        throw new Error(`Failed to get partition info: ${diskInfoResult.stderr?.toString()}`);
    }
    
    const diskInfo = JSON.parse(diskInfoResult.stdout.toString());
    let rootBStart = null;
    let rootBSize = null;
    
    for (const partition of diskInfo.disk.partitions) {
        if (partition.name === 'ROOT_b') {
            // Remove 'B' suffix from start offset and convert to number
            rootBStart = parseInt(partition.start.replace('B', ''));
            rootBSize = parseInt(partition.size.replace('B', ''));
            break;
        }
    }
    
    if (rootBStart === null) {
        throw new Error('Could not find ROOT_b partition');
    }
    
    Logger.info(`Found ROOT_b partition at offset ${rootBStart}, size ${rootBSize} bytes`);
    
    // Get size of root.img for comparison
    const fedoraStats = await fs.stat(fedoraCloudPath);
    const rootImgSize = fedoraStats.size;
    
    Logger.info(`root.img size: ${rootImgSize} bytes`);
    
    // Compare ROOT_b partition with fedora-cloud.qcow2 chunk by chunk
    const diskHandle = await fs.open(diskPath, 'r');
    const fedoraHandle = await fs.open(fedoraCloudPath, 'r');
    
    const chunkSize = 64 * 1024 * 1024; // 64MB chunks
    let bytesRemaining = rootImgSize;
    let currentDiskOffset = rootBStart;
    let currentFedoraOffset = 0;
    
    try {
        while (bytesRemaining > 0) {
            const currentChunkSize = Math.min(chunkSize, bytesRemaining);
            
            // Read chunk from ROOT_b partition
            const diskBuffer = Buffer.alloc(currentChunkSize);
            await diskHandle.read(diskBuffer, 0, currentChunkSize, currentDiskOffset);
            
            // Read chunk from fedora-cloud.qcow2
            const fedoraBuffer = Buffer.alloc(currentChunkSize);
            await fedoraHandle.read(fedoraBuffer, 0, currentChunkSize, currentFedoraOffset);
            
            // Compare chunks
            if (!diskBuffer.equals(fedoraBuffer)) {
                throw new Error(`ROOT_b partition content does NOT match root.img at offset ${currentDiskOffset - rootBStart}`);
            }
            
            bytesRemaining -= currentChunkSize;
            currentDiskOffset += currentChunkSize;
            currentFedoraOffset += currentChunkSize;
        }
        
        Logger.info('✓ ROOT_b partition content matches root.img');
        return true;
        
    } finally {
        await diskHandle.close();
        await fedoraHandle.close();
    }
}

// Command handlers
async function handleMock(mockPath) {
    Logger.info(`Creating mock environment at ${mockPath}`);
    
    await fs.mkdir(mockPath, { recursive: true });
    await downloadFedoraCloudImage(mockPath);
    const { ovmfCodeDestPath, ovmfVarsDestPath, isUbuntu } = await copyOvmfFiles(mockPath);
    const persistent = await createMockUsrPersistent(mockPath);
    await populateMockMnt(mockPath);
    await createMockDisk(mockPath, persistent);

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
        throw error
        process.exit(1);
    }
    process.exit(0);
}

if (import.meta.main) {
    main();
}
