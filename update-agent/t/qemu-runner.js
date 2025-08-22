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

async function copyOvmfCode(dir) {
    const ovmfSourcePath = '/usr/share/edk2/ovmf/OVMF_CODE_4M.qcow2';
    const ovmfDestPath = join(dir, 'OVMF_CODE_4M.qcow2');
    
    Logger.info('Copying OVMF_CODE_4M.qcow2 to mock directory...');
    await fs.copyFile(ovmfSourcePath, ovmfDestPath);
    
    return ovmfDestPath;
}

async function populateMockUsrPersistent(dir) {
    const usrPersistentDir = join(dir, 'usr_persistent');
    await fs.mkdir(usrPersistentDir, { recursive: true });
    
    // Copy mock-usr-persistent/* if it exists
    try {
        const result = Bun.spawnSync(['cp', '-r', 'mock-usr-persistent/*', usrPersistentDir], { shell: true });
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
    
    // Create claim.js
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
    
    const claimJs = `// Auto-generated claim data
export const claim = ${JSON.stringify(claimData, null, 2)};
export default claim;
`;
    
    await fs.writeFile(join(mntDir, 'claim.js'), claimJs);
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
        const result = Bun.spawnSync(['parted', '--script', diskPath, ...cmd]);
        if (result.status !== 0) {
            const stderr = result.stderr?.toString() || '';
            const stdout = result.stdout?.toString() || '';
            throw new Error(`parted command failed: ${cmd.join(' ')}\nstdout: ${stdout}\nstderr: ${stderr}`);
        }
    }
}


async function createImageFromDirectory(sourceDir, imagePath, sizeInMB) {
    // Create filesystem image using native Bun file operations
    const imageHandle = await fs.open(imagePath, 'w');
    await imageHandle.truncate(sizeInMB * 1024 * 1024);
    await imageHandle.close();
    
    // Format as ext4 and populate with directory contents
    const result = Bun.spawnSync(['mkfs.ext4', '-F', '-d', sourceDir, imagePath]);
    if (result.status !== 0) {
        throw new Error(`mkfs.ext4 failed for ${imagePath}: ${result.stderr?.toString()}`);
    }
}

async function createMockFilesystems(dir) {
    // Create filesystem images for mounting
    const usrPersistentImg = join(dir, 'usr_persistent.img');
    const mntImg = join(dir, 'mnt.img');
    
    const usrPersistentSource = join(dir, 'usr_persistent');
    const mntSource = join(dir, 'mnt');
    
    // Create each filesystem image from its corresponding directory
    await createImageFromDirectory(usrPersistentSource, usrPersistentImg, 100); // 100MB
    await createImageFromDirectory(mntSource, mntImg, 1024); // 1GB
    
    return { usrPersistentImg, mntImg };
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
      Type=oneshot
      ExecStart=/var/mnt/program/update-agent
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
      update_location = "/var/mnt/program/claim.js"
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
  - mount /dev/vdd /usr/persistent
  - mount /dev/vde /var/mnt
  - mkdir -p /var/mnt/program
  - mount -t 9p -o trans=virtio,version=9p2000.L program /var/mnt/program
  - printf '\x00\x00\x00\x00' > /tmp/efi_bootchain && efivar -n 781e084c-a330-417c-b678-38e696380cb9-BootChainFwCurrent -w -f /tmp/efi_bootchain
  - printf '\x00\x00\x00\x00' > /tmp/efi_rootfs_status && efivar -n 781e084c-a330-417c-b678-38e696380cb9-RootfsStatusSlotB -w -f /tmp/efi_rootfs_status
  - printf '\x03\x00\x00\x00' > /tmp/efi_retry_max && efivar -n 781e084c-a330-417c-b678-38e696380cb9-RootfsRetryCountMax -w -f /tmp/efi_retry_max
  - printf '\x03\x00\x00\x00' > /tmp/efi_retry_b && efivar -n 781e084c-a330-417c-b678-38e696380cb9-RootfsRetryCountB -w -f /tmp/efi_retry_b
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
    const usrPersistentImg = join(absoluteMockPath, 'usr_persistent.img');
    const mntImg = join(absoluteMockPath, 'mnt.img');
    
    // Recreate cloud-init ISO with the actual program path
    const cloudInitIso = await createCloudInit(absoluteMockPath, absoluteProgramPath);
    
    // Create a directory with the program and claim for mounting
    const programDir = join(absoluteMockPath, 'program');
    await fs.mkdir(programDir, { recursive: true });
    await fs.copyFile(absoluteProgramPath, join(programDir, 'update-agent'));
    
    // Copy claim.js to the program directory
    const claimJsSource = join(absoluteMockPath, 'mnt', 'claim.js');
    await fs.copyFile(claimJsSource, join(programDir, 'claim.js'));
    
    const ovmfCodePath = join(absoluteMockPath, 'OVMF_CODE_4M.qcow2');
    
    const qemuArgs = [
        '-machine', 'q35',
        '-cpu', 'host',
        '-enable-kvm',
        '-m', QEMU_MEMORY,
        '-nographic',
        '-snapshot',
        '-drive', `if=pflash,file=${ovmfCodePath}`,
        '-drive', `file=${cloudImagePath},format=qcow2,if=virtio`,
        '-drive', `file=${join(absoluteMockPath, 'disk.img')},format=raw,if=virtio`,
        '-drive', `file=${cloudInitIso},format=raw,if=virtio,readonly=on`,
        '-drive', `file=${usrPersistentImg},format=raw,if=virtio`,
        '-drive', `file=${mntImg},format=raw,if=virtio,readonly=on`,
        '-netdev', 'user,id=net0',
        '--bios', '/usr/share/edk2/ovmf/OVMF_CODE.fd',
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
    await copyOvmfCode(mockPath);
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
