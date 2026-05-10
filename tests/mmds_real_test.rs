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
    AsyncResultExt, Config, IPConfiguration, MMDSVersion, MachineConfiguration, NetworkInterface,
    NetworkInterfaces, StaticNetworkConfiguration, VMCommandBuilder, new_machine,
    with_process_runner, with_read_only,
};
use ipnet::Ipv4Net;
use serde_json::json;

static NEXT_NETWORK_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Debug, Clone)]
enum GuestFilesystem {
    Initramfs(PathBuf),
    Rootfs(PathBuf),
}

fn assets_available() -> bool {
    (real_vm_support::python_service_rootfs_available()
        || (real_vm_support::assets_available()
            && command_in_path("cpio")
            && command_in_path("find")))
        && Path::new("/dev/net/tun").exists()
        && command_in_path("ip")
}

fn firecracker_command(socket_path: &Path, vmid: &str) -> firecracker_sdk::VMCommand {
    VMCommandBuilder::default()
        .with_bin(real_vm_support::firecracker_binary())
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
        let host_ip = Ipv4Addr::new(172, 30, subnet_octet, 1);
        let guest_ip = Ipv4Addr::new(172, 30, subnet_octet, 2);

        Self {
            tap_name: format!("fcmmds{unique:03}"),
            host_cidr: format!("{host_ip}/30"),
            host_ip,
            guest_cidr: format!("{guest_ip}/30").parse::<Ipv4Net>().unwrap(),
            guest_ip,
            guest_mac: format!("06:00:00:10:{subnet_octet:02x}:{unique:02x}"),
            http_port: 8080,
        }
    }

    fn network_interface(&self) -> NetworkInterface {
        NetworkInterface {
            static_configuration: Some(
                StaticNetworkConfiguration::new(&self.tap_name)
                    .with_mac_address(self.guest_mac.clone()),
            ),
            allow_mmds: true,
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
            allow_mmds: true,
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

fn command_in_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
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

    fs::copy(real_vm_support::busybox_source(), root.join("bin/busybox")).unwrap();
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

    let fetch_mmds_script = r#"#!/bin/busybox sh
printf 'Content-Type: text/plain\r\n\r\n'

token_response=$(
  {
    printf 'PUT /latest/api/token HTTP/1.1\r\n'
    printf 'Host: 169.254.169.254\r\n'
    printf 'X-metadata-token-ttl-seconds: 60\r\n'
    printf 'Connection: close\r\n\r\n'
  } | /bin/busybox nc -w 2 169.254.169.254 80
) || {
  echo token-request-failed
  exit 0
}

token=$(
  /bin/busybox printf '%s' "$token_response" \
    | /bin/busybox tr -d '\r' \
    | /bin/busybox sed -n '1,/^$/d;p' \
    | /bin/busybox head -n 1
)

if [ -z "$token" ]; then
  echo missing-token
  /bin/busybox printf '%s\n' "$token_response"
  exit 0
fi

metadata_response=$(
  {
    printf 'GET /latest/meta-data/message HTTP/1.1\r\n'
    printf 'Host: 169.254.169.254\r\n'
    printf 'X-metadata-token: %s\r\n' "$token"
    printf 'Connection: close\r\n\r\n'
  } | /bin/busybox nc -w 2 169.254.169.254 80
) || {
  echo metadata-request-failed
  exit 0
}

metadata=$(
  /bin/busybox printf '%s' "$metadata_response" \
    | /bin/busybox tr -d '\r' \
    | /bin/busybox sed -n '1,/^$/d;p' \
    | /bin/busybox head -n 1
)

if [ -n "$metadata" ]; then
  echo "$metadata"
else
  echo missing-metadata
  /bin/busybox printf '%s\n' "$metadata_response"
fi
"#;
    write_executable(&root.join("www/cgi-bin/fetch_mmds"), fetch_mmds_script);

    let health_script = r#"#!/bin/busybox sh
printf 'Content-Type: text/plain\r\n\r\n'
echo ready
"#;
    write_executable(&root.join("www/cgi-bin/health"), health_script);

    let initramfs_path = temp_dir.join("guest-initramfs.cpio");
    build_initramfs_archive(&root, &initramfs_path);
    initramfs_path
}

fn build_guest_filesystem(temp_dir: &Path, network: &StaticTapNetwork) -> GuestFilesystem {
    if real_vm_support::python_service_rootfs_available() {
        let _ = network;
        GuestFilesystem::Rootfs(real_vm_support::build_http_service_rootfs(
            temp_dir,
            "guest-http-service-rootfs",
        ))
    } else {
        GuestFilesystem::Initramfs(build_guest_initramfs(temp_dir, network))
    }
}

fn base_config(
    socket_path: &Path,
    vmid: &str,
    guest_filesystem: &GuestFilesystem,
    network: &StaticTapNetwork,
) -> Config {
    match guest_filesystem {
        GuestFilesystem::Initramfs(initramfs_path) => Config {
            vmid: vmid.to_string(),
            socket_path: socket_path.display().to_string(),
            kernel_image_path: real_vm_support::kernel_path().to_string(),
            initrd_path: Some(initramfs_path.display().to_string()),
            kernel_args: "console=ttyS0 reboot=k panic=1 pci=off nomodules".to_string(),
            machine_cfg: MachineConfiguration::new(1, 128),
            network_interfaces: NetworkInterfaces::from(vec![network.network_interface()]),
            mmds_address: Some("169.254.169.254".parse().unwrap()),
            mmds_version: MMDSVersion::V2,
            forward_signals: Some(Vec::new()),
            ..Config::default()
        },
        GuestFilesystem::Rootfs(rootfs_path) => {
            let rootfs_path = rootfs_path.display().to_string();
            Config {
                vmid: vmid.to_string(),
                socket_path: socket_path.display().to_string(),
                kernel_image_path: real_vm_support::kernel_path().to_string(),
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
                mmds_address: Some("169.254.169.254".parse().unwrap()),
                mmds_version: MMDSVersion::V2,
                forward_signals: Some(Vec::new()),
                ..Config::default()
            }
        }
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

fn wait_for_guest_http(network: &StaticTapNetwork, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    let mut last_error = String::new();

    while Instant::now() < deadline {
        match guest_http_get(network, "/cgi-bin/health") {
            Ok(body) if body == "ready" => return,
            Ok(body) => last_error = format!("unexpected body: {body}"),
            Err(error) => {
                last_error = error;
            }
        }
        thread::sleep(Duration::from_millis(500));
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
fn test_real_mmds_v2_guest_can_fetch_metadata_over_network() {
    if !assets_available() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let network = StaticTapNetwork::allocate();
    let guest_filesystem = build_guest_filesystem(temp_dir.path(), &network);
    let socket_path = temp_dir.path().join("machine.sock");

    let _tap = network.provision();
    let mut machine = new_machine(
        base_config(&socket_path, "mmds-real", &guest_filesystem, &network),
        [with_process_runner(firecracker_command(
            &socket_path,
            "mmds-real",
        ))],
    )
    .unwrap();

    machine.start().unwrap();
    machine
        .set_metadata(&json!({
            "latest": {
                "meta-data": {
                    "message": "hello-from-mmds"
                }
            }
        }))
        .unwrap();

    wait_for_guest_http(&network, Duration::from_secs(30));
    let body = guest_http_get(&network, "/cgi-bin/fetch_mmds").unwrap();
    assert_eq!("hello-from-mmds", body);

    machine.stop_vmm().unwrap();
    let _ = machine.wait();
}
