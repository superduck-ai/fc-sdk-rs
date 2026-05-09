# Models Layout

The Go SDK stores generated API models under `client/models/`.

This Rust migration keeps the model implementations in `src/models/`, with one
Rust source file per Firecracker model from the Go repository to stay close to
the original repository shape without introducing a generated-code layer.

The model file set matches the Go `client/models/*.go` inventory one-for-one
and is exercised by the migrated unit, integration, and real-VM tests.
