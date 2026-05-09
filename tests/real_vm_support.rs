#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const DEFAULT_DOCKER_ROOTFS_IMAGES: &[&str] = &[
    "registry.gz.cvte.cn/ccloud/ubuntu:22.04",
    "registry.gz.cvte.cn/e2b/base:latest",
];

const DEFAULT_DOCKER_PYTHON_ROOTFS_IMAGES: &[&str] = &[
    "registry.gz.cvte.cn/e2b/base:latest",
    "registry.gz.cvte.cn/ccloud/ubuntu:22.04",
];

const PYTHON_HTTP_SERVICE_SCRIPT: &str = r#"import http.client
import os
import subprocess
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PID_FILE = "/run/snapshot-sleep.pid"
PORT = 8080


def sleep_pid():
    try:
        with open(PID_FILE, "r", encoding="utf-8") as pid_file:
            return int(pid_file.read().strip())
    except Exception:
        return None


def sleep_present():
    pid = sleep_pid()
    if pid is None:
        return False

    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def start_sleep():
    if sleep_present():
        return "already-running"

    os.makedirs("/run", exist_ok=True)
    process = subprocess.Popen(
        ["sleep", "422"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    with open(PID_FILE, "w", encoding="utf-8") as pid_file:
        pid_file.write(str(process.pid))
    return "started"


def fetch_mmds():
    try:
        token_conn = http.client.HTTPConnection("169.254.169.254", 80, timeout=2)
        token_conn.request(
            "PUT",
            "/latest/api/token",
            headers={"X-metadata-token-ttl-seconds": "60"},
        )
        token_response = token_conn.getresponse()
        token = token_response.read().decode("utf-8").strip()
        token_conn.close()

        if not token:
            return "missing-token"

        metadata_conn = http.client.HTTPConnection("169.254.169.254", 80, timeout=2)
        metadata_conn.request(
            "GET",
            "/latest/meta-data/message",
            headers={"X-metadata-token": token},
        )
        metadata_response = metadata_conn.getresponse()
        metadata = metadata_response.read().decode("utf-8").strip()
        metadata_conn.close()

        return metadata or "missing-metadata"
    except Exception as exc:
        return f"metadata-request-failed: {exc}"


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path == "/cgi-bin/health":
            self.respond("ready")
            return
        if path == "/cgi-bin/fetch_mmds":
            self.respond(fetch_mmds())
            return
        if path == "/cgi-bin/start_sleep":
            self.respond(start_sleep())
            return
        if path == "/cgi-bin/status":
            self.respond("present" if sleep_present() else "missing")
            return

        self.send_response(404)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def log_message(self, *_args):
        return

    def respond(self, body):
        encoded = body.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


if __name__ == "__main__":
    print("guest http ready", flush=True)
    server = ThreadingHTTPServer(("0.0.0.0", PORT), Handler)
    server.serve_forever()
"#;

unsafe extern "C" {
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
}

pub fn firecracker_binary() -> &'static str {
    "/data/firecracker"
}

pub fn kernel_path() -> &'static str {
    "/data_jfs/fc-kernels/vmlinux-6.1.158/vmlinux.bin"
}

pub fn busybox_source() -> &'static str {
    "/data_jfs/fc-busybox/1.36.1/amd64/busybox"
}

#[derive(Debug, Clone)]
enum RootfsSource {
    Busybox(PathBuf),
    DockerImage(String),
}

pub fn assets_available() -> bool {
    is_root()
        && Path::new("/dev/kvm").exists()
        && Path::new(firecracker_binary()).exists()
        && Path::new(kernel_path()).exists()
        && command_in_path("mkfs.ext4")
        && command_in_path("mknod")
        && rootfs_source().is_some()
}

pub fn python_service_rootfs_available() -> bool {
    is_root()
        && Path::new("/dev/kvm").exists()
        && Path::new(firecracker_binary()).exists()
        && Path::new(kernel_path()).exists()
        && command_in_path("mkfs.ext4")
        && command_in_path("mknod")
        && python_service_rootfs_image().is_some()
}

pub fn build_sleeping_rootfs(temp_dir: &Path, image_name: &str) -> PathBuf {
    match rootfs_source().expect("missing local guest rootfs source") {
        RootfsSource::Busybox(path) => build_busybox_sleeping_rootfs(temp_dir, image_name, &path),
        RootfsSource::DockerImage(image) => {
            build_docker_sleeping_rootfs(temp_dir, image_name, &image)
        }
    }
}

pub fn build_http_service_rootfs(temp_dir: &Path, image_name: &str) -> PathBuf {
    let image = python_service_rootfs_image().expect("missing docker image with python3");
    build_docker_python_service_rootfs(temp_dir, image_name, &image, PYTHON_HTTP_SERVICE_SCRIPT)
}

