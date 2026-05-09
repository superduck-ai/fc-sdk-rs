//! Helpers for translating CNI results into guest network configuration.
//!
//! This module mirrors the Go SDK `cni/vmconf` package layout while keeping the
//! public Rust API at `crate::cni::vmconf::*`.

#[path = "vmconf.rs"]
mod imp;

pub use imp::{StaticNetworkConf, VmConfError, mtu_of, static_network_conf_from};
