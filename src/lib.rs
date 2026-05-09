//! Firecracker Rust SDK.
//!
//! This crate is a from-scratch Rust migration of the Go Firecracker SDK.
//! The migration keeps the module boundaries close to the Go layout while
//! using idiomatic Rust types and builders.
//!
//! # Examples
//!
//! Process runner logging:
//! ```no_run
//! use firecracker_sdk::{
//!     Config, DrivesBuilder, MachineConfiguration, VMCommandBuilder, new_machine,
//!     with_process_runner,
//! };
//!
//! let socket_path = "/tmp/firecracker.sock";
//! let command = VMCommandBuilder::default()
//!     .with_bin("firecracker")
//!     .with_socket_path(socket_path)
//!     .with_stdout_path("/tmp/stdout.log")
//!     .with_stderr_path("/tmp/stderr.log")
//!     .build();
//!
//! let mut machine = new_machine(
//!     Config {
//!         socket_path: socket_path.to_string(),
//!         kernel_image_path: "/path/to/kernel".to_string(),
//!         drives: DrivesBuilder::new("/path/to/rootfs").build(),
//!         machine_cfg: MachineConfiguration::new(1, 256),
//!         ..Config::default()
//!     },
//!     [with_process_runner(command)],
//! )
//! .unwrap();
//! machine.start().unwrap();
//! machine.wait().unwrap();
//! ```
//!
//! Building a drive list:
//! ```no_run
//! use firecracker_sdk::{Config, DrivesBuilder, MachineConfiguration, Machine};
//!
//! let drives = DrivesBuilder::new("/path/to/rootfs")
//!     .add_drive("/first/path/drive.img", true, std::iter::empty())
//!     .add_drive("/second/path/drive.img", false, std::iter::empty())
//!     .build();
//!
//! let mut machine = Machine::new(Config {
//!     socket_path: "/tmp/firecracker.sock".to_string(),
//!     kernel_image_path: "/path/to/kernel".to_string(),
//!     drives,
//!     machine_cfg: MachineConfiguration::new(1, 256),
//!     ..Config::default()
//! })
//! .unwrap();
//! machine.start().unwrap();
//! machine.wait().unwrap();
//! ```
//!
//! Adding a drive option:
//! ```no_run
//! use std::time::Duration;
//!
//! use firecracker_sdk::{
//!     Config, DrivesBuilder, Machine, MachineConfiguration, TokenBucketBuilder, new_rate_limiter,
//!     with_rate_limiter,
//! };
//!
//! let limiter = new_rate_limiter(
//!     TokenBucketBuilder::default()
//!         .with_initial_size(1024 * 1024)
//!         .with_bucket_size(1024 * 1024)
//!         .with_refill_duration(Duration::from_millis(500))
//!         .build(),
//!     TokenBucketBuilder::default().build(),
//!     std::iter::empty(),
//! );
//!
//! let drives = DrivesBuilder::new("/path/to/rootfs")
//!     .add_drive("/path/to/drive1.img", true, std::iter::empty())
//!     .add_drive(
//!         "/path/to/drive2.img",
//!         false,
//!         [with_rate_limiter(limiter)],
//!     )
//!     .build();
//!
//! let mut machine = Machine::new(Config {
//!     socket_path: "/tmp/firecracker.sock".to_string(),
//!     kernel_image_path: "/path/to/kernel".to_string(),
//!     drives,
//!     machine_cfg: MachineConfiguration::new(1, 256),
//!     ..Config::default()
//! })
//! .unwrap();
//! machine.start().unwrap();
//! machine.wait().unwrap();
//! ```
//!
//! Rate limiting a network interface:
//! ```no_run
//! use std::time::Duration;
//!
//! use firecracker_sdk::{
//!     Config, DrivesBuilder, Machine, MachineConfiguration, NetworkInterface, NetworkInterfaces,
//!     StaticNetworkConfiguration, TokenBucketBuilder, new_rate_limiter,
//! };
//!
//! let inbound = new_rate_limiter(
//!     TokenBucketBuilder::default()
//!         .with_initial_size(1024 * 1024)
//!         .with_bucket_size(1024 * 1024)
//!         .with_refill_duration(Duration::from_secs(30))
//!         .build(),
//!     TokenBucketBuilder::default()
//!         .with_initial_size(5)
//!         .with_bucket_size(5)
//!         .with_refill_duration(Duration::from_secs(5))
//!         .build(),
//!     std::iter::empty(),
//! );
//!
//! let outbound = new_rate_limiter(
//!     TokenBucketBuilder::default()
//!         .with_initial_size(100)
//!         .with_bucket_size(1024 * 1024 * 10)
//!         .with_refill_duration(Duration::from_secs(30))
//!         .build(),
//!     TokenBucketBuilder::default()
//!         .with_initial_size(100)
//!         .with_bucket_size(100)
//!         .with_refill_duration(Duration::from_secs(5))
//!         .build(),
//!     std::iter::empty(),
//! );
//!
//! let network_interfaces = NetworkInterfaces::from(vec![NetworkInterface {
//!     static_configuration: Some(
//!         StaticNetworkConfiguration::new("tap-name").with_mac_address("01-23-45-67-89-AB-CD-EF"),
//!     ),
//!     in_rate_limiter: Some(inbound),
//!     out_rate_limiter: Some(outbound),
//!     ..NetworkInterface::default()
//! }]);
//!
//! let mut machine = Machine::new(Config {
//!     socket_path: "/path/to/socket".to_string(),
//!     kernel_image_path: "/path/to/kernel".to_string(),
//!     drives: DrivesBuilder::new("/path/to/rootfs").build(),
//!     machine_cfg: MachineConfiguration::new(1, 256),
//!     network_interfaces,
//!     ..Config::default()
//! })
//! .unwrap();
//! machine.start().unwrap();
//! machine.wait().unwrap();
//! ```
//!
//! Enabling the jailer:
//! ```no_run
//! use std::sync::Arc;
//!
//! use firecracker_sdk::{
//!     Config, DrivesBuilder, JailerConfig, Machine, MachineConfiguration, NaiveChrootStrategy,
//! };
//!
//! let kernel_image_path = "/path/to/kernel-image";
//! let mut machine = Machine::new(Config {
//!     socket_path: "api.socket".to_string(),
//!     kernel_image_path: kernel_image_path.to_string(),
//!     kernel_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
//!     drives: DrivesBuilder::new("/path/to/rootfs").build(),
//!     log_level: Some("Debug".to_string()),
//!     machine_cfg: MachineConfiguration::new(1, 256),
//!     jailer_cfg: Some(JailerConfig {
//!         uid: Some(123),
//!         gid: Some(100),
//!         id: "my-jailer-test".to_string(),
//!         numa_node: Some(0),
//!         chroot_base_dir: Some("/path/to/jailer-workspace".to_string()),
//!         chroot_strategy: Some(Arc::new(NaiveChrootStrategy::new(kernel_image_path))),
//!         exec_file: "/path/to/firecracker-binary".to_string(),
//!         ..JailerConfig::default()
//!     }),
//!     ..Config::default()
//! })
//! .unwrap();
//! machine.start().unwrap();
//! machine.wait().unwrap();
//! ```

