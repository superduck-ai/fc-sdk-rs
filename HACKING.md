# Hacking

This repository tracks the Firecracker Go SDK closely in layout and behavior,
but the implementation is written from scratch in Rust.

## Local Prerequisites

The current test matrix uses the following assets:

1. Firecracker binary at `/data/firecracker`
2. Kernel images under `/data_jfs/fc-kernels`
3. Access to `/dev/kvm` for real microVM startup tests
4. Access to `/dev/vhost-vsock` for vsock-capable paths
5. Root privileges for the real CNI, TAP, and locally-built guest rootfs tests
6. `mkfs.ext4`, `cpio`, and `mknod` for the synthesized guest images used by
   the real end-to-end tests
7. Static busybox binary at `/data_jfs/fc-busybox/1.36.1/amd64/busybox` for
   the initramfs-based networking and MMDS tests
8. For the synthesized ext4 guest rootfs used by the other real lifecycle and
   snapshot tests: either the same local BusyBox asset, or local Docker plus a
   locally available image such as `registry.gz.cvte.cn/ccloud/ubuntu:22.04`
   or `registry.gz.cvte.cn/e2b/base:latest`
9. Optional local `jailer` binary for jailer end-to-end parity work

## Repository Conventions

- Keep code organization close to the Go SDK when practical:
  `src/machine.rs`, `src/handlers.rs`, `src/network.rs`, `src/jailer.rs`,
  `src/client*`, and matching files under `tests/`.
- Prefer handwritten Rust implementations over generated bindings.
- Avoid introducing dependencies unless they materially improve correctness or
  maintainability.

## Validation Workflow

The default migration gate is:

```bash
make check
```

That expands to:

```bash
cargo fmt --all
cargo test --quiet
cargo test --quiet --doc
cargo check --examples
cargo bench --no-run
```

## Timeout Controls

The SDK keeps the Go-compatible timeout environment variable names:

- `FIRECRACKER_GO_SDK_INIT_TIMEOUT_SECONDS`
- `FIRECRACKER_GO_SDK_REQUEST_TIMEOUT_MILLISECONDS`

## Testdata Notes

The `testdata/` directory exists to keep repository layout close to the Go SDK.
It currently contains a mix of locally-authored helpers and local compatibility
assets:

- `firecracker` and `jailer` binaries extracted from the matching official
  Firecracker release
- `vmlinux` and `root-drive.img` as local compatibility links kept for
  repository-shape parity
- sparse placeholder secondary drive images
- `sigprint.sh` for signal-forwarding experiments

Still not bundled here:

- SSH-enabled rootfs images and keys
- opaque guest images copied from another SDK tree

The real lifecycle/snapshot/network tests now construct their own minimal guest
initramfs or ext4 rootfs at runtime. If you add new data files, prefer assets
created locally for this repository over copying opaque binaries from another
SDK tree.

If you need to force the ext4-based real tests onto a particular local Docker
image filesystem, set `FIRECRACKER_RUST_SDK_ROOTFS_IMAGE`.