fn build_busybox_sleeping_rootfs(
    temp_dir: &Path,
    image_name: &str,
    busybox_path: &Path,
) -> PathBuf {
    let root = temp_dir.join(format!("{image_name}-rootfs"));
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::create_dir_all(root.join("dev")).unwrap();
    fs::create_dir_all(root.join("proc")).unwrap();
    fs::create_dir_all(root.join("run")).unwrap();
    fs::create_dir_all(root.join("sys")).unwrap();
    fs::create_dir_all(root.join("tmp")).unwrap();

    fs::copy(busybox_path, root.join("bin/busybox")).unwrap();
    let mut busybox_permissions = fs::metadata(root.join("bin/busybox"))
        .unwrap()
        .permissions();
    busybox_permissions.set_mode(0o755);
    fs::set_permissions(root.join("bin/busybox"), busybox_permissions).unwrap();

    create_char_device_if_missing(&root.join("dev/console"), 5, 1);
    create_char_device_if_missing(&root.join("dev/null"), 1, 3);

    let init_script = r#"#!/bin/busybox sh
set -eu

/bin/busybox mount -t proc proc /proc
/bin/busybox mount -t sysfs sysfs /sys

echo "guest rootfs init ready"

while true; do /bin/busybox sleep 3600; done
"#;
    write_executable(&root.join("init"), init_script);

    build_ext4_image(temp_dir, image_name, &root)
}

fn build_docker_sleeping_rootfs(temp_dir: &Path, image_name: &str, image: &str) -> PathBuf {
    let root = temp_dir.join(format!("{image_name}-rootfs"));
    fs::create_dir_all(&root).unwrap();

    populate_root_from_docker_image(&root, temp_dir, image_name, image);

    fs::create_dir_all(root.join("dev")).unwrap();
    fs::create_dir_all(root.join("proc")).unwrap();
    fs::create_dir_all(root.join("run")).unwrap();
    fs::create_dir_all(root.join("sys")).unwrap();
    fs::create_dir_all(root.join("tmp")).unwrap();

    create_char_device_if_missing(&root.join("dev/console"), 5, 1);
    create_char_device_if_missing(&root.join("dev/null"), 1, 3);

    let shell = if root.join("bin/sh").exists() {
        "/bin/sh"
    } else {
        "/usr/bin/sh"
    };
    let init_script = format!(
        r#"#!{shell}
set -eu
export PATH=/bin:/sbin:/usr/bin:/usr/sbin

mount -t proc proc /proc
mount -t sysfs sysfs /sys

echo "guest rootfs init ready"

while true; do sleep 3600; done
"#
    );
    write_executable(&root.join("init"), &init_script);

    build_ext4_image(temp_dir, image_name, &root)
}

fn build_docker_python_service_rootfs(
    temp_dir: &Path,
    image_name: &str,
    image: &str,
    service_script: &str,
) -> PathBuf {
    let root = temp_dir.join(format!("{image_name}-rootfs"));
    fs::create_dir_all(&root).unwrap();

    populate_root_from_docker_image(&root, temp_dir, image_name, image);

    fs::create_dir_all(root.join("dev")).unwrap();
    fs::create_dir_all(root.join("opt")).unwrap();
    fs::create_dir_all(root.join("run")).unwrap();
    fs::create_dir_all(root.join("tmp")).unwrap();

    create_char_device_if_missing(&root.join("dev/console"), 5, 1);
    create_char_device_if_missing(&root.join("dev/null"), 1, 3);

    write_executable(&root.join("opt/guest_http_service.py"), service_script);

    let shell = guest_shell(&root);
    let init_script = format!(
        r#"#!{shell}
set -eu
export PATH=/usr/local/bin:/usr/local/sbin:/usr/bin:/usr/sbin:/bin:/sbin

mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t tmpfs tmpfs /run

echo "guest init started"
exec python3 -u /opt/guest_http_service.py
"#
    );
    write_executable(&root.join("init"), &init_script);

    build_ext4_image(temp_dir, image_name, &root)
}

fn build_ext4_image(temp_dir: &Path, image_name: &str, root: &Path) -> PathBuf {
    let image_path = temp_dir.join(format!("{image_name}.ext4"));
    let image_file = fs::File::create(&image_path).unwrap();
    image_file
        .set_len(suggested_ext4_size(directory_size(root)))
        .unwrap();

    let output = Command::new("mkfs.ext4")
        .args(["-q", "-d"])
        .arg(&root)
        .args(["-F"])
        .arg(&image_path)
        .output()
        .unwrap();
    assert_success(&output, "build ext4 rootfs image");

    image_path
}

