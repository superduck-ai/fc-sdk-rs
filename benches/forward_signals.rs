#[path = "../tests/real_vm_support.rs"]
mod real_vm_support;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use firecracker_sdk::{
    CommandStdio, Config, MachineConfiguration, VMCommandBuilder, new_machine, with_process_runner,
};

static VM_COUNTER: AtomicUsize = AtomicUsize::new(0);
static ROOTFS_PATH: OnceLock<PathBuf> = OnceLock::new();

fn assets_available() -> bool {
    real_vm_support::assets_available()
}

fn batch_size() -> usize {
    std::env::var("FIRECRACKER_RUST_SDK_BENCH_VM_COUNT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(200)
}

fn make_vm_command_with_serial(
    socket_path: &Path,
    vmid: &str,
    serial_path: &Path,
) -> firecracker_sdk::VMCommand {
    VMCommandBuilder::default()
        .with_bin(real_vm_support::firecracker_binary())
        .with_socket_path(socket_path.display().to_string())
        .with_args(["--id", vmid, "--no-seccomp"])
        .with_stdin(CommandStdio::Null)
        .with_stdout(CommandStdio::Path(serial_path.display().to_string()))
        .with_stderr(CommandStdio::Path(serial_path.display().to_string()))
        .build()
}

fn rootfs_path() -> &'static Path {
    ROOTFS_PATH.get_or_init(|| {
        let temp_dir = Box::new(tempfile::tempdir().unwrap());
        let rootfs = real_vm_support::build_sleeping_rootfs(temp_dir.path(), "bench-rootfs");
        let _ = Box::leak(temp_dir);
        rootfs
    })
}

fn create_machine(
    bench_name: &str,
    forward_signals: Option<Vec<i32>>,
) -> (
    firecracker_sdk::Machine,
    tempfile::TempDir,
    PathBuf,
    PathBuf,
) {
    let counter = VM_COUNTER.fetch_add(1, Ordering::SeqCst);
    let vmid = format!("{bench_name}-{counter}");
    let temp_dir = tempfile::Builder::new().prefix(&vmid).tempdir().unwrap();
    let socket_path = temp_dir.path().join("firecracker.sock");
    let log_path = temp_dir.path().join("firecracker.log");
    let serial_path = temp_dir.path().join("firecracker.serial");
    std::fs::File::create(&log_path).unwrap();
    std::fs::File::create(&serial_path).unwrap();

    let machine = new_machine(
        Config {
            vmid: vmid.clone(),
            socket_path: socket_path.display().to_string(),
            log_path: Some(log_path.display().to_string()),
            log_level: Some("Info".to_string()),
            kernel_image_path: real_vm_support::kernel_path().to_string(),
            kernel_args:
                "console=ttyS0 reboot=k panic=1 pci=off nomodules root=/dev/vda rw rootfstype=ext4 init=/init"
                    .to_string(),
            drives: firecracker_sdk::DrivesBuilder::new(rootfs_path().display().to_string())
                .with_root_drive(
                    rootfs_path().display().to_string(),
                    [firecracker_sdk::with_read_only(true)],
                )
                .build(),
            machine_cfg: MachineConfiguration::new(1, 256),
            disable_validation: true,
            forward_signals,
            ..Config::default()
        },
        [with_process_runner(make_vm_command_with_serial(
            &socket_path,
            &vmid,
            &serial_path,
        ))],
    )
    .unwrap();

    (machine, temp_dir, log_path, serial_path)
}

fn wait_for_guest_boot(log_path: &Path, serial_path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if std::fs::read_to_string(serial_path)
            .ok()
            .is_some_and(|contents| contents.contains("guest rootfs init ready"))
        {
            return;
        }

        if std::fs::read_to_string(log_path)
            .ok()
            .is_some_and(|contents| contents.contains("Vmm is stopping"))
        {
            return;
        }

        if Instant::now() >= deadline {
            thread::sleep(Duration::from_millis(100));
            return;
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn start_and_wait_vm(bench_name: &str, forward_signals: Option<Vec<i32>>) {
    let (mut machine, temp_dir, log_path, serial_path) =
        create_machine(bench_name, forward_signals);
    machine.start().unwrap();
    wait_for_guest_boot(&log_path, &serial_path);
    machine.stop_vmm().unwrap();
    let _ = machine.wait();
    drop(temp_dir);
}

fn run_batch(bench_name: &str, forward_signals: Option<Vec<i32>>) {
    let mut handles = Vec::with_capacity(batch_size());
    for _ in 0..batch_size() {
        let bench_name = bench_name.to_string();
        let forward_signals = forward_signals.clone();
        handles.push(thread::spawn(move || {
            start_and_wait_vm(&bench_name, forward_signals);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

fn benchmark_forward_signals(c: &mut Criterion) {
    if !assets_available() {
        return;
    }

    let mut group = c.benchmark_group("forward_signals");
    group.sample_size(10);

    group.bench_function("default", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                run_batch("forward-signals-default", None);
                total += start.elapsed();
            }
            total
        });
    });

    group.bench_function("disabled", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                run_batch("forward-signals-disabled", Some(Vec::new()));
                total += start.elapsed();
            }
            total
        });
    });

    group.finish();
}

criterion_group!(benches, benchmark_forward_signals);
criterion_main!(benches);
