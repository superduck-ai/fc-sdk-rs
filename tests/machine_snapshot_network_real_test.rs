#![cfg(target_os = "linux")]

mod real_vm_support;

use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use firecracker_sdk::{
    AsyncResultExt, Config, IPConfiguration, MachineConfiguration, NetworkInterface,
    NetworkInterfaces, StaticNetworkConfiguration, VMCommandBuilder, new_machine,
    with_memory_backend, with_process_runner, with_read_only, with_snapshot,
};
use ipnet::Ipv4Net;

static NEXT_NETWORK_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Debug, Clone)]
enum GuestFilesystem {
    Initramfs(PathBuf),
    Rootfs(PathBuf),
}

unsafe extern "C" {
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
}

fn is_root() -> bool {
    unsafe { libc_geteuid() == 0 }
}

fn command_in_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}

fn firecracker_binary() -> &'static str {
    "/data/firecracker"
}

fn kernel_path() -> &'static str {
    "/data_jfs/fc-kernels/vmlinux-6.1.158/vmlinux.bin"
}

fn busybox_source() -> &'static str {
    "/data_jfs/fc-busybox/1.36.1/amd64/busybox"
}

fn assets_available() -> bool {
    is_root()
        && Path::new("/dev/kvm").exists()
        && Path::new("/dev/net/tun").exists()
        && Path::new(firecracker_binary()).exists()
        && Path::new(kernel_path()).exists()
        && command_in_path("ip")
        && ((Path::new(busybox_source()).exists()
            && command_in_path("cpio")
            && command_in_path("find")
            && command_in_path("mknod"))
            || real_vm_support::python_service_rootfs_available())
}

fn firecracker_command(socket_path: &Path, vmid: &str) -> firecracker_sdk::VMCommand {
    VMCommandBuilder::default()
        .with_bin(firecracker_binary())
        .with_socket_path(socket_path.display().to_string())
        .with_args(["--id", vmid, "--no-seccomp"])
        .build()
}

#[derive(Debug, Clone)]
struct StaticTapNetwork {
    tap_name: String,
    host_cidr: String,
    host_ip: Ipv4Addr,
    guest_cidr: Ipv4Net,
    guest_ip: Ipv4Addr,
    guest_mac: String,
    http_port: u16,
}

impl StaticTapNetwork {
    fn allocate() -> Self {
        let unique = NEXT_NETWORK_ID.fetch_add(1, Ordering::SeqCst);
        let subnet_octet = ((std::process::id() + unique) % 250 + 1) as u8;
        let host_ip = Ipv4Addr::new(172, 31, subnet_octet, 1);
        let guest_ip = Ipv4Addr::new(172, 31, subnet_octet, 2);

        Self {
            tap_name: format!("fcsnp{unique:03}"),
            host_cidr: format!("{host_ip}/30"),
            host_ip,
            guest_cidr: format!("{guest_ip}/30").parse::<Ipv4Net>().unwrap(),
            guest_ip,
            guest_mac: format!("06:00:00:00:{subnet_octet:02x}:{unique:02x}"),
            http_port: 8080,
        }
    }

    fn network_interface(&self) -> NetworkInterface {
        NetworkInterface {
            static_configuration: Some(
                StaticNetworkConfiguration::new(&self.tap_name)
                    .with_mac_address(self.guest_mac.clone()),
            ),
            ..NetworkInterface::default()
        }
    }

    fn network_interface_with_static_ip(&self) -> NetworkInterface {
        NetworkInterface {
            static_configuration: Some(
                StaticNetworkConfiguration::new(&self.tap_name)
                    .with_mac_address(self.guest_mac.clone())
                    .with_ip_configuration(
                        IPConfiguration::new(self.guest_cidr, self.host_ip).with_if_name("eth0"),
                    ),
            ),
            ..NetworkInterface::default()
        }
    }

    fn provision(&self) -> TapDevice {
        TapDevice::create(self)
    }
}

#[derive(Debug)]
struct TapDevice {
    tap_name: String,
}

impl TapDevice {
    fn create(spec: &StaticTapNetwork) -> Self {
        let output = Command::new("ip")
            .args(["tuntap", "add", "dev", &spec.tap_name, "mode", "tap"])
            .output()
            .unwrap();
        assert_success(&output, "create tap device");

        let output = Command::new("ip")
            .args(["addr", "add", &spec.host_cidr, "dev", &spec.tap_name])
            .output()
            .unwrap();
        assert_success(&output, "assign tap address");

        let output = Command::new("ip")
            .args(["link", "set", "dev", &spec.tap_name, "up"])
            .output()
            .unwrap();
        assert_success(&output, "bring tap up");

        Self {
            tap_name: spec.tap_name.clone(),
        }
    }
}

