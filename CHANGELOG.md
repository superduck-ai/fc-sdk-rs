# Changelog

All notable changes in this repository should be recorded in this file.

## [Unreleased]

### Added

- A from-scratch Rust SDK layout that mirrors the major module boundaries of
  `firecracker-go-sdk`.
- Cargo-based validation gates for unit tests, doctests, examples, and bench
  compilation.
- Real Firecracker lifecycle, snapshot, transport timeout, CNI, and signal
  forwarding coverage in `tests/`.

### Changed

- `Machine::stop_vmm`, `Machine::wait`, and `Machine::pid` now preserve and
  expose terminal process state more closely to the Go SDK semantics.

### Known Gaps

- Full jailer end-to-end execution parity still depends on a local `jailer`
  binary and matching privileged environment setup.
- SSH-backed snapshot restore content verification still depends on an
  SSH-enabled rootfs image and private key material that are not bundled here.
