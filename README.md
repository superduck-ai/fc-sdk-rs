# firecracker-rs-sdk2

This repository is a from-scratch Rust implementation of the Firecracker Go
SDK layout and behavior, built by migrating the Go project module by module
into idiomatic Rust.

## Status

The current crate includes:

- a synchronous Firecracker client over Unix domain sockets
- machine lifecycle management
- command and jailer builders
- drive, balloon, rate limiter, snapshot, vsock, MMDS, and handler support
- pure-logic CNI conversion and setup abstractions
- unit tests, integration tests, doctests, and real Firecracker API coverage

The code is intentionally organized to stay close to the Go SDK structure while
using Rust modules, traits, builders, and strongly typed models.

## Local prerequisites

The current test suite assumes the following local assets:

- Firecracker binary: `/data/firecracker`
- Kernel images: `/data_jfs/fc-kernels/...`
- Root privileges plus `/dev/kvm` for real microVM startup tests
- `/dev/net/tun`, `mkfs.ext4`, `cpio`, and `mknod` for the real guest
  networking and locally-built guest rootfs tests
- Static busybox binary: `/data_jfs/fc-busybox/1.36.1/amd64/busybox`
  for the initramfs-based real networking and MMDS tests
- For the synthesized ext4 guest rootfs used by the other real lifecycle and
  snapshot tests: either the same local BusyBox asset, or local Docker plus a
  locally available image such as `registry.gz.cvte.cn/ccloud/ubuntu:22.04`
  or `registry.gz.cvte.cn/e2b/base:latest`

For repository-shape parity with the Go SDK, `testdata/` also includes local
compatibility assets such as `testdata/firecracker`, `testdata/jailer`,
`testdata/vmlinux`, and `testdata/root-drive.img`.

Some tests use the real Firecracker API socket. More advanced end-to-end VM
flows such as full snapshot restore with guest networking now synthesize their
own minimal BusyBox initramfs or ext4 rootfs locally instead of depending on an
external guest disk image.

If you want to force the ext4-based real tests to use a specific local Docker
image as the guest rootfs source, set `FIRECRACKER_RUST_SDK_ROOTFS_IMAGE`.

## Running tests

```bash
cargo test --quiet
```

The repository also provides a `Makefile` with a `check` target that runs the
same validation gates used during the migration audit:

```bash
make check
```

Doctests can also be run independently:

```bash
cargo test --quiet --doc
```

## Layout

The project keeps the major Go SDK areas split into corresponding Rust modules:

- `src/client*` for API transport and client methods
- `src/machine.rs` for VM lifecycle orchestration
- `src/handlers.rs` for handler chains
- `src/network.rs` and `src/cni/*` for network and CNI logic
- `src/jailer.rs` for jailer command construction and handler adaptation
- `src/vsock/*` for vsock dial/listen helpers
- `tests/` for migrated unit and integration coverage

## Snapshot docs

Snapshot-specific usage notes live in [docs/snapshotting.md](docs/snapshotting.md).

Additional contributor and environment notes live in:

- [CONTRIBUTING.md](CONTRIBUTING.md)
- [HACKING.md](HACKING.md)
- [CHANGELOG.md](CHANGELOG.md)