impl Drop for TapDevice {
    fn drop(&mut self) {
        let _ = Command::new("ip")
            .args(["link", "del", "dev", &self.tap_name])
            .output();
    }
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn create_char_device(path: &Path, major: u32, minor: u32) {
    let output = Command::new("mknod")
        .arg(path)
        .args(["c", &major.to_string(), &minor.to_string()])
        .output()
        .unwrap();
    assert_success(&output, "create initramfs device node");
}

fn build_initramfs_archive(root_dir: &Path, archive_path: &Path) {
    let mut find = Command::new("find")
        .arg(".")
        .arg("-print0")
        .current_dir(root_dir)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let find_stdout = find.stdout.take().unwrap();
    let archive = fs::File::create(archive_path).unwrap();
    let cpio = Command::new("cpio")
        .args(["--null", "-o", "--format=newc"])
        .current_dir(root_dir)
        .stdin(Stdio::from(find_stdout))
        .stdout(Stdio::from(archive))
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    assert_success(&cpio, "build initramfs archive");
    assert!(find.wait().unwrap().success());
}

fn build_guest_initramfs(temp_dir: &Path, network: &StaticTapNetwork) -> PathBuf {
    let root = temp_dir.join("guest-initramfs");
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::create_dir_all(root.join("dev")).unwrap();
    fs::create_dir_all(root.join("proc")).unwrap();
    fs::create_dir_all(root.join("run")).unwrap();
    fs::create_dir_all(root.join("sys")).unwrap();
    fs::create_dir_all(root.join("tmp")).unwrap();
    fs::create_dir_all(root.join("www/cgi-bin")).unwrap();

    fs::copy(busybox_source(), root.join("bin/busybox")).unwrap();
    let mut busybox_permissions = fs::metadata(root.join("bin/busybox"))
        .unwrap()
        .permissions();
    busybox_permissions.set_mode(0o755);
    fs::set_permissions(root.join("bin/busybox"), busybox_permissions).unwrap();

    create_char_device(&root.join("dev/console"), 5, 1);
    create_char_device(&root.join("dev/null"), 1, 3);

    let init_script = format!(
        r#"#!/bin/busybox sh
set -eu

/bin/busybox mount -t proc proc /proc
/bin/busybox mount -t sysfs sysfs /sys

echo "guest init started"

iface=""
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
  for path in /sys/class/net/*; do
    name="${{path##*/}}"
    if [ "$name" != "lo" ]; then
      iface="$name"
      break
    fi
  done
  [ -n "$iface" ] && break
  /bin/busybox sleep 1
done

[ -n "$iface" ] || {{
  echo "no guest network interface"
  /bin/busybox ls -l /sys/class/net || true
  while true; do /bin/busybox sleep 3600; done
}}

/bin/busybox ip link set lo up
/bin/busybox ip link set "$iface" up
/bin/busybox ip addr add {guest_cidr} dev "$iface"
/bin/busybox ip route add default via {host_ip}

/bin/busybox httpd -f -p {http_port} -h /www &
echo "guest http ready on $iface"

while true; do /bin/busybox sleep 3600; done
"#,
        guest_cidr = network.guest_cidr,
        host_ip = network.host_ip,
        http_port = network.http_port,
    );
    write_executable(&root.join("init"), &init_script);

    let start_sleep_script = r#"#!/bin/busybox sh
printf 'Content-Type: text/plain\r\n\r\n'

if [ -s /run/snapshot-sleep.pid ] && kill -0 "$(/bin/busybox cat /run/snapshot-sleep.pid)" 2>/dev/null; then
  echo already-running
  exit 0
fi

/bin/busybox sleep 422 >/dev/null 2>&1 &
echo $! > /run/snapshot-sleep.pid
echo started
"#;
    write_executable(&root.join("www/cgi-bin/start_sleep"), start_sleep_script);

    let status_script = r#"#!/bin/busybox sh
printf 'Content-Type: text/plain\r\n\r\n'

if [ -s /run/snapshot-sleep.pid ] && kill -0 "$(/bin/busybox cat /run/snapshot-sleep.pid)" 2>/dev/null; then
  echo present
else
  echo missing
fi
"#;
    write_executable(&root.join("www/cgi-bin/status"), status_script);

    let initramfs_path = temp_dir.join("guest-initramfs.cpio");
    build_initramfs_archive(&root, &initramfs_path);
    initramfs_path
}

fn build_guest_filesystem(temp_dir: &Path, network: &StaticTapNetwork) -> GuestFilesystem {
    if real_vm_support::python_service_rootfs_available() {
        let _ = network;
        GuestFilesystem::Rootfs(real_vm_support::build_http_service_rootfs(
            temp_dir,
            "snapshot-network-rootfs",
        ))
    } else {
        GuestFilesystem::Initramfs(build_guest_initramfs(temp_dir, network))
    }
}

