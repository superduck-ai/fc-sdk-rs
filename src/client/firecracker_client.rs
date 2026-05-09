use std::time::Duration;

use crate::client_transports::{UnixSocketTransport, new_unix_socket_transport};
use crate::error::Result;
use crate::models::{
    Balloon, BalloonStats, BalloonStatsUpdate, BalloonUpdate, BootSource, CpuConfig, Drive,
    EntropyDevice, FirecrackerVersion, FullVmConfiguration, InstanceActionInfo, InstanceInfo,
    Logger, MachineConfiguration, Metrics, MmdsConfig, NetworkInterfaceModel, PartialDrive,
    PartialNetworkInterface, SnapshotCreateParams, SnapshotLoadParams, Vm, VsockModel,
};
use crate::utils::env_value_or_default_int;

pub const FIRECRACKER_REQUEST_TIMEOUT_ENV: &str = "FIRECRACKER_GO_SDK_REQUEST_TIMEOUT_MILLISECONDS";
pub const DEFAULT_FIRECRACKER_REQUEST_TIMEOUT: i32 = 500;

pub trait ClientOps: Send {
    fn get_firecracker_version(&mut self) -> Result<FirecrackerVersion> {
        Ok(FirecrackerVersion {
            firecracker_version: String::new(),
        })
    }

    fn put_logger(&mut self, _logger: &Logger) -> Result<()> {
        Ok(())
    }

    fn put_metrics(&mut self, _metrics: &Metrics) -> Result<()> {
        Ok(())
    }

    fn put_machine_configuration(&mut self, _config: &MachineConfiguration) -> Result<()> {
        Ok(())
    }

    fn patch_machine_configuration(&mut self, _config: &MachineConfiguration) -> Result<()> {
        Ok(())
    }

    fn get_machine_configuration(&mut self) -> Result<MachineConfiguration> {
        Ok(MachineConfiguration::default())
    }

    fn put_guest_boot_source(&mut self, _boot_source: &BootSource) -> Result<()> {
        Ok(())
    }

    fn put_cpu_configuration(&mut self, _config: &CpuConfig) -> Result<()> {
        Ok(())
    }

    fn put_entropy_device(&mut self, _device: &EntropyDevice) -> Result<()> {
        Ok(())
    }

    fn put_guest_drive_by_id(&mut self, _drive_id: &str, _drive: &Drive) -> Result<()> {
        Ok(())
    }

    fn patch_guest_drive_by_id(&mut self, _drive_id: &str, _drive: &PartialDrive) -> Result<()> {
        Ok(())
    }

    fn put_guest_network_interface_by_id(
        &mut self,
        _iface_id: &str,
        _iface: &NetworkInterfaceModel,
    ) -> Result<()> {
        Ok(())
    }

    fn patch_guest_network_interface_by_id(
        &mut self,
        _iface_id: &str,
        _iface: &PartialNetworkInterface,
    ) -> Result<()> {
        Ok(())
    }

    fn put_guest_vsock(&mut self, _vsock: &VsockModel) -> Result<()> {
        Ok(())
    }

    fn patch_vm(&mut self, _vm: &Vm) -> Result<()> {
        Ok(())
    }

    fn create_snapshot(&mut self, _snapshot: &SnapshotCreateParams) -> Result<()> {
        Ok(())
    }

    fn load_snapshot(&mut self, _snapshot: &SnapshotLoadParams) -> Result<()> {
        Ok(())
    }

    fn create_sync_action(&mut self, _action: &InstanceActionInfo) -> Result<()> {
        Ok(())
    }

    fn put_mmds(&mut self, _metadata: &serde_json::Value) -> Result<()> {
        Ok(())
    }

    fn get_mmds(&mut self) -> Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }

    fn patch_mmds(&mut self, _metadata: &serde_json::Value) -> Result<()> {
        Ok(())
    }

    fn put_mmds_config(&mut self, _config: &MmdsConfig) -> Result<()> {
        Ok(())
    }

    fn describe_instance(&mut self) -> Result<InstanceInfo> {
        Ok(InstanceInfo::default())
    }

    fn put_balloon(&mut self, _balloon: &Balloon) -> Result<()> {
        Ok(())
    }

    fn get_balloon_config(&mut self) -> Result<Balloon> {
        Ok(Balloon::default())
    }

    fn patch_balloon(&mut self, _balloon_update: &BalloonUpdate) -> Result<()> {
        Ok(())
    }

    fn get_balloon_stats(&mut self) -> Result<BalloonStats> {
        Ok(BalloonStats::default())
    }

    fn patch_balloon_stats_interval(
        &mut self,
        _balloon_stats_update: &BalloonStatsUpdate,
    ) -> Result<()> {
        Ok(())
    }

    fn get_export_vm_config(&mut self) -> Result<FullVmConfiguration> {
        Ok(FullVmConfiguration::default())
    }
}

