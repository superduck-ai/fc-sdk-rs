use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub name: String,
    pub mac_address: Option<String>,
    pub mtu: i32,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("did not find expected link on device {device:?}")]
pub struct LinkNotFoundError {
    pub device: String,
}

pub trait NetlinkOps {
    fn get_link(&self, namespace_path: &str, name: &str) -> Result<Link, LinkNotFoundError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RealNetlinkOps;

impl NetlinkOps for RealNetlinkOps {
    fn get_link(&self, _namespace_path: &str, name: &str) -> Result<Link, LinkNotFoundError> {
        let base_path = std::path::Path::new("/sys/class/net").join(name);
        if !base_path.exists() {
            return Err(LinkNotFoundError {
                device: name.to_string(),
            });
        }

        let mtu = std::fs::read_to_string(base_path.join("mtu"))
            .map_err(|_| LinkNotFoundError {
                device: name.to_string(),
            })?
            .trim()
            .parse::<i32>()
            .map_err(|_| LinkNotFoundError {
                device: name.to_string(),
            })?;

        let mac_address = std::fs::read_to_string(base_path.join("address"))
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        Ok(Link {
            name: name.to_string(),
            mac_address,
            mtu,
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct UnsupportedNetlinkOps;

impl NetlinkOps for UnsupportedNetlinkOps {
    fn get_link(&self, _namespace_path: &str, name: &str) -> Result<Link, LinkNotFoundError> {
        Err(LinkNotFoundError {
            device: name.to_string(),
        })
    }
}
