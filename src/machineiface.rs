use async_trait::async_trait;

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
    async fn update_guest_network_interface_rate_limit(
        &mut self,
        iface_id: &str,
        rate_limiters: RateLimiterSet,
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

    async fn update_guest_network_interface_rate_limit(
        &mut self,
        iface_id: &str,
        rate_limiters: RateLimiterSet,
    ) -> Result<()> {
        Machine::update_guest_network_interface_rate_limit(self, iface_id, rate_limiters).await
    }
}
