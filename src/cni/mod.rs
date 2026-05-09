use serde::{Deserialize, Serialize};

pub mod internal;
pub mod vmconf;

use ipnet::Ipv4Net;
use std::net::Ipv4Addr;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CniResult {
    pub interfaces: Vec<CniInterface>,
    pub ips: Vec<CniIpConfig>,
    pub routes: Vec<CniRoute>,
    pub dns: CniDnsConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CniInterface {
    pub name: String,
    pub sandbox: String,
    pub mac: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CniIpConfig {
    pub interface: Option<usize>,
    pub address: Ipv4Net,
    pub gateway: Ipv4Addr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CniRoute {
    pub dst: Ipv4Net,
    pub gw: Ipv4Addr,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CniDnsConfig {
    pub nameservers: Vec<String>,
    pub domain: Option<String>,
    pub search: Vec<String>,
    pub options: Vec<String>,
}
