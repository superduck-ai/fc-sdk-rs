# Operations Layout

The Go SDK stores generated operation parameter and response wrappers under
`client/operations/`.

This Rust migration keeps the operation surface in the handwritten Unix-socket
client at `src/client/firecracker_client.rs`, with one Rust method per
Firecracker API operation from `client/swagger.yaml`.

The generated Go wrappers are not reproduced as compiled Rust source files
under this directory because the crate implementation lives under `src/`, but
the API surface is fully migrated and verified through
`tests/client_transport_test.rs`, `tests/firecracker_test.rs`, and the real VM
integration tests.
