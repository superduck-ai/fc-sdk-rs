# Snapshotting

Snapshotting is exposed in this Rust SDK through the Firecracker snapshot API.

As with the Go SDK, the important operational caveat is unchanged: snapshots
only capture guest memory and Firecracker/KVM device state. Host-side resources
such as tap devices, block device placement, jailer workspace layout, and any
external orchestration state must be recreated by the caller before a snapshot
is loaded again.

In practice that means:

- Drives should remain at the same host paths when a snapshot is restored.
- Network configuration must be reconstructed by the caller.
- Snapshot loading happens during machine initialization, before normal boot.

## Creating a snapshot

To create a snapshot, start the VM, pause it, then call
`Machine::create_snapshot`.

```rust,no_run
use firecracker_sdk::{Config, Machine, MachineConfiguration};

# tokio::runtime::Builder::new_current_thread()
#     .enable_all()
#     .build()
#     .unwrap()
#     .block_on(async {
let mut machine = Machine::new(Config {
    socket_path: "/tmp/firecracker.sock".to_string(),
    kernel_image_path: "/path/to/kernel".to_string(),
    machine_cfg: MachineConfiguration::new(1, 256),
    ..Config::default()
})?;

machine.start().await?;
machine.pause_vm().await?;
machine.create_snapshot("/tmp/vm.mem", "/tmp/vm.snap").await?;
# Ok::<(), firecracker_sdk::Error>(())
#     })
#     .unwrap();
```

## Loading a snapshot

To load a snapshot, build the machine with `with_snapshot(...)`. When the
machine starts, the snapshot is loaded instead of running the normal boot
configuration flow.

```rust,no_run
use firecracker_sdk::{
    Config, MachineConfiguration, new_machine, with_memory_backend, with_snapshot,
};

# tokio::runtime::Builder::new_current_thread()
#     .enable_all()
#     .build()
#     .unwrap()
#     .block_on(async {
let mut machine = new_machine(
    Config {
        socket_path: "/tmp/firecracker.sock".to_string(),
        machine_cfg: MachineConfiguration::new(1, 256),
        ..Config::default()
    },
    [with_snapshot(
        "",
        "/tmp/vm.snap",
        [with_memory_backend("File", "/tmp/vm.mem")],
    )],
)?;

machine.start().await?;
machine.resume_vm().await?;
# Ok::<(), firecracker_sdk::Error>(())
#     })
#     .unwrap();
```

## Notes

- `with_snapshot("", snapshot_path, ...)` is valid when the memory backend is
  provided through `with_memory_backend(...)`.
- `SnapshotConfig::with_paths(mem_path, snapshot_path)` is available when you
  want to build the snapshot configuration directly.
- The repository now includes real end-to-end snapshot and restore coverage.
  When the required host capabilities are present, the tests synthesize their
  own minimal guest rootfs locally from BusyBox or a local Docker image
  filesystem instead of requiring a vendored guest disk image.