fn rootfs_source() -> Option<RootfsSource> {
    if let Some(image) = configured_rootfs_image() {
        return docker_rootfs_source(&image);
    }

    if Path::new(busybox_source()).exists() {
        return Some(RootfsSource::Busybox(PathBuf::from(busybox_source())));
    }

    default_docker_rootfs_image()
        .as_deref()
        .and_then(docker_rootfs_source)
}

fn configured_rootfs_image() -> Option<String> {
    std::env::var("FIRECRACKER_RUST_SDK_ROOTFS_IMAGE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn python_service_rootfs_image() -> Option<String> {
    if let Some(image) = configured_rootfs_image() {
        if docker_image_has_command(&image, "python3") {
            return Some(image);
        }
    }

    default_docker_python_rootfs_image()
}

fn default_docker_rootfs_image() -> Option<String> {
    if !command_in_path("docker") {
        return None;
    }

    DEFAULT_DOCKER_ROOTFS_IMAGES
        .iter()
        .find(|image| docker_image_available(image))
        .map(|image| (*image).to_string())
}

fn default_docker_python_rootfs_image() -> Option<String> {
    if !command_in_path("docker") {
        return None;
    }

    DEFAULT_DOCKER_PYTHON_ROOTFS_IMAGES
        .iter()
        .find(|image| docker_image_has_command(image, "python3"))
        .map(|image| (*image).to_string())
}

fn docker_rootfs_source(image: &str) -> Option<RootfsSource> {
    if command_in_path("docker") && command_in_path("tar") && docker_image_available(image) {
        Some(RootfsSource::DockerImage(image.to_string()))
    } else {
        None
    }
}

fn docker_image_available(image: &str) -> bool {
    Command::new("docker")
        .args(["image", "inspect", image])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn docker_image_has_command(image: &str, command: &str) -> bool {
    if !docker_image_available(image) {
        return false;
    }

    Command::new("docker")
        .args([
            "run",
            "--rm",
            "--entrypoint",
            "/bin/sh",
            image,
            "-lc",
            &format!("command -v {command} >/dev/null 2>&1"),
        ])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn populate_root_from_docker_image(root: &Path, temp_dir: &Path, image_name: &str, image: &str) {
    let container = DockerContainer::create(image);
    let tar_path = temp_dir.join(format!("{image_name}-rootfs.tar"));

    let output = Command::new("docker")
        .args(["export", "--output"])
        .arg(&tar_path)
        .arg(&container.id)
        .output()
        .unwrap();
    assert_success(&output, "export docker guest rootfs");

    let output = Command::new("tar")
        .args(["-xf"])
        .arg(&tar_path)
        .args(["-C"])
        .arg(root)
        .output()
        .unwrap();
    assert_success(&output, "extract docker guest rootfs");

    let _ = fs::remove_file(tar_path);
}

fn is_root() -> bool {
    unsafe { libc_geteuid() == 0 }
}

fn command_in_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}

fn guest_shell(root: &Path) -> &str {
    if root.join("bin/sh").exists() {
        "/bin/sh"
    } else {
        "/usr/bin/sh"
    }
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn create_char_device_if_missing(path: &Path, major: u32, minor: u32) {
    if path.exists() {
        return;
    }

    let output = Command::new("mknod")
        .arg(path)
        .args(["c", &major.to_string(), &minor.to_string()])
        .output()
        .unwrap();
    assert_success(&output, "create rootfs device node");
}

fn directory_size(path: &Path) -> u64 {
    let metadata = fs::symlink_metadata(path).unwrap();
    if metadata.is_dir() {
        fs::read_dir(path)
            .unwrap()
            .map(|entry| directory_size(&entry.unwrap().path()))
            .sum()
    } else {
        metadata.len()
    }
}

fn suggested_ext4_size(root_size: u64) -> u64 {
    const MIB: u64 = 1024 * 1024;
    const MIN_SIZE: u64 = 64 * MIB;
    const EXTRA_SLACK: u64 = 128 * MIB;
    const ALIGNMENT: u64 = 16 * MIB;

    let sized = root_size
        .saturating_mul(2)
        .max(root_size.saturating_add(EXTRA_SLACK))
        .max(MIN_SIZE);

    sized.div_ceil(ALIGNMENT) * ALIGNMENT
}

struct DockerContainer {
    id: String,
}

impl DockerContainer {
    fn create(image: &str) -> Self {
        let output = Command::new("docker")
            .args(["create", image])
            .output()
            .unwrap();
        assert_success(&output, "create docker container for guest rootfs");

        let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert!(
            !id.is_empty(),
            "docker create returned an empty container id"
        );

        Self { id }
    }
}

impl Drop for DockerContainer {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.id]).output();
    }
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
