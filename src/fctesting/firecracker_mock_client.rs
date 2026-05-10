use async_trait::async_trait;

use crate::client::ClientOps;
use crate::error::Result;
use crate::models::{
    Balloon, BalloonStats, BalloonStatsUpdate, BalloonUpdate, BootSource, CpuConfig, Drive,
    EntropyDevice, FirecrackerVersion, FullVmConfiguration, InstanceActionInfo, InstanceInfo,
    Logger, MachineConfiguration, Metrics, MmdsConfig, NetworkInterfaceModel, PartialDrive,
    PartialNetworkInterface, SnapshotCreateParams, SnapshotLoadParams, Vm, VsockModel,
};

type FirecrackerVersionFn = Box<dyn FnMut() -> Result<FirecrackerVersion> + Send>;
type LoggerFn = Box<dyn FnMut(&Logger) -> Result<()> + Send>;
type MetricsFn = Box<dyn FnMut(&Metrics) -> Result<()> + Send>;
type MachineCfgFn = Box<dyn FnMut(&MachineConfiguration) -> Result<()> + Send>;
type GetMachineCfgFn = Box<dyn FnMut() -> Result<MachineConfiguration> + Send>;
type BootSourceFn = Box<dyn FnMut(&BootSource) -> Result<()> + Send>;
type CpuConfigFn = Box<dyn FnMut(&CpuConfig) -> Result<()> + Send>;
type EntropyDeviceFn = Box<dyn FnMut(&EntropyDevice) -> Result<()> + Send>;
type DriveFn = Box<dyn FnMut(&str, &Drive) -> Result<()> + Send>;
type PartialDriveFn = Box<dyn FnMut(&str, &PartialDrive) -> Result<()> + Send>;
type NetIfFn = Box<dyn FnMut(&str, &NetworkInterfaceModel) -> Result<()> + Send>;
type PartialNetIfFn = Box<dyn FnMut(&str, &PartialNetworkInterface) -> Result<()> + Send>;
type VsockFn = Box<dyn FnMut(&VsockModel) -> Result<()> + Send>;
type VmFn = Box<dyn FnMut(&Vm) -> Result<()> + Send>;
type SnapshotCreateFn = Box<dyn FnMut(&SnapshotCreateParams) -> Result<()> + Send>;
type SnapshotLoadFn = Box<dyn FnMut(&SnapshotLoadParams) -> Result<()> + Send>;
type InstanceActionFn = Box<dyn FnMut(&InstanceActionInfo) -> Result<()> + Send>;
type MetadataFn = Box<dyn FnMut(&serde_json::Value) -> Result<()> + Send>;
type GetMetadataFn = Box<dyn FnMut() -> Result<serde_json::Value> + Send>;
type MmdsConfigFn = Box<dyn FnMut(&MmdsConfig) -> Result<()> + Send>;
type InstanceInfoFn = Box<dyn FnMut() -> Result<InstanceInfo> + Send>;
type BalloonFn = Box<dyn FnMut(&Balloon) -> Result<()> + Send>;
type GetBalloonFn = Box<dyn FnMut() -> Result<Balloon> + Send>;
type BalloonUpdateFn = Box<dyn FnMut(&BalloonUpdate) -> Result<()> + Send>;
type GetBalloonStatsFn = Box<dyn FnMut() -> Result<BalloonStats> + Send>;
type BalloonStatsUpdateFn = Box<dyn FnMut(&BalloonStatsUpdate) -> Result<()> + Send>;
type ExportVmConfigFn = Box<dyn FnMut() -> Result<FullVmConfiguration> + Send>;

