use crate::error::Result;
use crate::machine::{Machine, RateLimiterSet};

pub trait MachineIface {
    fn start(&mut self) -> Result<()>;
    fn stop_vmm(&mut self) -> Result<()>;
    fn shutdown(&mut self) -> Result<()>;
    fn wait(&mut self) -> Result<()>;
    fn set_metadata(&mut self, metadata: &serde_json::Value) -> Result<()>;
    fn update_guest_drive(&mut self, drive_id: &str, path_on_host: &str) -> Result<()>;
    fn update_guest_network_interface_rate_limit(
        &mut self,
        iface_id: &str,
        rate_limiters: RateLimiterSet,
    ) -> Result<()>;
}

impl MachineIface for Machine {
    fn start(&mut self) -> Result<()> {
        Machine::start(self)
    }

    fn stop_vmm(&mut self) -> Result<()> {
        Machine::stop_vmm(self)
    }

    fn shutdown(&mut self) -> Result<()> {
        Machine::shutdown(self)
    }

    fn wait(&mut self) -> Result<()> {
        Machine::wait(self)
    }

    fn set_metadata(&mut self, metadata: &serde_json::Value) -> Result<()> {
        Machine::set_metadata(self, metadata)
    }

    fn update_guest_drive(&mut self, drive_id: &str, path_on_host: &str) -> Result<()> {
        Machine::update_guest_drive(self, drive_id, path_on_host)
    }

    fn update_guest_network_interface_rate_limit(
        &mut self,
        iface_id: &str,
        rate_limiters: RateLimiterSet,
    ) -> Result<()> {
        Machine::update_guest_network_interface_rate_limit(self, iface_id, rate_limiters)
    }
}