#[derive(Debug, Clone)]
pub struct Client {
    transport: UnixSocketTransport,
    init_timeout: Duration,
}

impl Client {
    pub fn new(socket_path: impl Into<String>) -> Self {
        let request_timeout_ms = env_value_or_default_int(
            FIRECRACKER_REQUEST_TIMEOUT_ENV,
            DEFAULT_FIRECRACKER_REQUEST_TIMEOUT,
        );
        let init_timeout_seconds = env_value_or_default_int(
            crate::config::FIRECRACKER_INIT_TIMEOUT_ENV,
            crate::config::DEFAULT_FIRECRACKER_INIT_TIMEOUT_SECONDS as i32,
        );

        Self {
            transport: new_unix_socket_transport(
                socket_path.into(),
                Duration::from_millis(request_timeout_ms as u64),
            ),
            init_timeout: Duration::from_secs(init_timeout_seconds as u64),
        }
    }

    pub fn socket_path(&self) -> &str {
        self.transport.socket_path()
    }

    pub fn request_timeout(&self) -> Duration {
        self.transport.request_timeout()
    }

    fn drive_request_timeout(&self) -> Duration {
        self.request_timeout() / 2
    }

    pub fn init_timeout(&self) -> Duration {
        self.init_timeout
    }

    pub fn raw_request(&self, method: &str, path: &str, body: Option<&[u8]>) -> Result<Vec<u8>> {
        self.transport.raw_request(method, path, body)
    }

    pub fn raw_json_request<T: serde::Serialize>(
        &self,
        method: &str,
        path: &str,
        body: &T,
    ) -> Result<Vec<u8>> {
        self.transport.raw_json_request(method, path, body)
    }

    fn raw_json_request_with_timeouts<T: serde::Serialize>(
        &self,
        method: &str,
        path: &str,
        body: &T,
        read_timeout: Option<Duration>,
        write_timeout: Option<Duration>,
    ) -> Result<Vec<u8>> {
        self.transport.raw_json_request_with_timeouts(
            method,
            path,
            body,
            read_timeout,
            write_timeout,
        )
    }