#[derive(Default)]
pub struct MockClient {
    pub get_firecracker_version_fn: Option<FirecrackerVersionFn>,
    pub put_logger_fn: Option<LoggerFn>,
    pub put_metrics_fn: Option<MetricsFn>,
    pub put_machine_configuration_fn: Option<MachineCfgFn>,
    pub patch_machine_configuration_fn: Option<MachineCfgFn>,
    pub get_machine_configuration_fn: Option<GetMachineCfgFn>,
    pub put_guest_boot_source_fn: Option<BootSourceFn>,
    pub put_cpu_configuration_fn: Option<CpuConfigFn>,
    pub put_entropy_device_fn: Option<EntropyDeviceFn>,
    pub put_guest_drive_by_id_fn: Option<DriveFn>,
    pub patch_guest_drive_by_id_fn: Option<PartialDriveFn>,
    pub put_guest_network_interface_by_id_fn: Option<NetIfFn>,
    pub patch_guest_network_interface_by_id_fn: Option<PartialNetIfFn>,
    pub put_guest_vsock_fn: Option<VsockFn>,
    pub patch_vm_fn: Option<VmFn>,
    pub create_snapshot_fn: Option<SnapshotCreateFn>,
    pub load_snapshot_fn: Option<SnapshotLoadFn>,
    pub create_sync_action_fn: Option<InstanceActionFn>,
    pub put_mmds_fn: Option<MetadataFn>,
    pub get_mmds_fn: Option<GetMetadataFn>,
    pub patch_mmds_fn: Option<MetadataFn>,
    pub put_mmds_config_fn: Option<MmdsConfigFn>,
    pub describe_instance_fn: Option<InstanceInfoFn>,
    pub put_balloon_fn: Option<BalloonFn>,
    pub get_balloon_config_fn: Option<GetBalloonFn>,
    pub patch_balloon_fn: Option<BalloonUpdateFn>,
    pub get_balloon_stats_fn: Option<GetBalloonStatsFn>,
    pub patch_balloon_stats_interval_fn: Option<BalloonStatsUpdateFn>,
    pub get_export_vm_config_fn: Option<ExportVmConfigFn>,
}

#[async_trait]
impl ClientOps for MockClient {
    async fn get_firecracker_version(&mut self) -> Result<FirecrackerVersion> {
        if let Some(f) = self.get_firecracker_version_fn.as_mut() {
            return f();
        }
        Ok(FirecrackerVersion {
            firecracker_version: String::new(),
        })
    }

    async fn put_logger(&mut self, logger: &Logger) -> Result<()> {
        if let Some(f) = self.put_logger_fn.as_mut() {
            return f(logger);
        }
        Ok(())
    }

    async fn put_metrics(&mut self, metrics: &Metrics) -> Result<()> {
        if let Some(f) = self.put_metrics_fn.as_mut() {
            return f(metrics);
        }
        Ok(())
    }

    async fn put_machine_configuration(&mut self, config: &MachineConfiguration) -> Result<()> {
        if let Some(f) = self.put_machine_configuration_fn.as_mut() {
            return f(config);
        }
        Ok(())
    }

    async fn patch_machine_configuration(&mut self, config: &MachineConfiguration) -> Result<()> {
        if let Some(f) = self.patch_machine_configuration_fn.as_mut() {
            return f(config);
        }
        Ok(())
    }

    async fn get_machine_configuration(&mut self) -> Result<MachineConfiguration> {
        if let Some(f) = self.get_machine_configuration_fn.as_mut() {
            return f();
        }
        Ok(MachineConfiguration::default())
    }

    async fn put_guest_boot_source(&mut self, boot_source: &BootSource) -> Result<()> {
        if let Some(f) = self.put_guest_boot_source_fn.as_mut() {
            return f(boot_source);
        }
        Ok(())
    }

    async fn put_cpu_configuration(&mut self, config: &CpuConfig) -> Result<()> {
        if let Some(f) = self.put_cpu_configuration_fn.as_mut() {
            return f(config);
        }
        Ok(())
    }

    async fn put_entropy_device(&mut self, device: &EntropyDevice) -> Result<()> {
        if let Some(f) = self.put_entropy_device_fn.as_mut() {
            return f(device);
        }
        Ok(())
    }

    async fn put_guest_drive_by_id(&mut self, drive_id: &str, drive: &Drive) -> Result<()> {
        if let Some(f) = self.put_guest_drive_by_id_fn.as_mut() {
            return f(drive_id, drive);
        }
        Ok(())
    }

    async fn patch_guest_drive_by_id(
        &mut self,
        drive_id: &str,
        drive: &PartialDrive,
    ) -> Result<()> {
        if let Some(f) = self.patch_guest_drive_by_id_fn.as_mut() {
            return f(drive_id, drive);
        }
        Ok(())
    }

