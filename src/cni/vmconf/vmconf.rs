use thiserror::Error;

use crate::cni::internal::{
    CniUtilError, LinkNotFoundError, NetlinkOps, interface_ips, vm_tap_pair,
};
use crate::cni::{CniIpConfig, CniResult, CniRoute};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaticNetworkConf {
    pub tap_name: String,
    pub netns_path: String,
    pub vm_if_name: Option<String>,
    pub vm_mac_addr: Option<String>,
    pub vm_mtu: i32,
    pub vm_ip_config: Option<CniIpConfig>,
    pub vm_routes: Vec<CniRoute>,
    pub vm_nameservers: Vec<String>,
    pub vm_domain: Option<String>,
    pub vm_search_domains: Vec<String>,
    pub vm_resolver_options: Vec<String>,
}

impl StaticNetworkConf {
    pub fn ip_boot_param(&self) -> String {
        let Some(ip_config) = self.vm_ip_config.as_ref() else {
            return String::new();
        };

        let mask = ip_config.address.netmask().octets();
        let mut nameservers = [String::new(), String::new()];
        for (slot, nameserver) in nameservers.iter_mut().zip(self.vm_nameservers.iter()) {
            *slot = nameserver.clone();
        }

        [
            ip_config.address.addr().to_string(),
            String::new(),
            ip_config.gateway.to_string(),
            format!("{}.{}.{}.{}", mask[0], mask[1], mask[2], mask[3]),
            String::new(),
            self.vm_if_name.clone().unwrap_or_default(),
            "off".to_string(),
            nameservers[0].clone(),
            nameservers[1].clone(),
            String::new(),
        ]
        .join(":")
    }
}

pub fn static_network_conf_from(
    result: &CniResult,
    container_id: &str,
    netlink_ops: &dyn NetlinkOps,
) -> Result<StaticNetworkConf, VmConfError> {
    let (vm_iface, tap_iface) = vm_tap_pair(result, container_id)?;
    let vm_ips = interface_ips(result, &vm_iface.name, &vm_iface.sandbox);
    if vm_ips.len() != 1 {
        return Err(VmConfError::UnexpectedVmIpCount {
            iface_name: vm_iface.name,
            ips: vm_ips,
        });
    }

    let tap_mtu = mtu_of(&tap_iface.name, &tap_iface.sandbox, netlink_ops)?;

    Ok(StaticNetworkConf {
        tap_name: tap_iface.name,
        netns_path: tap_iface.sandbox,
        vm_mac_addr: vm_iface.mac,
        vm_mtu: tap_mtu,
        vm_ip_config: Some(vm_ips[0].clone()),
        vm_routes: result.routes.clone(),
        vm_nameservers: result.dns.nameservers.clone(),
        vm_domain: result.dns.domain.clone(),
        vm_search_domains: result.dns.search.clone(),
        vm_resolver_options: result.dns.options.clone(),
        ..StaticNetworkConf::default()
    })
}

pub fn mtu_of(
    iface_name: &str,
    netns_path: &str,
    netlink_ops: &dyn NetlinkOps,
) -> Result<i32, VmConfError> {
    let link = netlink_ops
        .get_link(netns_path, iface_name)
        .map_err(|error| VmConfError::LinkLookup {
            iface_name: iface_name.to_string(),
            netns_path: netns_path.to_string(),
            source: error,
        })?;

    Ok(link.mtu)
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum VmConfError {
    #[error(transparent)]
    CniUtil(#[from] CniUtilError),
    #[error("expected to find 1 IP for vm interface {iface_name:?}, but instead found {ips:?}")]
    UnexpectedVmIpCount {
        iface_name: String,
        ips: Vec<CniIpConfig>,
    },
    #[error("failed to find device {iface_name:?} in netns {netns_path:?}: {source}")]
    LinkLookup {
        iface_name: String,
        netns_path: String,
        source: LinkNotFoundError,
    },
}