    pub fn get_firecracker_version(&self) -> Result<FirecrackerVersion> {
        let body = self.raw_request("GET", "/version", None)?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn put_logger(&self, logger: &Logger) -> Result<()> {
        self.raw_json_request("PUT", "/logger", logger).map(|_| ())
    }

    pub fn put_metrics(&self, metrics: &Metrics) -> Result<()> {
        self.raw_json_request("PUT", "/metrics", metrics)
            .map(|_| ())
    }

    pub fn put_machine_configuration(&self, config: &MachineConfiguration) -> Result<()> {
        self.raw_json_request("PUT", "/machine-config", config)
            .map(|_| ())
    }

    pub fn patch_machine_configuration(&self, config: &MachineConfiguration) -> Result<()> {
        self.raw_json_request("PATCH", "/machine-config", config)
            .map(|_| ())
    }

    pub fn get_machine_configuration(&self) -> Result<MachineConfiguration> {
        let body = self.raw_request("GET", "/machine-config", None)?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn put_guest_boot_source(&self, boot_source: &BootSource) -> Result<()> {
        self.raw_json_request("PUT", "/boot-source", boot_source)
            .map(|_| ())
    }

    pub fn put_cpu_configuration(&self, config: &CpuConfig) -> Result<()> {
        self.raw_json_request("PUT", "/cpu-config", config)
            .map(|_| ())
    }

    pub fn put_entropy_device(&self, device: &EntropyDevice) -> Result<()> {
        self.raw_json_request("PUT", "/entropy", device).map(|_| ())
    }

    pub fn put_guest_drive_by_id(&self, drive_id: &str, drive: &Drive) -> Result<()> {
        let timeout = self.drive_request_timeout();
        self.raw_json_request_with_timeouts(
            "PUT",
            &format!("/drives/{drive_id}"),
            drive,
            Some(timeout),
            Some(timeout),
        )
        .map(|_| ())
    }

    pub fn patch_guest_drive_by_id(&self, drive_id: &str, drive: &PartialDrive) -> Result<()> {
        self.raw_json_request("PATCH", &format!("/drives/{drive_id}"), drive)
            .map(|_| ())
    }

    pub fn put_guest_network_interface_by_id(
        &self,
        iface_id: &str,
        iface: &NetworkInterfaceModel,
    ) -> Result<()> {
        self.raw_json_request("PUT", &format!("/network-interfaces/{iface_id}"), iface)
            .map(|_| ())
    }

    pub fn patch_guest_network_interface_by_id(
        &self,
        iface_id: &str,
        iface: &PartialNetworkInterface,
    ) -> Result<()> {
        self.raw_json_request("PATCH", &format!("/network-interfaces/{iface_id}"), iface)
            .map(|_| ())
    }

    pub fn put_guest_vsock(&self, vsock: &VsockModel) -> Result<()> {
        self.raw_json_request("PUT", "/vsock", vsock).map(|_| ())
    }

    pub fn patch_vm(&self, vm: &Vm) -> Result<()> {
        self.raw_json_request("PATCH", "/vm", vm).map(|_| ())
    }

    pub fn create_snapshot(&self, snapshot: &SnapshotCreateParams) -> Result<()> {
        self.raw_json_request_with_timeouts(
            "PUT",
            "/snapshot/create",
            snapshot,
            None,
            Some(self.request_timeout()),
        )
        .map(|_| ())
    }

    pub fn load_snapshot(&self, snapshot: &SnapshotLoadParams) -> Result<()> {
        self.raw_json_request_with_timeouts(
            "PUT",
            "/snapshot/load",
            snapshot,
            None,
            Some(self.request_timeout()),
        )
        .map(|_| ())
    }

    pub fn create_sync_action(&self, action: &InstanceActionInfo) -> Result<()> {
        self.raw_json_request("PUT", "/actions", action).map(|_| ())
    }

    pub fn put_mmds(&self, metadata: &serde_json::Value) -> Result<()> {
        self.raw_json_request("PUT", "/mmds", metadata).map(|_| ())
    }

    pub fn get_mmds(&self) -> Result<serde_json::Value> {
        let body = self.raw_request("GET", "/mmds", None)?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn patch_mmds(&self, metadata: &serde_json::Value) -> Result<()> {
        self.raw_json_request("PATCH", "/mmds", metadata)
            .map(|_| ())
    }

    pub fn put_mmds_config(&self, config: &MmdsConfig) -> Result<()> {
        self.raw_json_request("PUT", "/mmds/config", config)
            .map(|_| ())
    }

    pub fn describe_instance(&self) -> Result<InstanceInfo> {
        let body = self.raw_request("GET", "/", None)?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn get_instance_info(&self) -> Result<InstanceInfo> {
        self.describe_instance()
    }

    pub fn put_balloon(&self, balloon: &Balloon) -> Result<()> {
        self.raw_json_request("PUT", "/balloon", balloon)
            .map(|_| ())
    }

    pub fn get_balloon_config(&self) -> Result<Balloon> {
        let body = self.raw_request("GET", "/balloon", None)?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn describe_balloon_config(&self) -> Result<Balloon> {
        self.get_balloon_config()
    }

    pub fn patch_balloon(&self, balloon_update: &BalloonUpdate) -> Result<()> {
        self.raw_json_request("PATCH", "/balloon", balloon_update)
            .map(|_| ())
    }

    pub fn get_balloon_stats(&self) -> Result<BalloonStats> {
        let body = self.raw_request("GET", "/balloon/statistics", None)?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn describe_balloon_stats(&self) -> Result<BalloonStats> {
        self.get_balloon_stats()
    }

    pub fn patch_balloon_stats_interval(
        &self,
        balloon_stats_update: &BalloonStatsUpdate,
    ) -> Result<()> {
        self.raw_json_request("PATCH", "/balloon/statistics", balloon_stats_update)
            .map(|_| ())
    }

    pub fn get_export_vm_config(&self) -> Result<FullVmConfiguration> {
        let body = self.raw_request("GET", "/vm/config", None)?;
        Ok(serde_json::from_slice(&body)?)
    }
}

impl ClientOps for Client {
    fn get_firecracker_version(&mut self) -> Result<FirecrackerVersion> {
        Client::get_firecracker_version(self)
    }

    fn put_logger(&mut self, logger: &Logger) -> Result<()> {
        Client::put_logger(self, logger)
    }

    fn put_metrics(&mut self, metrics: &Metrics) -> Result<()> {
        Client::put_metrics(self, metrics)
    }

    fn put_machine_configuration(&mut self, config: &MachineConfiguration) -> Result<()> {
        Client::put_machine_configuration(self, config)
    }

    fn patch_machine_configuration(&mut self, config: &MachineConfiguration) -> Result<()> {
        Client::patch_machine_configuration(self, config)
    }

    fn get_machine_configuration(&mut self) -> Result<MachineConfiguration> {
        Client::get_machine_configuration(self)
    }

    fn put_guest_boot_source(&mut self, boot_source: &BootSource) -> Result<()> {
        Client::put_guest_boot_source(self, boot_source)
    }

    fn put_cpu_configuration(&mut self, config: &CpuConfig) -> Result<()> {
        Client::put_cpu_configuration(self, config)
    }

    fn put_entropy_device(&mut self, device: &EntropyDevice) -> Result<()> {
        Client::put_entropy_device(self, device)
    }

    fn put_guest_drive_by_id(&mut self, drive_id: &str, drive: &Drive) -> Result<()> {
        Client::put_guest_drive_by_id(self, drive_id, drive)
    }

    fn patch_guest_drive_by_id(&mut self, drive_id: &str, drive: &PartialDrive) -> Result<()> {
        Client::patch_guest_drive_by_id(self, drive_id, drive)
    }

    fn put_guest_network_interface_by_id(
        &mut self,
        iface_id: &str,
        iface: &NetworkInterfaceModel,
    ) -> Result<()> {
        Client::put_guest_network_interface_by_id(self, iface_id, iface)
    }

    fn patch_guest_network_interface_by_id(
        &mut self,
        iface_id: &str,
        iface: &PartialNetworkInterface,
    ) -> Result<()> {
        Client::patch_guest_network_interface_by_id(self, iface_id, iface)
    }

    fn put_guest_vsock(&mut self, vsock: &VsockModel) -> Result<()> {
        Client::put_guest_vsock(self, vsock)
    }

    fn patch_vm(&mut self, vm: &Vm) -> Result<()> {
        Client::patch_vm(self, vm)
    }

    fn create_snapshot(&mut self, snapshot: &SnapshotCreateParams) -> Result<()> {
        Client::create_snapshot(self, snapshot)
    }

    fn load_snapshot(&mut self, snapshot: &SnapshotLoadParams) -> Result<()> {
        Client::load_snapshot(self, snapshot)
    }

    fn create_sync_action(&mut self, action: &InstanceActionInfo) -> Result<()> {
        Client::create_sync_action(self, action)
    }

    fn put_mmds(&mut self, metadata: &serde_json::Value) -> Result<()> {
        Client::put_mmds(self, metadata)
    }

    fn get_mmds(&mut self) -> Result<serde_json::Value> {
        Client::get_mmds(self)
    }

    fn patch_mmds(&mut self, metadata: &serde_json::Value) -> Result<()> {
        Client::patch_mmds(self, metadata)
    }

    fn put_mmds_config(&mut self, config: &MmdsConfig) -> Result<()> {
        Client::put_mmds_config(self, config)
    }

    fn describe_instance(&mut self) -> Result<InstanceInfo> {
        Client::describe_instance(self)
    }

    fn put_balloon(&mut self, balloon: &Balloon) -> Result<()> {
        Client::put_balloon(self, balloon)
    }

    fn get_balloon_config(&mut self) -> Result<Balloon> {
        Client::get_balloon_config(self)
    }

    fn patch_balloon(&mut self, balloon_update: &BalloonUpdate) -> Result<()> {
        Client::patch_balloon(self, balloon_update)
    }

    fn get_balloon_stats(&mut self) -> Result<BalloonStats> {
        Client::get_balloon_stats(self)
    }

    fn patch_balloon_stats_interval(
        &mut self,
        balloon_stats_update: &BalloonStatsUpdate,
    ) -> Result<()> {
        Client::patch_balloon_stats_interval(self, balloon_stats_update)
    }

    fn get_export_vm_config(&mut self) -> Result<FullVmConfiguration> {
        Client::get_export_vm_config(self)
    }
}

#[derive(Debug, Default)]
pub struct NoopClient;

impl ClientOps for NoopClient {}
