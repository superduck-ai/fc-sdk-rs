#![allow(non_snake_case)]

use std::net::Ipv4Addr;

use firecracker_sdk::cni::internal::{
    filter_by_sandbox, ifaces_with_name, interface_ips, vm_tap_pair,
};
use firecracker_sdk::cni::{CniInterface, CniIpConfig, CniResult};
use ipnet::Ipv4Net;

#[test]
fn TestInterfaceIPs() {
    let veth_name = "veth0";
    let vm_iface_name = "vm0";
    let netns_id = "netns";
    let vm_id = "vmid";

    let result = CniResult {
        interfaces: vec![
            CniInterface {
                name: veth_name.to_string(),
                sandbox: netns_id.to_string(),
                mac: None,
            },
            CniInterface {
                name: vm_iface_name.to_string(),
                sandbox: netns_id.to_string(),
                mac: None,
            },
            CniInterface {
                name: vm_iface_name.to_string(),
                sandbox: vm_id.to_string(),
                mac: None,
            },
        ],
        ips: vec![
            CniIpConfig {
                interface: Some(0),
                address: "10.0.0.2/24".parse::<Ipv4Net>().unwrap(),
                gateway: Ipv4Addr::new(10, 0, 0, 1),
            },
            CniIpConfig {
                interface: Some(2),
                address: "10.0.1.2/24".parse::<Ipv4Net>().unwrap(),
                gateway: Ipv4Addr::new(10, 0, 1, 1),
            },
            CniIpConfig {
                interface: Some(0),
                address: "192.168.0.2/24".parse::<Ipv4Net>().unwrap(),
                gateway: Ipv4Addr::new(192, 168, 0, 1),
            },
        ],
        ..CniResult::default()
    };

    assert!(interface_ips(&result, vm_iface_name, netns_id).is_empty());

    let vm_ips = interface_ips(&result, vm_iface_name, vm_id);
    assert_eq!(vec![result.ips[1].clone()], vm_ips);

    let veth_ips = interface_ips(&result, veth_name, netns_id);
    assert_eq!(vec![result.ips[0].clone(), result.ips[2].clone()], veth_ips);

    assert!(interface_ips(&result, veth_name, vm_id).is_empty());
}

#[test]
fn TestFilterBySandbox() {
    let ifaces = vec![
        CniInterface {
            name: "veth0".to_string(),
            sandbox: "netns".to_string(),
            mac: None,
        },
        CniInterface {
            name: "vm0".to_string(),
            sandbox: "netns".to_string(),
            mac: None,
        },
        CniInterface {
            name: "vm0".to_string(),
            sandbox: "vmid".to_string(),
            mac: None,
        },
    ];

    let (inside_netns, outside_netns) = filter_by_sandbox("netns", &ifaces);
    assert_eq!(ifaces[..2].to_vec(), inside_netns);
    assert_eq!(ifaces[2..].to_vec(), outside_netns);

    let (inside_vm, outside_vm) = filter_by_sandbox("vmid", &ifaces);
    assert_eq!(ifaces[2..].to_vec(), inside_vm);
    assert_eq!(ifaces[..2].to_vec(), outside_vm);
}

#[test]
fn TestIfacesWithName() {
    let ifaces = vec![
        CniInterface {
            name: "veth0".to_string(),
            sandbox: "netns".to_string(),
            mac: None,
        },
        CniInterface {
            name: "vm0".to_string(),
            sandbox: "netns".to_string(),
            mac: None,
        },
        CniInterface {
            name: "vm0".to_string(),
            sandbox: "vmid".to_string(),
            mac: None,
        },
    ];

    assert_eq!(ifaces[..1].to_vec(), ifaces_with_name("veth0", &ifaces));
    assert_eq!(ifaces[1..].to_vec(), ifaces_with_name("vm0", &ifaces));
}

#[test]
fn TestGetVMTapPair() {
    let result = CniResult {
        interfaces: vec![
            CniInterface {
                name: "veth0".to_string(),
                sandbox: "/my/lil/netns".to_string(),
                mac: Some("22:33:44:55:66:77".to_string()),
            },
            CniInterface {
                name: "tap0".to_string(),
                sandbox: "/my/lil/netns".to_string(),
                mac: Some("11:22:33:44:55:66".to_string()),
            },
            CniInterface {
                name: "tap0".to_string(),
                sandbox: "this-is-not-a-machine".to_string(),
                mac: Some("22:33:44:55:66:77".to_string()),
            },
        ],
        ..CniResult::default()
    };

    let (vm_iface, tap_iface) = vm_tap_pair(&result, "this-is-not-a-machine").unwrap();
    assert_eq!("tap0", vm_iface.name);
    assert_eq!("this-is-not-a-machine", vm_iface.sandbox);
    assert_eq!(Some("22:33:44:55:66:77".to_string()), vm_iface.mac);

    assert_eq!("tap0", tap_iface.name);
    assert_eq!("/my/lil/netns", tap_iface.sandbox);
    assert_eq!(Some("11:22:33:44:55:66".to_string()), tap_iface.mac);
}
