#![allow(non_snake_case)]

use std::net::Ipv4Addr;

use firecracker_sdk::cni::internal::{Link, MockNetlinkOps};
use firecracker_sdk::cni::vmconf::{StaticNetworkConf, mtu_of, static_network_conf_from};
use firecracker_sdk::cni::{CniDnsConfig, CniInterface, CniIpConfig, CniResult, CniRoute};
use ipnet::Ipv4Net;

#[test]
fn TestMTUOf() {
    let nl_ops = MockNetlinkOps {
        links: vec![
            Link {
                name: "tap0".to_string(),
                mac_address: Some("11:22:33:44:55:66".to_string()),
                mtu: 1338,
            },
            Link {
                name: "veth0".to_string(),
                mac_address: Some("22:33:44:55:66:77".to_string()),
                mtu: 1337,
            },
        ],
        ..MockNetlinkOps::default()
    };

    let actual_mtu = mtu_of("tap0", "/my/lil/netns", &nl_ops).unwrap();
    assert_eq!(1338, actual_mtu);
}

#[test]
fn TestIPBootParams() {
    let static_network_conf = StaticNetworkConf {
        tap_name: "taptaptap".to_string(),
        netns_path: "/my/lil/netns".to_string(),
        vm_mac_addr: Some("00:11:22:33:44:55".to_string()),
        vm_if_name: Some("eth0".to_string()),
        vm_mtu: 1337,
        vm_ip_config: Some(CniIpConfig {
            interface: None,
            address: "10.0.0.2/24".parse::<Ipv4Net>().unwrap(),
            gateway: Ipv4Addr::new(10, 0, 0, 1),
        }),
        vm_routes: vec![CniRoute {
            dst: "192.168.0.2/16".parse::<Ipv4Net>().unwrap(),
            gw: Ipv4Addr::new(192, 168, 0, 1),
        }],
        vm_nameservers: vec![
            "1.1.1.1".to_string(),
            "8.8.8.8".to_string(),
            "1.0.0.1".to_string(),
        ],
        vm_domain: Some("example.com".to_string()),
        vm_search_domains: vec!["look".to_string(), "here".to_string()],
        vm_resolver_options: vec![
            "choice".to_string(),
            "is".to_string(),
            "an".to_string(),
            "illusion".to_string(),
        ],
    };

    assert_eq!(
        "10.0.0.2::10.0.0.1:255.255.255.0::eth0:off:1.1.1.1:8.8.8.8:",
        static_network_conf.ip_boot_param()
    );
}

#[test]
fn test_static_network_conf_from() {
    let result = CniResult {
        interfaces: vec![
            CniInterface {
                name: "tap0".to_string(),
                sandbox: "/var/run/netns/test".to_string(),
                mac: Some("11:22:33:44:55:66".to_string()),
            },
            CniInterface {
                name: "tap0".to_string(),
                sandbox: "vm-123".to_string(),
                mac: Some("aa:bb:cc:dd:ee:ff".to_string()),
            },
        ],
        ips: vec![CniIpConfig {
            interface: Some(1),
            address: "10.0.0.2/24".parse::<Ipv4Net>().unwrap(),
            gateway: Ipv4Addr::new(10, 0, 0, 1),
        }],
        routes: vec![CniRoute {
            dst: "10.0.0.0/24".parse::<Ipv4Net>().unwrap(),
            gw: Ipv4Addr::new(10, 0, 0, 1),
        }],
        dns: CniDnsConfig {
            nameservers: vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()],
            domain: Some("example.com".to_string()),
            search: vec!["svc.cluster.local".to_string()],
            options: vec!["ndots:5".to_string()],
        },
    };

    let nl_ops = MockNetlinkOps {
        links: vec![Link {
            name: "tap0".to_string(),
            mac_address: Some("11:22:33:44:55:66".to_string()),
            mtu: 1500,
        }],
        ..MockNetlinkOps::default()
    };

    let conf = static_network_conf_from(&result, "vm-123", &nl_ops).unwrap();
    assert_eq!("tap0", conf.tap_name);
    assert_eq!("/var/run/netns/test", conf.netns_path);
    assert_eq!(Some("aa:bb:cc:dd:ee:ff".to_string()), conf.vm_mac_addr);
    assert_eq!(1500, conf.vm_mtu);
    assert_eq!(
        Some(CniIpConfig {
            interface: Some(1),
            address: "10.0.0.2/24".parse::<Ipv4Net>().unwrap(),
            gateway: Ipv4Addr::new(10, 0, 0, 1),
        }),
        conf.vm_ip_config
    );
    assert_eq!(result.routes, conf.vm_routes);
    assert_eq!(result.dns.nameservers, conf.vm_nameservers);
    assert_eq!(result.dns.domain, conf.vm_domain);
    assert_eq!(result.dns.search, conf.vm_search_domains);
    assert_eq!(result.dns.options, conf.vm_resolver_options);
}
