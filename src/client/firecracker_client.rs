use std::time::Duration;

use async_trait::async_trait;

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

pub trait ApplyClientOpt {
    fn apply(self: Box<Self>, client: &mut Client);
}

impl<F> ApplyClientOpt for F
where
    F: FnOnce(&mut Client),
{
    fn apply(self: Box<Self>, client: &mut Client) {
        (*self)(client);
    }
}

pub type ClientOpt = Box<dyn ApplyClientOpt + Send + 'static>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequestOptions {
    read_timeout_override: Option<Option<Duration>>,
    write_timeout_override: Option<Option<Duration>>,
}

impl RequestOptions {
    pub fn from_opts(opts: impl IntoIterator<Item = RequestOpt>) -> Self {
        let mut options = Self::default();
        for opt in opts {
            opt.apply(&mut options);
        }
        options
    }

    fn resolve(
        self,
        default_read_timeout: Option<Duration>,
        default_write_timeout: Option<Duration>,
    ) -> (Option<Duration>, Option<Duration>) {
        (
            self.read_timeout_override.unwrap_or(default_read_timeout),
            self.write_timeout_override.unwrap_or(default_write_timeout),
        )
    }
}

pub trait ApplyRequestOpt {
    fn apply(self: Box<Self>, options: &mut RequestOptions);
}

impl<F> ApplyRequestOpt for F
where
    F: FnOnce(&mut RequestOptions),
{
    fn apply(self: Box<Self>, options: &mut RequestOptions) {
        (*self)(options);
    }
}

pub type RequestOpt = Box<dyn ApplyRequestOpt + Send + 'static>;
pub type PatchGuestDriveByIdOpt = RequestOpt;
pub type PatchGuestNetworkInterfaceByIdOpt = RequestOpt;
pub type PatchVmOpt = RequestOpt;
pub type CreateSnapshotOpt = RequestOpt;
pub type PutBalloonOpt = RequestOpt;
pub type PatchBalloonOpt = RequestOpt;
pub type PatchBalloonStatsIntervalOpt = RequestOpt;

#[async_trait]
pub trait ClientOps: Send {
    async fn get_firecracker_version(&mut self) -> Result<FirecrackerVersion> {
        Ok(FirecrackerVersion {
            firecracker_version: String::new(),
        })
    }

    async fn put_logger(&mut self, _logger: &Logger) -> Result<()> {
        Ok(())
    }

    async fn put_metrics(&mut self, _metrics: &Metrics) -> Result<()> {
        Ok(())
    }

    async fn put_machine_configuration(&mut self, _config: &MachineConfiguration) -> Result<()> {
        Ok(())
    }

    async fn patch_machine_configuration(&mut self, _config: &MachineConfiguration) -> Result<()> {
        Ok(())
    }

    async fn get_machine_configuration(&mut self) -> Result<MachineConfiguration> {
        Ok(MachineConfiguration::default())
    }

    async fn put_guest_boot_source(&mut self, _boot_source: &BootSource) -> Result<()> {
        Ok(())
    }

    async fn put_cpu_configuration(&mut self, _config: &CpuConfig) -> Result<()> {
        Ok(())
    }

    async fn put_entropy_device(&mut self, _device: &EntropyDevice) -> Result<()> {
        Ok(())
    }

    async fn put_guest_drive_by_id(&mut self, _drive_id: &str, _drive: &Drive) -> Result<()> {
        Ok(())
    }

    async fn patch_guest_drive_by_id(
        &mut self,
        _drive_id: &str,
        _drive: &PartialDrive,
    ) -> Result<()> {
        Ok(())
    }

    async fn patch_guest_drive_by_id_with_options(
        &mut self,
        drive_id: &str,
        drive: &PartialDrive,
        _options: RequestOptions,
    ) -> Result<()> {
        self.patch_guest_drive_by_id(drive_id, drive).await
    }

    async fn put_guest_network_interface_by_id(
        &mut self,
        _iface_id: &str,
        _iface: &NetworkInterfaceModel,
    ) -> Result<()> {
        Ok(())
    }

    async fn patch_guest_network_interface_by_id(
        &mut self,
        _iface_id: &str,
        _iface: &PartialNetworkInterface,
    ) -> Result<()> {
        Ok(())
    }

    async fn patch_guest_network_interface_by_id_with_options(
        &mut self,
        iface_id: &str,
        iface: &PartialNetworkInterface,
        _options: RequestOptions,
    ) -> Result<()> {
        self.patch_guest_network_interface_by_id(iface_id, iface)
            .await
    }

