use async_trait::async_trait;

use crate::client::RequestOptions;
use crate::error::Result;
use crate::machine::{Machine, RateLimiterSet};

#[async_trait]
pub trait MachineIface {
    async fn start(&mut self) -> Result<()>;
    async fn stop_vmm(&mut self) -> Result<()>;
    async fn shutdown(&mut self) -> Result<()>;
    async fn wait(&mut self) -> Result<()>;
    async fn set_metadata(&mut self, metadata: &serde_json::Value) -> Result<()>;
    async fn update_guest_drive(&mut self, drive_id: &str, path_on_host: &str) -> Result<()>;
    async fn update_guest_drive_with_options(
        &mut self,
        drive_id: &str,
        path_on_host: &str,
        options: RequestOptions,
    ) -> Result<()> {
        let _ = options;
        self.update_guest_drive(drive_id, path_on_host).await
    }
    async fn update_guest_network_interface_rate_limit(
        &mut self,
        iface_id: &str,
        rate_limiters: RateLimiterSet,
    ) -> Result<()>;
    async fn update_guest_network_interface_rate_limit_with_options(
        &mut self,
        iface_id: &str,
        rate_limiters: RateLimiterSet,
        options: RequestOptions,
    ) -> Result<()> {
        let _ = options;
        self.update_guest_network_interface_rate_limit(iface_id, rate_limiters)
            .await
    }
    async fn pause_vm_with_options(&mut self, options: RequestOptions) -> Result<()>;
    async fn resume_vm_with_options(&mut self, options: RequestOptions) -> Result<()>;
    async fn create_snapshot_with_options(
        &mut self,
        mem_file_path: &str,
        snapshot_path: &str,
        options: RequestOptions,
    ) -> Result<()>;
    async fn create_balloon_with_options(
        &mut self,
        amount_mib: i64,
        deflate_on_oom: bool,
        stats_polling_intervals: i64,
        options: RequestOptions,
    ) -> Result<()>;
    async fn update_balloon_with_options(
        &mut self,
        amount_mib: i64,
        options: RequestOptions,
    ) -> Result<()>;
    async fn update_balloon_stats_with_options(
        &mut self,
        stats_polling_intervals: i64,
        options: RequestOptions,
    ) -> Result<()>;
}

#[async_trait]
impl MachineIface for Machine {
    async fn start(&mut self) -> Result<()> {
        Machine::start(self).await
    }

    async fn stop_vmm(&mut self) -> Result<()> {
        Machine::stop_vmm(self).await
    }

    async fn shutdown(&mut self) -> Result<()> {
        Machine::shutdown(self).await
    }

    async fn wait(&mut self) -> Result<()> {
        Machine::wait(self).await
    }

    async fn set_metadata(&mut self, metadata: &serde_json::Value) -> Result<()> {
        Machine::set_metadata(self, metadata).await
    }

    async fn update_guest_drive(&mut self, drive_id: &str, path_on_host: &str) -> Result<()> {
        Machine::update_guest_drive(self, drive_id, path_on_host).await
    }

    async fn update_guest_drive_with_options(
        &mut self,
        drive_id: &str,
        path_on_host: &str,
        options: RequestOptions,
    ) -> Result<()> {
        Machine::update_guest_drive_with_options(self, drive_id, path_on_host, options).await
    }

    async fn update_guest_network_interface_rate_limit(
        &mut self,
        iface_id: &str,
        rate_limiters: RateLimiterSet,
    ) -> Result<()> {
        Machine::update_guest_network_interface_rate_limit(self, iface_id, rate_limiters).await
    }

    async fn update_guest_network_interface_rate_limit_with_options(
        &mut self,
        iface_id: &str,
        rate_limiters: RateLimiterSet,
        options: RequestOptions,
    ) -> Result<()> {
        Machine::update_guest_network_interface_rate_limit_with_options(
            self,
            iface_id,
            rate_limiters,
            options,
        )
        .await
    }

    async fn pause_vm_with_options(&mut self, options: RequestOptions) -> Result<()> {
        Machine::pause_vm_with_options(self, options).await
    }

    async fn resume_vm_with_options(&mut self, options: RequestOptions) -> Result<()> {
        Machine::resume_vm_with_options(self, options).await
    }

    async fn create_snapshot_with_options(
        &mut self,
        mem_file_path: &str,
        snapshot_path: &str,
        options: RequestOptions,
    ) -> Result<()> {
        Machine::create_snapshot_with_options(self, mem_file_path, snapshot_path, options).await
    }

    async fn create_balloon_with_options(
        &mut self,
        amount_mib: i64,
        deflate_on_oom: bool,
        stats_polling_intervals: i64,
        options: RequestOptions,
    ) -> Result<()> {
        Machine::create_balloon_with_options(
            self,
            amount_mib,
            deflate_on_oom,
            stats_polling_intervals,
            options,
        )
        .await
    }

    async fn update_balloon_with_options(
        &mut self,
        amount_mib: i64,
        options: RequestOptions,
    ) -> Result<()> {
        Machine::update_balloon_with_options(self, amount_mib, options).await
    }

    async fn update_balloon_stats_with_options(
        &mut self,
        stats_polling_intervals: i64,
        options: RequestOptions,
    ) -> Result<()> {
        Machine::update_balloon_stats_with_options(self, stats_polling_intervals, options).await
    }
}