    async fn put_guest_network_interface_by_id(
        &mut self,
        iface_id: &str,
        iface: &NetworkInterfaceModel,
    ) -> Result<()> {
        if let Some(f) = self.put_guest_network_interface_by_id_fn.as_mut() {
            return f(iface_id, iface);
        }
        Ok(())
    }

    async fn patch_guest_network_interface_by_id(
        &mut self,
        iface_id: &str,
        iface: &PartialNetworkInterface,
    ) -> Result<()> {
        if let Some(f) = self.patch_guest_network_interface_by_id_fn.as_mut() {
            return f(iface_id, iface);
        }
        Ok(())
    }

    async fn put_guest_vsock(&mut self, vsock: &VsockModel) -> Result<()> {
        if let Some(f) = self.put_guest_vsock_fn.as_mut() {
            return f(vsock);
        }
        Ok(())
    }

    async fn patch_vm(&mut self, vm: &Vm) -> Result<()> {
        if let Some(f) = self.patch_vm_fn.as_mut() {
            return f(vm);
        }
        Ok(())
    }

    async fn create_snapshot(&mut self, snapshot: &SnapshotCreateParams) -> Result<()> {
        if let Some(f) = self.create_snapshot_fn.as_mut() {
            return f(snapshot);
        }
        Ok(())
    }

    async fn load_snapshot(&mut self, snapshot: &SnapshotLoadParams) -> Result<()> {
        if let Some(f) = self.load_snapshot_fn.as_mut() {
            return f(snapshot);
        }
        Ok(())
    }

    async fn create_sync_action(&mut self, action: &InstanceActionInfo) -> Result<()> {
        if let Some(f) = self.create_sync_action_fn.as_mut() {
            return f(action);
        }
        Ok(())
    }

    async fn put_mmds(&mut self, metadata: &serde_json::Value) -> Result<()> {
        if let Some(f) = self.put_mmds_fn.as_mut() {
            return f(metadata);
        }
        Ok(())
    }

    async fn get_mmds(&mut self) -> Result<serde_json::Value> {
        if let Some(f) = self.get_mmds_fn.as_mut() {
            return f();
        }
        Ok(serde_json::json!({}))
    }

    async fn patch_mmds(&mut self, metadata: &serde_json::Value) -> Result<()> {
        if let Some(f) = self.patch_mmds_fn.as_mut() {
            return f(metadata);
        }
        Ok(())
    }

    async fn put_mmds_config(&mut self, config: &MmdsConfig) -> Result<()> {
        if let Some(f) = self.put_mmds_config_fn.as_mut() {
            return f(config);
        }
        Ok(())
    }

    async fn describe_instance(&mut self) -> Result<InstanceInfo> {
        if let Some(f) = self.describe_instance_fn.as_mut() {
            return f();
        }
        Ok(InstanceInfo::default())
    }

    async fn put_balloon(&mut self, balloon: &Balloon) -> Result<()> {
        if let Some(f) = self.put_balloon_fn.as_mut() {
            return f(balloon);
        }
        Ok(())
    }

    async fn get_balloon_config(&mut self) -> Result<Balloon> {
        if let Some(f) = self.get_balloon_config_fn.as_mut() {
            return f();
        }
        Ok(Balloon::default())
    }

    async fn patch_balloon(&mut self, balloon_update: &BalloonUpdate) -> Result<()> {
        if let Some(f) = self.patch_balloon_fn.as_mut() {
            return f(balloon_update);
        }
        Ok(())
    }

    async fn get_balloon_stats(&mut self) -> Result<BalloonStats> {
        if let Some(f) = self.get_balloon_stats_fn.as_mut() {
            return f();
        }
        Ok(BalloonStats::default())
    }

    async fn patch_balloon_stats_interval(
        &mut self,
        balloon_stats_update: &BalloonStatsUpdate,
    ) -> Result<()> {
        if let Some(f) = self.patch_balloon_stats_interval_fn.as_mut() {
            return f(balloon_stats_update);
        }
        Ok(())
    }

    async fn get_export_vm_config(&mut self) -> Result<FullVmConfiguration> {
        if let Some(f) = self.get_export_vm_config_fn.as_mut() {
            return f();
        }
        Ok(FullVmConfiguration::default())
    }
}