pub mod balloon;
pub mod client;
pub mod client_transports;
pub mod cni;
pub mod command_builder;
pub mod config;
pub mod drives;
pub mod error;
pub mod fctesting;
pub mod handlers;
pub mod internal;
pub mod jailer;
pub mod kernelargs;
pub mod machine;
pub mod machineiface;
pub mod models;
pub mod network;
pub mod opts;
pub mod pointer_helpers;
pub mod rate_limiter;
pub mod snapshot;
pub mod utils;
pub mod version;
pub mod vsock;

pub use balloon::{BalloonDevice, BalloonOpt, with_stats_polling_intervals};
pub use client::{
    Client, ClientOps, DEFAULT_FIRECRACKER_REQUEST_TIMEOUT, FIRECRACKER_REQUEST_TIMEOUT_ENV,
    NoopClient,
};
pub use client_transports::{UnixSocketTransport, new_unix_socket_transport};
pub use cni::internal::{RealNetlinkOps, UnsupportedNetlinkOps};
pub use cni::*;
pub use command_builder::{
    CommandStdio, DEFAULT_FC_BIN, VMCommand, VMCommandBuilder, seccomp_args,
};
pub use config::{
    Config, DEFAULT_FIRECRACKER_INIT_TIMEOUT_SECONDS, DEFAULT_FORWARD_SIGNALS, DEFAULT_NET_NS_DIR,
    FIRECRACKER_INIT_TIMEOUT_ENV, FifoLogWriter, MMDSVersion, SeccompConfig,
};
pub use drives::{
    DriveOpt, DrivesBuilder, ROOT_DRIVE_NAME, with_cache_type, with_drive_id, with_io_engine,
    with_partuuid, with_rate_limiter, with_read_only,
};
pub use error::{Error, Result};
pub use handlers::{
    ADD_VSOCKS_HANDLER_NAME, ATTACH_DRIVES_HANDLER_NAME, BOOTSTRAP_LOGGING_HANDLER_NAME,
    CONFIG_MMDS_HANDLER_NAME, CREATE_BOOT_SOURCE_HANDLER_NAME, CREATE_LOG_FILES_HANDLER_NAME,
    CREATE_MACHINE_HANDLER_NAME, CREATE_NETWORK_INTERFACES_HANDLER_NAME, Handler, HandlerList,
    Handlers, LINK_FILES_TO_ROOTFS_HANDLER_NAME, LOAD_SNAPSHOT_HANDLER_NAME,
    NEW_SET_METADATA_HANDLER_NAME, add_vsocks_handler, attach_drives_handler,
    bootstrap_logging_handler, config_mmds_handler, create_boot_source_handler,
    create_log_files_handler, create_machine_handler, create_network_interfaces_handler,
    default_handlers, new_set_metadata_handler,
};
pub use internal::{find_first_vendor_id, support_cpu_template};
pub use jailer::{
    DEFAULT_JAILER_BIN, DEFAULT_JAILER_PATH, DEFAULT_SOCKET_PATH, HandlersAdapter,
    JailerCommandBuilder, JailerConfig, LINK_FILES_HANDLER_NAME, NaiveChrootStrategy,
    ROOTFS_FOLDER_NAME, get_numa_cpuset, jail,
};
pub use kernelargs::{KernelArgs, parse_kernel_args};
pub use machine::{Machine, RateLimiterSet};
pub use machineiface::MachineIface;
pub use models::*;
pub use network::{
    CleanupFn, CniConfiguration, CniNetworkOperations, CniRuntimeConf, DEFAULT_CNI_BIN_DIR,
    DEFAULT_CNI_CACHE_DIR, DEFAULT_CNI_CONF_DIR, IPConfiguration, NetworkInterface,
    NetworkInterfaces, RealCniNetworkOperations, StaticNetworkConfiguration,
    UnsupportedCniNetworkOperations,
};
pub use opts::{
    Opt, SnapshotOpt, new_machine, with_client, with_cni_network_ops, with_logger,
    with_memory_backend, with_netlink_ops, with_process_runner, with_snapshot,
};
pub use pointer_helpers::{
    bool_ptr, bool_value, int_ptr, int_value, int64_ptr, int64_value, string_ptr, string_value,
};
pub use rate_limiter::{RateLimiterOpt, TokenBucketBuilder, new_rate_limiter};
pub use snapshot::SnapshotConfig;
pub use utils::{env_value_or_default_int, wait_for_alive_vmm};
pub use version::VERSION;
pub use vsock::{
    AckError, ConnectMessageError, DialConfig, DialOption, VsockDevice, VsockListener, VsockStream,
    connect_message, dial, dial_with_config, dial_with_options, is_temporary_net_error, listen,
    listen_with_config, listen_with_options, with_ack_msg_timeout, with_connection_msg_timeout,
    with_dial_timeout, with_retry_interval, with_retry_timeout,
};
