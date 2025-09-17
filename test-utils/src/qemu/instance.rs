use cmd_lib::{run_cmd, run_fun};
use regex::Regex;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Debug)]
pub struct QemuInstance {
    id: String,
    ssh_port: u16,
}

impl Drop for QemuInstance {
    fn drop(&mut self) {
        self.kill();
    }
}

impl QemuInstance {
    pub fn start(working_dir: impl AsRef<Path>, img_path: impl AsRef<Path>) -> Self {
        let working_dir = working_dir.as_ref();
        let img_path = img_path.as_ref().to_str().unwrap();
        let id = Uuid::new_v4().to_string();

        let tmp_overlay_path = working_dir
            .join(format!("{id}.qcow2"))
            .to_string_lossy()
            .to_string();

        let qmp = working_dir
            .join(format!("{id}.qmp"))
            .to_string_lossy()
            .to_string();

        run_cmd! {
            qemu-img create -f qcow2 -F qcow2 -b $img_path $tmp_overlay_path;

            qemu-system-x86_64
                -machine q35 -cpu host -enable-kvm -m 2048 -daemonize -display none
                -name guest=$id,process=qemu-$id
                -qmp unix:$qmp,server,nowait
                -drive file=$tmp_overlay_path,if=virtio,format=qcow2
                -object rng-random,filename=/dev/urandom,id=rng0
                -device virtio-rng-pci,rng=rng0
                -nic user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:0-:22,ipv6=off
        }
        .unwrap();

        println!("getting ssh port");
        let ssh_port = qmp_ssh_port(&qmp);

        println!("checking if guest is listening on ssh port {ssh_port}");
        let start = Instant::now();
        while Instant::now() - start < Duration::from_secs(60) {
            let result = run_cmd! {
                ssh -p $ssh_port -o ConnectTimeout=5 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o GlobalKnownHostsFile=/dev/null
                    worldcoin@127.0.0.1 echo hello world
            };

            if result.is_ok() {
                println!("guest ready");
                return Self { id, ssh_port };
            }

            thread::sleep(std::time::Duration::from_millis(2_000));
        }

        panic!("timed out when trying to reach vm through ssh on port {ssh_port}")
    }

    fn kill(&self) {
        let id = &self.id;
        run_cmd!(pkill -f process=qemu-$id).unwrap();
    }

    pub fn copy(&self, host_path: impl AsRef<Path>, guest_path: impl AsRef<Path>) {
        let host_path = host_path.as_ref().to_str().unwrap();
        let guest_path = guest_path.as_ref().to_str().unwrap();
        let port = self.ssh_port;

        run_cmd!{
            scp -O -P $port -o ConnectTimeout=5 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o GlobalKnownHostsFile=/dev/null
                $host_path worldcoin@127.0.0.1:$guest_path
        }.unwrap();
    }

    pub fn run(&self, cmd: &str) -> String {
        let port = self.ssh_port;
        run_fun! {
            ssh -p $port -o ConnectTimeout=5 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o GlobalKnownHostsFile=/dev/null
                worldcoin@127.0.0.1 $cmd
        }.unwrap()
    }
}

pub fn qmp_ssh_port(qmp_path: &str) -> u16 {
    let stream = UnixStream::connect(qmp_path).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);

    let payload = concat!(
        r#"{"execute":"qmp_capabilities"}"#,
        "\n",
        r#"{"execute":"human-monitor-command","arguments":{"command-line":"info usernet"} }"#,
        "\n"
    );

    writer.write_all(payload.as_bytes()).unwrap();
    writer.flush().unwrap();

    let re =
        Regex::new(r#"127\.0\.0\.1[ :]+(\d+)(?:\s*->\s*|\s+)[0-9.]+\s+22"#).unwrap();

    let mut line = String::new();
    while reader.read_line(&mut line).unwrap() > 0 {
        if let Some(c) = re.captures(&line) {
            return c[1].parse().unwrap();
        }
        line.clear();
    }

    panic!("hostfwd :22 not found")
}
