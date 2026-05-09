use thiserror::Error;

use crate::cni::internal::LinkNotFoundError;
use crate::cni::{CniInterface, CniIpConfig, CniResult};

pub fn interface_ips(result: &CniResult, iface_name: &str, sandbox: &str) -> Vec<CniIpConfig> {
    result
        .ips
        .iter()
        .filter_map(|ip_config| {
            let index = ip_config.interface?;
            let iface = result.interfaces.get(index)?;
            (iface.name == iface_name && iface.sandbox == sandbox).then(|| ip_config.clone())
        })
        .collect()
}

pub fn filter_by_sandbox(
    sandbox: &str,
    ifaces: &[CniInterface],
) -> (Vec<CniInterface>, Vec<CniInterface>) {
    let mut inside = Vec::new();
    let mut outside = Vec::new();

    for iface in ifaces {
        if iface.sandbox == sandbox {
            inside.push(iface.clone());
        } else {
            outside.push(iface.clone());
        }
    }

    (inside, outside)
}

pub fn ifaces_with_name(name: &str, ifaces: &[CniInterface]) -> Vec<CniInterface> {
    ifaces
        .iter()
        .filter(|iface| iface.name == name)
        .cloned()
        .collect()
}

pub fn vm_tap_pair(
    result: &CniResult,
    vm_id: &str,
) -> Result<(CniInterface, CniInterface), CniUtilError> {
    let (vm_ifaces, other_ifaces) = filter_by_sandbox(vm_id, &result.interfaces);
    if vm_ifaces.len() > 1 {
        return Err(CniUtilError::MultipleInterfacesInSandbox {
            sandbox: vm_id.to_string(),
            count: vm_ifaces.len(),
        });
    }
    if vm_ifaces.is_empty() {
        return Err(CniUtilError::LinkNotFound(LinkNotFoundError {
            device: format!("pseudo-device for {vm_id}"),
        }));
    }

    let vm_iface = vm_ifaces[0].clone();
    let tap_ifaces = ifaces_with_name(&vm_iface.name, &other_ifaces);
    if tap_ifaces.len() > 1 {
        return Err(CniUtilError::MultipleInterfacesWithName {
            name: vm_iface.name.clone(),
            count: tap_ifaces.len(),
        });
    }
    if tap_ifaces.is_empty() {
        return Err(CniUtilError::LinkNotFound(LinkNotFoundError {
            device: vm_iface.name.clone(),
        }));
    }

    Ok((vm_iface, tap_ifaces[0].clone()))
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CniUtilError {
    #[error(
        "expected to find at most 1 interface in sandbox {sandbox:?}, but instead found {count}"
    )]
    MultipleInterfacesInSandbox { sandbox: String, count: usize },
    #[error("expected to find at most 1 interface with name {name:?}, but instead found {count}")]
    MultipleInterfacesWithName { name: String, count: usize },
    #[error(transparent)]
    LinkNotFound(#[from] LinkNotFoundError),
}
