# Contributing

This repository is a from-scratch Rust implementation of the Firecracker Go SDK.
Contributions should preserve that constraint.

## Core Rules

- Do not copy code from sibling Rust SDK repositories or other third-party Rust
  projects into this crate.
- The Go SDK may be used as a behavioral and layout reference, but Rust code in
  this repository should be written directly for this crate.
- Keep module boundaries and test organization as close to the Go SDK as is
  practical without fighting Rust conventions.

## Before Opening a Change

- Prefer a small, scoped change over a cross-repository rewrite.
- Record behavior changes in `CHANGELOG.md` when they affect public semantics.
- Keep tests with the migrated feature area whenever possible.

## Required Validation

Run the full local gate before proposing a change:

```bash
cargo fmt --all
cargo test --quiet
cargo test --quiet --doc
cargo check --examples
cargo bench --no-run
```

Or run the same set through:

```bash
make check
```

## Environment Notes

- Firecracker binary used by the current real tests: `/data/firecracker`
- Kernel directory used by the current real tests: `/data_jfs/fc-kernels`
- Some integration tests require root privileges, `/dev/kvm`, `/dev/vhost-vsock`,
  or local CNI capabilities.

## Review Expectations

- Behavior parity matters more than superficial API similarity.
- Tests should cover both pure logic and real Firecracker paths when the
  environment allows it.
- If parity cannot be achieved because an external binary or image is missing,
  document the exact blocker in the change description.