fn source_config(
    socket_path: &Path,
    vmid: &str,
    guest_filesystem: &GuestFilesystem,
    network: &StaticTapNetwork,
) -> Config {
    match guest_filesystem {
        GuestFilesystem::Initramfs(initramfs_path) => Config {
            vmid: vmid.to_string(),
            socket_path: socket_path.display().to_string(),
            kernel_image_path: kernel_path().to_string(),
            initrd_path: Some(initramfs_path.display().to_string()),
            kernel_args: "console=ttyS0 reboot=k panic=1 pci=off nomodules".to_string(),
            machine_cfg: MachineConfiguration::new(1, 128),
            network_interfaces: NetworkInterfaces::from(vec![network.network_interface()]),
            forward_signals: Some(Vec::new()),
            ..Config::default()
        },
        GuestFilesystem::Rootfs(rootfs_path) => {
            let rootfs_path = rootfs_path.display().to_string();
            Config {
                vmid: vmid.to_string(),
                socket_path: socket_path.display().to_string(),
                kernel_image_path: kernel_path().to_string(),
                kernel_args:
                    "console=ttyS0 reboot=k panic=1 pci=off nomodules root=/dev/vda rw rootfstype=ext4 init=/init"
                        .to_string(),
                drives: firecracker_sdk::DrivesBuilder::new(rootfs_path.clone())
                    .with_root_drive(rootfs_path, [with_read_only(true)])
                    .build(),
                machine_cfg: MachineConfiguration::new(1, 128),
                network_interfaces: NetworkInterfaces::from(vec![
                    network.network_interface_with_static_ip(),
                ]),
                forward_signals: Some(Vec::new()),
                ..Config::default()
            }
        }
    }
}

fn restore_config(socket_path: &Path, vmid: &str) -> Config {
    Config {
        vmid: vmid.to_string(),
        socket_path: socket_path.display().to_string(),
        forward_signals: Some(Vec::new()),
        ..Config::default()
    }
}

fn guest_http_get(network: &StaticTapNetwork, path: &str) -> Result<String, String> {
    let address = SocketAddr::new(IpAddr::V4(network.guest_ip), network.http_port);
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    stream
        .write_all(
            format!(
                "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                network.guest_ip
            )
            .as_bytes(),
        )
        .map_err(|error| error.to_string())?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;

    let response = String::from_utf8_lossy(&response);
    if !response.starts_with("HTTP/1.0 200") && !response.starts_with("HTTP/1.1 200") {
        return Err(response.into_owned());
    }

    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or_default()
        .trim()
        .to_string();
    Ok(body)
}

fn wait_for_guest_http(network: &StaticTapNetwork, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    let mut last_error = String::new();

    while Instant::now() < deadline {
        match guest_http_get(network, "/cgi-bin/status") {
            Ok(body) => return body,
            Err(error) => {
                last_error = error;
                thread::sleep(Duration::from_millis(500));
            }
        }
    }

    panic!("timed out waiting for guest http server: {last_error}");
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn test_real_snapshot_restore_preserves_guest_process_over_network() {
    if !assets_available() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let network = StaticTapNetwork::allocate();
    let guest_filesystem = build_guest_filesystem(temp_dir.path(), &network);

    let socket_path = temp_dir.path().join("source.sock");
    let restore_socket_path = temp_dir.path().join("restore.sock");
    let mem_path = temp_dir.path().join("vm.mem");
    let snapshot_path = temp_dir.path().join("vm.snap");

    {
        let _tap = network.provision();
        let mut machine = new_machine(
            source_config(
                &socket_path,
                "snapshot-network-source",
                &guest_filesystem,
                &network,
            ),
            [with_process_runner(firecracker_command(
                &socket_path,
                "snapshot-network-source",
            ))],
        )
        .unwrap();

        machine.start().unwrap();
        assert_eq!(
            "missing",
            wait_for_guest_http(&network, Duration::from_secs(30))
        );
        assert_eq!(
            "started",
            guest_http_get(&network, "/cgi-bin/start_sleep").unwrap()
        );
        assert_eq!(
            "present",
            guest_http_get(&network, "/cgi-bin/status").unwrap()
        );

        machine.pause_vm().unwrap();
        machine
            .create_snapshot(
                &mem_path.display().to_string(),
                &snapshot_path.display().to_string(),
            )
            .unwrap();
        machine.stop_vmm().unwrap();
        let _ = machine.wait();
    }

    assert!(mem_path.exists());
    assert!(snapshot_path.exists());
    assert!(std::fs::metadata(&mem_path).unwrap().len() > 0);
    assert!(std::fs::metadata(&snapshot_path).unwrap().len() > 0);

    {
        let _tap = network.provision();
        let mut restored_machine = new_machine(
            restore_config(&restore_socket_path, "snapshot-network-restore"),
            [
                with_snapshot(
                    "",
                    snapshot_path.display().to_string(),
                    [with_memory_backend("File", mem_path.display().to_string())],
                ),
                with_process_runner(firecracker_command(
                    &restore_socket_path,
                    "snapshot-network-restore",
                )),
            ],
        )
        .unwrap();

        restored_machine.start().unwrap();
        restored_machine.resume_vm().unwrap();
        assert_eq!(
            "present",
            wait_for_guest_http(&network, Duration::from_secs(30))
        );

        restored_machine.stop_vmm().unwrap();
        let _ = restored_machine.wait();
    }
}