    async fn put_guest_vsock(&mut self, _vsock: &VsockModel) -> Result<()> {
        Ok(())
    }

    async fn patch_vm(&mut self, _vm: &Vm) -> Result<()> {
        Ok(())
    }

    async fn patch_vm_with_options(&mut self, vm: &Vm, _options: RequestOptions) -> Result<()> {
        self.patch_vm(vm).await
    }

    async fn create_snapshot(&mut self, _snapshot: &SnapshotCreateParams) -> Result<()> {
        Ok(())
    }

    async fn create_snapshot_with_options(
        &mut self,
        snapshot: &SnapshotCreateParams,
        _options: RequestOptions,
    ) -> Result<()> {
        self.create_snapshot(snapshot).await
    }

    async fn load_snapshot(&mut self, _snapshot: &SnapshotLoadParams) -> Result<()> {
        Ok(())
    }

    async fn create_sync_action(&mut self, _action: &InstanceActionInfo) -> Result<()> {
        Ok(())
    }

    async fn put_mmds(&mut self, _metadata: &serde_json::Value) -> Result<()> {
        Ok(())
    }

    async fn get_mmds(&mut self) -> Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }

    async fn patch_mmds(&mut self, _metadata: &serde_json::Value) -> Result<()> {
        Ok(())
    }

    async fn put_mmds_config(&mut self, _config: &MmdsConfig) -> Result<()> {
        Ok(())
    }

    async fn describe_instance(&mut self) -> Result<InstanceInfo> {
        Ok(InstanceInfo::default())
    }

    async fn put_balloon(&mut self, _balloon: &Balloon) -> Result<()> {
        Ok(())
    }

    async fn put_balloon_with_options(
        &mut self,
        balloon: &Balloon,
        _options: RequestOptions,
    ) -> Result<()> {
        self.put_balloon(balloon).await
    }

    async fn get_balloon_config(&mut self) -> Result<Balloon> {
        Ok(Balloon::default())
    }

    async fn patch_balloon(&mut self, _balloon_update: &BalloonUpdate) -> Result<()> {
        Ok(())
    }

    async fn patch_balloon_with_options(
        &mut self,
        balloon_update: &BalloonUpdate,
        _options: RequestOptions,
    ) -> Result<()> {
        self.patch_balloon(balloon_update).await
    }

    async fn get_balloon_stats(&mut self) -> Result<BalloonStats> {
        Ok(BalloonStats::default())
    }

    async fn patch_balloon_stats_interval(
        &mut self,
        _balloon_stats_update: &BalloonStatsUpdate,
    ) -> Result<()> {
        Ok(())
    }

    async fn patch_balloon_stats_interval_with_options(
        &mut self,
        balloon_stats_update: &BalloonStatsUpdate,
        _options: RequestOptions,
    ) -> Result<()> {
        self.patch_balloon_stats_interval(balloon_stats_update)
            .await
    }

    async fn get_export_vm_config(&mut self) -> Result<FullVmConfiguration> {
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

    pub fn new_with_opts(
        socket_path: impl Into<String>,
        opts: impl IntoIterator<Item = ClientOpt>,
    ) -> Self {
        let mut client = Self::new(socket_path);
        for opt in opts {
            opt.apply(&mut client);
        }
        client
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

    pub async fn raw_request(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        self.transport.raw_request(method, path, body).await
    }

    pub async fn raw_json_request<T: serde::Serialize>(
        &self,
        method: &str,
        path: &str,
        body: &T,
    ) -> Result<Vec<u8>> {
        self.transport.raw_json_request(method, path, body).await
    }

    async fn raw_json_request_with_timeouts<T: serde::Serialize>(
        &self,
        method: &str,
        path: &str,
        body: &T,
        read_timeout: Option<Duration>,
        write_timeout: Option<Duration>,
    ) -> Result<Vec<u8>> {
        self.transport
            .raw_json_request_with_timeouts(method, path, body, read_timeout, write_timeout)
            .await
    }

    pub async fn get_firecracker_version(&self) -> Result<FirecrackerVersion> {
        let body = self.raw_request("GET", "/version", None).await?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub async fn put_logger(&self, logger: &Logger) -> Result<()> {
        self.raw_json_request("PUT", "/logger", logger)
            .await
            .map(|_| ())
    }

    pub async fn put_metrics(&self, metrics: &Metrics) -> Result<()> {
        self.raw_json_request("PUT", "/metrics", metrics)
            .await
            .map(|_| ())
    }

    pub async fn put_machine_configuration(&self, config: &MachineConfiguration) -> Result<()> {
        self.raw_json_request("PUT", "/machine-config", config)
            .await
            .map(|_| ())
    }

    pub async fn patch_machine_configuration(&self, config: &MachineConfiguration) -> Result<()> {
        self.raw_json_request("PATCH", "/machine-config", config)
            .await
            .map(|_| ())
    }

    pub async fn get_machine_configuration(&self) -> Result<MachineConfiguration> {
        let body = self.raw_request("GET", "/machine-config", None).await?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub async fn put_guest_boot_source(&self, boot_source: &BootSource) -> Result<()> {
        self.raw_json_request("PUT", "/boot-source", boot_source)
            .await
            .map(|_| ())
    }

    pub async fn put_cpu_configuration(&self, config: &CpuConfig) -> Result<()> {
        self.raw_json_request("PUT", "/cpu-config", config)
            .await
            .map(|_| ())
    }

    pub async fn put_entropy_device(&self, device: &EntropyDevice) -> Result<()> {
        self.raw_json_request("PUT", "/entropy", device)
            .await
            .map(|_| ())
    }

    pub async fn put_guest_drive_by_id(&self, drive_id: &str, drive: &Drive) -> Result<()> {
        let timeout = self.drive_request_timeout();
        self.raw_json_request_with_timeouts(
            "PUT",
            &format!("/drives/{drive_id}"),
            drive,
            Some(timeout),
            Some(timeout),
        )
        .await
        .map(|_| ())
    }

    pub async fn patch_guest_drive_by_id(
        &self,
        drive_id: &str,
        drive: &PartialDrive,
    ) -> Result<()> {
        self.patch_guest_drive_by_id_with_options(drive_id, drive, RequestOptions::default())
            .await
    }

    pub async fn patch_guest_drive_by_id_with_options(
        &self,
        drive_id: &str,
        drive: &PartialDrive,
        options: RequestOptions,
    ) -> Result<()> {
        let (read_timeout, write_timeout) =
            options.resolve(Some(self.request_timeout()), Some(self.request_timeout()));
        self.raw_json_request_with_timeouts(
            "PATCH",
            &format!("/drives/{drive_id}"),
            drive,
            read_timeout,
            write_timeout,
        )
        .await
        .map(|_| ())
    }

    pub async fn put_guest_network_interface_by_id(
        &self,
        iface_id: &str,
        iface: &NetworkInterfaceModel,
    ) -> Result<()> {
        self.raw_json_request("PUT", &format!("/network-interfaces/{iface_id}"), iface)
            .await
            .map(|_| ())
    }

    pub async fn patch_guest_network_interface_by_id(
        &self,
        iface_id: &str,
        iface: &PartialNetworkInterface,
    ) -> Result<()> {
        self.patch_guest_network_interface_by_id_with_options(
            iface_id,
            iface,
            RequestOptions::default(),
        )
        .await
    }

    pub async fn patch_guest_network_interface_by_id_with_options(
        &self,
        iface_id: &str,
        iface: &PartialNetworkInterface,
        options: RequestOptions,
    ) -> Result<()> {
        let (read_timeout, write_timeout) =
            options.resolve(Some(self.request_timeout()), Some(self.request_timeout()));
        self.raw_json_request_with_timeouts(
            "PATCH",
            &format!("/network-interfaces/{iface_id}"),
            iface,
            read_timeout,
            write_timeout,
        )
        .await
        .map(|_| ())
    }

    pub async fn put_guest_vsock(&self, vsock: &VsockModel) -> Result<()> {
        self.raw_json_request("PUT", "/vsock", vsock)
            .await
            .map(|_| ())
    }

    pub async fn patch_vm(&self, vm: &Vm) -> Result<()> {
        self.patch_vm_with_options(vm, RequestOptions::default())
            .await
    }

    pub async fn patch_vm_with_options(&self, vm: &Vm, options: RequestOptions) -> Result<()> {
        let (read_timeout, write_timeout) =
            options.resolve(Some(self.request_timeout()), Some(self.request_timeout()));
        self.raw_json_request_with_timeouts("PATCH", "/vm", vm, read_timeout, write_timeout)
            .await
            .map(|_| ())
    }

    pub async fn create_snapshot(&self, snapshot: &SnapshotCreateParams) -> Result<()> {
        self.create_snapshot_with_options(snapshot, RequestOptions::default())
            .await
    }

    pub async fn create_snapshot_with_options(
        &self,
        snapshot: &SnapshotCreateParams,
        options: RequestOptions,
    ) -> Result<()> {
        let (read_timeout, write_timeout) = options.resolve(None, Some(self.request_timeout()));
        self.raw_json_request_with_timeouts(
            "PUT",
            "/snapshot/create",
            snapshot,
            read_timeout,
            write_timeout,
        )
        .await
        .map(|_| ())
    }

    pub async fn load_snapshot(&self, snapshot: &SnapshotLoadParams) -> Result<()> {
        self.raw_json_request_with_timeouts(
            "PUT",
            "/snapshot/load",
            snapshot,
            None,
            Some(self.request_timeout()),
        )
        .await
        .map(|_| ())
    }

    pub async fn create_sync_action(&self, action: &InstanceActionInfo) -> Result<()> {
        self.raw_json_request("PUT", "/actions", action)
            .await
            .map(|_| ())
    }

    pub async fn put_mmds(&self, metadata: &serde_json::Value) -> Result<()> {
        self.raw_json_request("PUT", "/mmds", metadata)
            .await
            .map(|_| ())
    }

    pub async fn get_mmds(&self) -> Result<serde_json::Value> {
        let body = self.raw_request("GET", "/mmds", None).await?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub async fn patch_mmds(&self, metadata: &serde_json::Value) -> Result<()> {
        self.raw_json_request("PATCH", "/mmds", metadata)
            .await
            .map(|_| ())
    }

    pub async fn put_mmds_config(&self, config: &MmdsConfig) -> Result<()> {
        self.raw_json_request("PUT", "/mmds/config", config)
            .await
            .map(|_| ())
    }

    pub async fn describe_instance(&self) -> Result<InstanceInfo> {
        let body = self.raw_request("GET", "/", None).await?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub async fn get_instance_info(&self) -> Result<InstanceInfo> {
        self.describe_instance().await
    }

    pub async fn put_balloon(&self, balloon: &Balloon) -> Result<()> {
        self.put_balloon_with_options(balloon, RequestOptions::default())
            .await
    }

    pub async fn put_balloon_with_options(
        &self,
        balloon: &Balloon,
        options: RequestOptions,
    ) -> Result<()> {
        let (read_timeout, write_timeout) =
            options.resolve(Some(self.request_timeout()), Some(self.request_timeout()));
        self.raw_json_request_with_timeouts("PUT", "/balloon", balloon, read_timeout, write_timeout)
            .await
            .map(|_| ())
    }

    pub async fn get_balloon_config(&self) -> Result<Balloon> {
        let body = self.raw_request("GET", "/balloon", None).await?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub async fn describe_balloon_config(&self) -> Result<Balloon> {
        self.get_balloon_config().await
    }

    pub async fn patch_balloon(&self, balloon_update: &BalloonUpdate) -> Result<()> {
        self.patch_balloon_with_options(balloon_update, RequestOptions::default())
            .await
    }

    pub async fn patch_balloon_with_options(
        &self,
        balloon_update: &BalloonUpdate,
        options: RequestOptions,
    ) -> Result<()> {
        let (read_timeout, write_timeout) =
            options.resolve(Some(self.request_timeout()), Some(self.request_timeout()));
        self.raw_json_request_with_timeouts(
            "PATCH",
            "/balloon",
            balloon_update,
            read_timeout,
            write_timeout,
        )
        .await
        .map(|_| ())
    }

    pub async fn get_balloon_stats(&self) -> Result<BalloonStats> {
        let body = self.raw_request("GET", "/balloon/statistics", None).await?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub async fn describe_balloon_stats(&self) -> Result<BalloonStats> {
        self.get_balloon_stats().await
    }

    pub async fn patch_balloon_stats_interval(
        &self,
        balloon_stats_update: &BalloonStatsUpdate,
    ) -> Result<()> {
        self.patch_balloon_stats_interval_with_options(
            balloon_stats_update,
            RequestOptions::default(),
        )
        .await
    }

    pub async fn patch_balloon_stats_interval_with_options(
        &self,
        balloon_stats_update: &BalloonStatsUpdate,
        options: RequestOptions,
    ) -> Result<()> {
        let (read_timeout, write_timeout) =
            options.resolve(Some(self.request_timeout()), Some(self.request_timeout()));
        self.raw_json_request_with_timeouts(
            "PATCH",
            "/balloon/statistics",
            balloon_stats_update,
            read_timeout,
            write_timeout,
        )
        .await
        .map(|_| ())
    }

    pub async fn get_export_vm_config(&self) -> Result<FullVmConfiguration> {
        let body = self.raw_request("GET", "/vm/config", None).await?;
        Ok(serde_json::from_slice(&body)?)
    }
}

pub fn with_request_timeout(request_timeout: Duration) -> ClientOpt {
    Box::new(move |client: &mut Client| {
        client.transport =
            new_unix_socket_transport(client.socket_path().to_string(), request_timeout);
    })
}

pub fn with_read_timeout(read_timeout: Duration) -> RequestOpt {
    Box::new(move |options: &mut RequestOptions| {
        options.read_timeout_override = Some(Some(read_timeout));
    })
}

pub fn without_read_timeout() -> RequestOpt {
    Box::new(move |options: &mut RequestOptions| {
        options.read_timeout_override = Some(None);
    })
}

pub fn with_write_timeout(write_timeout: Duration) -> RequestOpt {
    Box::new(move |options: &mut RequestOptions| {
        options.write_timeout_override = Some(Some(write_timeout));
    })
}

pub fn without_write_timeout() -> RequestOpt {
    Box::new(move |options: &mut RequestOptions| {
        options.write_timeout_override = Some(None);
    })
}

pub fn with_init_timeout(init_timeout: Duration) -> ClientOpt {
    Box::new(move |client: &mut Client| {
        client.init_timeout = init_timeout;
    })
}

pub fn with_unix_socket_transport(transport: UnixSocketTransport) -> ClientOpt {
    Box::new(move |client: &mut Client| {
        client.transport = transport;
    })
}

#[async_trait]
impl ClientOps for Client {
    async fn get_firecracker_version(&mut self) -> Result<FirecrackerVersion> {
        Client::get_firecracker_version(self).await
    }

    async fn put_logger(&mut self, logger: &Logger) -> Result<()> {
        Client::put_logger(self, logger).await
    }

    async fn put_metrics(&mut self, metrics: &Metrics) -> Result<()> {
        Client::put_metrics(self, metrics).await
    }

    async fn put_machine_configuration(&mut self, config: &MachineConfiguration) -> Result<()> {
        Client::put_machine_configuration(self, config).await
    }

    async fn patch_machine_configuration(&mut self, config: &MachineConfiguration) -> Result<()> {
        Client::patch_machine_configuration(self, config).await
    }

    async fn get_machine_configuration(&mut self) -> Result<MachineConfiguration> {
        Client::get_machine_configuration(self).await
    }

    async fn put_guest_boot_source(&mut self, boot_source: &BootSource) -> Result<()> {
        Client::put_guest_boot_source(self, boot_source).await
    }

    async fn put_cpu_configuration(&mut self, config: &CpuConfig) -> Result<()> {
        Client::put_cpu_configuration(self, config).await
    }

    async fn put_entropy_device(&mut self, device: &EntropyDevice) -> Result<()> {
        Client::put_entropy_device(self, device).await
    }

    async fn put_guest_drive_by_id(&mut self, drive_id: &str, drive: &Drive) -> Result<()> {
        Client::put_guest_drive_by_id(self, drive_id, drive).await
    }

    async fn patch_guest_drive_by_id(
        &mut self,
        drive_id: &str,
        drive: &PartialDrive,
    ) -> Result<()> {
        Client::patch_guest_drive_by_id(self, drive_id, drive).await
    }

    async fn patch_guest_drive_by_id_with_options(
        &mut self,
        drive_id: &str,
        drive: &PartialDrive,
        options: RequestOptions,
    ) -> Result<()> {
        Client::patch_guest_drive_by_id_with_options(self, drive_id, drive, options).await
    }

    async fn put_guest_network_interface_by_id(
        &mut self,
        iface_id: &str,
        iface: &NetworkInterfaceModel,
    ) -> Result<()> {
        Client::put_guest_network_interface_by_id(self, iface_id, iface).await
    }

    async fn patch_guest_network_interface_by_id(
        &mut self,
        iface_id: &str,
        iface: &PartialNetworkInterface,
    ) -> Result<()> {
        Client::patch_guest_network_interface_by_id(self, iface_id, iface).await
    }

    async fn patch_guest_network_interface_by_id_with_options(
        &mut self,
        iface_id: &str,
        iface: &PartialNetworkInterface,
        options: RequestOptions,
    ) -> Result<()> {
        Client::patch_guest_network_interface_by_id_with_options(self, iface_id, iface, options)
            .await
    }

    async fn put_guest_vsock(&mut self, vsock: &VsockModel) -> Result<()> {
        Client::put_guest_vsock(self, vsock).await
    }

    async fn patch_vm(&mut self, vm: &Vm) -> Result<()> {
        Client::patch_vm(self, vm).await
    }

    async fn patch_vm_with_options(&mut self, vm: &Vm, options: RequestOptions) -> Result<()> {
        Client::patch_vm_with_options(self, vm, options).await
    }

    async fn create_snapshot(&mut self, snapshot: &SnapshotCreateParams) -> Result<()> {
        Client::create_snapshot(self, snapshot).await
    }

    async fn create_snapshot_with_options(
        &mut self,
        snapshot: &SnapshotCreateParams,
        options: RequestOptions,
    ) -> Result<()> {
        Client::create_snapshot_with_options(self, snapshot, options).await
    }

    async fn load_snapshot(&mut self, snapshot: &SnapshotLoadParams) -> Result<()> {
        Client::load_snapshot(self, snapshot).await
    }

    async fn create_sync_action(&mut self, action: &InstanceActionInfo) -> Result<()> {
        Client::create_sync_action(self, action).await
    }

    async fn put_mmds(&mut self, metadata: &serde_json::Value) -> Result<()> {
        Client::put_mmds(self, metadata).await
    }

    async fn get_mmds(&mut self) -> Result<serde_json::Value> {
        Client::get_mmds(self).await
    }

    async fn patch_mmds(&mut self, metadata: &serde_json::Value) -> Result<()> {
        Client::patch_mmds(self, metadata).await
    }

    async fn put_mmds_config(&mut self, config: &MmdsConfig) -> Result<()> {
        Client::put_mmds_config(self, config).await
    }

    async fn describe_instance(&mut self) -> Result<InstanceInfo> {
        Client::describe_instance(self).await
    }

    async fn put_balloon(&mut self, balloon: &Balloon) -> Result<()> {
        Client::put_balloon(self, balloon).await
    }

    async fn put_balloon_with_options(
        &mut self,
        balloon: &Balloon,
        options: RequestOptions,
    ) -> Result<()> {
        Client::put_balloon_with_options(self, balloon, options).await
    }

    async fn get_balloon_config(&mut self) -> Result<Balloon> {
        Client::get_balloon_config(self).await
    }

    async fn patch_balloon(&mut self, balloon_update: &BalloonUpdate) -> Result<()> {
        Client::patch_balloon(self, balloon_update).await
    }

    async fn patch_balloon_with_options(
        &mut self,
        balloon_update: &BalloonUpdate,
        options: RequestOptions,
    ) -> Result<()> {
        Client::patch_balloon_with_options(self, balloon_update, options).await
    }

    async fn get_balloon_stats(&mut self) -> Result<BalloonStats> {
        Client::get_balloon_stats(self).await
    }

    async fn patch_balloon_stats_interval(
        &mut self,
        balloon_stats_update: &BalloonStatsUpdate,
    ) -> Result<()> {
        Client::patch_balloon_stats_interval(self, balloon_stats_update).await
    }

    async fn patch_balloon_stats_interval_with_options(
        &mut self,
        balloon_stats_update: &BalloonStatsUpdate,
        options: RequestOptions,
    ) -> Result<()> {
        Client::patch_balloon_stats_interval_with_options(self, balloon_stats_update, options).await
    }

    async fn get_export_vm_config(&mut self) -> Result<FullVmConfiguration> {
        Client::get_export_vm_config(self).await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        RequestOptions, with_read_timeout, with_write_timeout, without_read_timeout,
        without_write_timeout,
    };

    #[test]
    fn request_options_override_and_disable_timeouts_independently() {
        let options = RequestOptions::from_opts(vec![
            without_read_timeout(),
            with_write_timeout(Duration::from_millis(75)),
        ]);
        assert_eq!(
            (None, Some(Duration::from_millis(75))),
            options.resolve(Some(Duration::from_secs(1)), Some(Duration::from_secs(2))),
        );

        let options = RequestOptions::from_opts(vec![
            with_read_timeout(Duration::from_millis(125)),
            without_write_timeout(),
        ]);
        assert_eq!(
            (Some(Duration::from_millis(125)), None),
            options.resolve(Some(Duration::from_secs(1)), Some(Duration::from_secs(2))),
        );
    }
}

#[derive(Debug, Default)]
pub struct NoopClient;

#[async_trait]
impl ClientOps for NoopClient {}
