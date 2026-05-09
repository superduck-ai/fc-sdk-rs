#![allow(non_snake_case)]

use std::fs;
use std::net::Ipv4Addr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use firecracker_sdk::fctesting::MockClient;
use firecracker_sdk::cni::internal::{Link, MockNetlinkOps};
use firecracker_sdk::cni::{CniDnsConfig, CniInterface, CniIpConfig, CniResult, CniRoute};
use firecracker_sdk::{
    CleanupFn, CniConfiguration, CniNetworkOperations, CniRuntimeConf, Config,
    DEFAULT_CNI_BIN_DIR, DEFAULT_CNI_CACHE_DIR, DEFAULT_CNI_CONF_DIR, IPConfiguration, Machine,
    MachineConfiguration, NetworkInterface, NetworkInterfaces, RealCniNetworkOperations,
    RealNetlinkOps, StaticNetworkConfiguration, parse_kernel_args,
};
use ipnet::Ipv4Net;

unsafe extern "C" {
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
}

fn is_root() -> bool {
    unsafe { libc_geteuid() == 0 }
}

#[derive(Default)]
struct MockCniOps {
    init_calls: std::sync::atomic::AtomicUsize,
    invoke_calls: std::sync::atomic::AtomicUsize,
    result: CniResult,
}

impl CniNetworkOperations for MockCniOps {
    fn initialize_netns(&self, _net_ns_path: &str) -> firecracker_sdk::Result<Vec<CleanupFn>> {
        self.init_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(vec![Box::new(|| Ok(()))])
    }

    fn invoke_cni(
        &self,
        _config: &CniConfiguration,
    ) -> firecracker_sdk::Result<(CniResult, Vec<CleanupFn>)> {
        self.invoke_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok((self.result.clone(), vec![Box::new(|| Ok(()))]))
    }
}

fn valid_ip_configuration() -> IPConfiguration {
    IPConfiguration::new(
        "198.51.100.2/24".parse::<Ipv4Net>().unwrap(),
        Ipv4Addr::new(198, 51, 100, 1),
    )
    .with_nameservers(["192.0.2.1", "192.0.2.2"])
}

fn valid_static_network_interface() -> NetworkInterface {
    NetworkInterface {
        static_configuration: Some(
            StaticNetworkConfiguration::new("tap0")
                .with_mac_address("00:11:22:33:44:55")
                .with_ip_configuration(valid_ip_configuration()),
        ),
        ..NetworkInterface::default()
    }
}

fn valid_cni_interface() -> NetworkInterface {
    NetworkInterface {
        cni_configuration: Some(CniConfiguration {
            network_name: Some("phony-network".to_string()),
            net_ns_path: Some("/my/phony/netns".to_string()),
            ..CniConfiguration::default()
        }),
        ..NetworkInterface::default()
    }
}

fn write_fake_cni_plugin(
    bin_dir: &Path,
    log_path: &Path,
    input_path: &Path,
    network_name: &str,
) -> PathBuf {
    let plugin_path = bin_dir.join("fake-cni");
    fs::write(
        &plugin_path,
        format!(
            r#"#!/bin/sh
set -eu

input="$(cat)"
printf '%s' "$input" > "{input_path}"
mkdir -p "${{CNI_CACHE_DIR}}/results"

case "${{CNI_COMMAND}}" in
  ADD)
    printf 'ADD|%s|%s|%s\n' "${{CNI_CONTAINERID}}" "${{CNI_NETNS}}" "${{CNI_IFNAME}}" >> "{log_path}"
    : > "${{CNI_CACHE_DIR}}/results/{network_name}-${{CNI_CONTAINERID}}-${{CNI_IFNAME}}"
    cat <<EOF
{{
  "cniVersion": "0.3.1",
  "interfaces": [
    {{"name": "lo", "sandbox": "${{CNI_NETNS}}"}},
    {{"name": "lo", "sandbox": "${{CNI_CONTAINERID}}", "mac": "aa:bb:cc:dd:ee:ff"}}
  ],
  "ips": [
    {{"interface": 1, "address": "10.168.0.2/16", "gateway": "10.168.0.1"}}
  ],
  "dns": {{
    "nameservers": ["1.1.1.1", "8.8.8.8"]
  }}
}}
EOF
    ;;
  DEL)
    printf 'DEL|%s|%s|%s\n' "${{CNI_CONTAINERID}}" "${{CNI_NETNS}}" "${{CNI_IFNAME}}" >> "{log_path}"
    rm -f "${{CNI_CACHE_DIR}}/results/{network_name}-${{CNI_CONTAINERID}}-${{CNI_IFNAME}}"
    ;;
esac
"#,
            input_path = input_path.display(),
            log_path = log_path.display(),
            network_name = network_name,
        ),
    )
    .unwrap();

    let mut permissions = fs::metadata(&plugin_path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&plugin_path, permissions).unwrap();
    plugin_path
}

fn build_real_cni_interface(
    conf_dir: Option<&Path>,
    cache_dir: &Path,
    bin_dir: &Path,
    net_ns_path: &Path,
    network_name: &str,
    network_config: Option<String>,
) -> NetworkInterfaces {
    NetworkInterfaces::from(vec![NetworkInterface {
        cni_configuration: Some(CniConfiguration {
            network_name: Some(network_name.to_string()),
            network_config,
            if_name: Some("veth0".to_string()),
            vm_if_name: Some("eth0".to_string()),
            bin_path: vec![bin_dir.display().to_string()],
            conf_dir: conf_dir.map(|path| path.display().to_string()),
            cache_dir: Some(cache_dir.display().to_string()),
            net_ns_path: Some(net_ns_path.display().to_string()),
            ..CniConfiguration::default()
        }),
        ..NetworkInterface::default()
    }])
}

fn assert_real_cni_static_configuration(interfaces: &NetworkInterfaces) {
    let static_config = interfaces[0].static_configuration.as_ref().unwrap();
    assert_eq!("lo", static_config.host_dev_name);
    assert_eq!(
        Some("aa:bb:cc:dd:ee:ff".to_string()),
        static_config.mac_address
    );

    let ip_config = static_config.ip_configuration.as_ref().unwrap();
    assert_eq!(
        IPConfiguration::new(
            "10.168.0.2/16".parse::<Ipv4Net>().unwrap(),
            "10.168.0.1".parse().unwrap(),
        )
        .with_if_name("eth0")
        .with_nameservers(["1.1.1.1", "8.8.8.8"]),
        ip_config.clone()
    );
}

fn run_cleanup(cleanup: Vec<firecracker_sdk::CleanupFn>) {
    for cleanup_fn in cleanup.into_iter().rev() {
        cleanup_fn().unwrap();
    }
}

#[test]
fn TestNetworkStaticValidation() {
    assert!(
        valid_static_network_interface()
            .static_configuration
            .as_ref()
            .unwrap()
            .validate()
            .is_ok()
    );
}

#[test]
fn TestNetworkStaticValidationFails_HostDevName() {
    let static_config = StaticNetworkConfiguration {
        mac_address: Some("00:11:22:33:44:55".to_string()),
        host_dev_name: String::new(),
        ip_configuration: Some(valid_ip_configuration()),
    };
    assert!(static_config.validate().is_err());
}

#[test]
fn TestNetworkStaticValidationFails_TooManyNameservers() {
    let static_config = StaticNetworkConfiguration::new("tap0").with_ip_configuration(
        IPConfiguration::new(
            "198.51.100.2/24".parse::<Ipv4Net>().unwrap(),
            Ipv4Addr::new(198, 51, 100, 1),
        )
        .with_nameservers(["192.0.2.1", "192.0.2.2", "192.0.2.3"]),
    );
    assert!(static_config.validate().is_err());
}

#[test]
fn TestNetworkStaticValidationFails_IPConfiguration() {
    assert!("2001:db8:a0b:12f0::2/24".parse::<Ipv4Net>().is_err());
}

#[test]
fn TestNetworkCNIValidation() {
    assert!(
        valid_cni_interface()
            .cni_configuration
            .as_ref()
            .unwrap()
            .validate()
            .is_ok()
    );
}

#[test]
fn TestNetworkCNIValidationFails_NetworkName() {
    assert!(CniConfiguration::default().validate().is_err());
}

#[test]
fn TestNetworkInterfacesValidation_None() {
    assert!(
        NetworkInterfaces::default()
            .validate(&parse_kernel_args("foo=bar this=phony"))
            .is_ok()
    );
}

#[test]
fn TestNetworkInterfacesValidation_Static() {
    assert!(
        NetworkInterfaces::from(vec![valid_static_network_interface()])
            .validate(&parse_kernel_args("foo=bar this=phony"))
            .is_ok()
    );
}

#[test]
fn TestNetworkInterfacesValidation_CNI() {
    assert!(
        NetworkInterfaces::from(vec![valid_cni_interface()])
            .validate(&parse_kernel_args("foo=bar this=phony"))
            .is_ok()
    );
}

#[test]
fn TestNetworkInterfacesValidation_MultipleStatic() {
    let interfaces = NetworkInterfaces::from(vec![
        NetworkInterface {
            static_configuration: Some(
                StaticNetworkConfiguration::new("tap0").with_mac_address("00:11:22:33:44:55"),
            ),
            ..NetworkInterface::default()
        },
        NetworkInterface {
            static_configuration: Some(
                StaticNetworkConfiguration::new("tap1").with_mac_address("11:22:33:44:55:66"),
            ),
            ..NetworkInterface::default()
        },
    ]);
    assert!(
        interfaces
            .validate(&parse_kernel_args("foo=bar this=phony"))
            .is_ok()
    );
}

#[test]
fn TestNetworkInterfacesValidationFails_MultipleCNI() {
    let interfaces = NetworkInterfaces::from(vec![
        valid_cni_interface(),
        NetworkInterface {
            cni_configuration: Some(CniConfiguration {
                network_name: Some("something-else".to_string()),
                net_ns_path: Some("/a/different/netns".to_string()),
                ..CniConfiguration::default()
            }),
            ..NetworkInterface::default()
        },
    ]);
    assert!(
        interfaces
            .validate(&parse_kernel_args("foo=bar this=phony"))
            .is_err()
    );
}

#[test]
fn TestNetworkInterfacesValidationFails_IPWithMultiple() {
    let interfaces = NetworkInterfaces::from(vec![
        valid_static_network_interface(),
        NetworkInterface {
            static_configuration: Some(
                StaticNetworkConfiguration::new("tap1").with_mac_address("11:22:33:44:55:66"),
            ),
            ..NetworkInterface::default()
        },
    ]);
    assert!(
        interfaces
            .validate(&parse_kernel_args("foo=bar this=phony"))
            .is_err()
    );
}

#[test]
fn TestNetworkInterfacesValidationFails_IPWithKernelArg() {
    let interfaces = NetworkInterfaces::from(vec![valid_static_network_interface()]);
    assert!(
        interfaces
            .validate(&parse_kernel_args("foo=bar this=phony ip=whatevz"))
            .is_err()
    );
}

#[test]
fn TestNetworkInterfacesValidationFails_CNIWithMultiple() {
    let interfaces = NetworkInterfaces::from(vec![
        valid_cni_interface(),
        NetworkInterface {
            static_configuration: Some(
                StaticNetworkConfiguration::new("tap1").with_mac_address("11:22:33:44:55:66"),
            ),
            ..NetworkInterface::default()
        },
    ]);
    assert!(
        interfaces
            .validate(&parse_kernel_args("foo=bar this=phony"))
            .is_err()
    );
}

#[test]
fn TestNetworkInterfacesValidationFails_CNIWithKernelArg() {
    let interfaces = NetworkInterfaces::from(vec![valid_cni_interface()]);
    assert!(
        interfaces
            .validate(&parse_kernel_args("foo=bar this=phony ip=whatevz"))
            .is_err()
    );
}

#[test]
fn TestNetworkInterfacesValidationFails_NeitherSpecified() {
    assert!(
        NetworkInterfaces::from(vec![NetworkInterface::default()])
            .validate(&parse_kernel_args("foo=bar this=phony"))
            .is_err()
    );
}

#[test]
fn TestNetworkInterfacesValidationFails_BothSpecified() {
    let interfaces = NetworkInterfaces::from(vec![NetworkInterface {
        static_configuration: Some(
            StaticNetworkConfiguration::new("tap0").with_mac_address("00:11:22:33:44:55"),
        ),
        cni_configuration: valid_cni_interface().cni_configuration,
        ..NetworkInterface::default()
    }]);
    assert!(
        interfaces
            .validate(&parse_kernel_args("foo=bar this=phony"))
            .is_err()
    );
}

#[test]
fn TestNetworkMachineCNIWithConfFile() {
    if !is_root() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    let conf_dir = temp_dir.path().join("conf");
    let cache_dir = temp_dir.path().join("cache");
    let netns_dir = temp_dir.path().join("netns");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&conf_dir).unwrap();

    let log_path = temp_dir.path().join("plugin.log");
    let input_path = temp_dir.path().join("plugin-input.json");
    let network_name = "fcnet";
    write_fake_cni_plugin(&bin_dir, &log_path, &input_path, network_name);

    let conf_path = conf_dir.join("fcnet.conflist");
    fs::write(
        &conf_path,
        r#"{
  "cniVersion": "0.3.1",
  "name": "fcnet",
  "plugins": [
    { "type": "fake-cni" }
  ]
}"#,
    )
    .unwrap();

    let net_ns_path = netns_dir.join("vm-123");
    let mut interfaces = build_real_cni_interface(
        Some(&conf_dir),
        &cache_dir,
        &bin_dir,
        &net_ns_path,
        network_name,
        None,
    );

    let cleanup = interfaces
        .setup_cni(
            "vm-123",
            &net_ns_path.display().to_string(),
            &RealCniNetworkOperations,
            &RealNetlinkOps,
        )
        .unwrap();

    assert_real_cni_static_configuration(&interfaces);
    assert!(
        fs::read_to_string(&input_path)
            .unwrap()
            .contains(r#""type":"fake-cni""#)
    );
    assert_eq!(
        format!(
            "DEL|vm-123|{}|veth0\nADD|vm-123|{}|veth0\n",
            net_ns_path.display(),
            net_ns_path.display()
        ),
        fs::read_to_string(&log_path).unwrap()
    );
    assert!(cache_dir.join("results/fcnet-vm-123-veth0").exists());
    assert!(net_ns_path.exists());

    run_cleanup(cleanup);

    assert_eq!(
        format!(
            "DEL|vm-123|{}|veth0\nADD|vm-123|{}|veth0\nDEL|vm-123|{}|veth0\n",
            net_ns_path.display(),
            net_ns_path.display(),
            net_ns_path.display()
        ),
        fs::read_to_string(&log_path).unwrap()
    );
    assert!(!cache_dir.join("results/fcnet-vm-123-veth0").exists());
    assert!(!net_ns_path.exists());
}

#[test]
fn TestNetworkMachineCNIWithParsedConfig() {
    if !is_root() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    let cache_dir = temp_dir.path().join("cache");
    let netns_dir = temp_dir.path().join("netns");
    fs::create_dir_all(&bin_dir).unwrap();

    let log_path = temp_dir.path().join("plugin.log");
    let input_path = temp_dir.path().join("plugin-input.json");
    let network_name = "fcnet-inline";
    write_fake_cni_plugin(&bin_dir, &log_path, &input_path, network_name);

    let net_ns_path = netns_dir.join("vm-inline");
    let config = Config {
        disable_validation: true,
        vmid: "vm-inline".to_string(),
        net_ns: Some(net_ns_path.display().to_string()),
        machine_cfg: MachineConfiguration::new(1, 128),
        network_interfaces: build_real_cni_interface(
            None,
            &cache_dir,
            &bin_dir,
            &net_ns_path,
            network_name,
            Some(
                r#"{
  "cniVersion": "0.3.1",
  "name": "fcnet-inline",
  "plugins": [
    { "type": "fake-cni" }
  ]
}"#
                .to_string(),
            ),
        ),
        ..Config::default()
    };

    let mut machine = Machine::new_with_client(config, Box::new(MockClient::default())).unwrap();
    machine.setup_network().unwrap();

    assert_real_cni_static_configuration(&machine.cfg.network_interfaces);
    assert_eq!(
        format!(
            "DEL|vm-inline|{}|veth0\nADD|vm-inline|{}|veth0\n",
            net_ns_path.display(),
            net_ns_path.display()
        ),
        fs::read_to_string(&log_path).unwrap()
    );
    assert!(net_ns_path.exists());

    machine.wait().unwrap();

    assert_eq!(
        format!(
            "DEL|vm-inline|{}|veth0\nADD|vm-inline|{}|veth0\nDEL|vm-inline|{}|veth0\n",
            net_ns_path.display(),
            net_ns_path.display(),
            net_ns_path.display()
        ),
        fs::read_to_string(&log_path).unwrap()
    );
    assert!(!cache_dir.join("results/fcnet-inline-vm-inline-veth0").exists());
    assert!(!net_ns_path.exists());
}

#[test]
fn test_ip_boot_param() {
    let ip_configuration = IPConfiguration::new(
        "10.0.0.2/24".parse::<Ipv4Net>().unwrap(),
        Ipv4Addr::new(10, 0, 0, 1),
    )
    .with_if_name("eth0")
    .with_nameservers(["1.1.1.1", "8.8.8.8"]);

    assert_eq!(
        "10.0.0.2::10.0.0.1:255.255.255.0::eth0:off:1.1.1.1:8.8.8.8:",
        ip_configuration.ip_boot_param()
    );
}

#[test]
fn test_cni_configuration_set_defaults() {
    let mut cni = CniConfiguration {
        container_id: Some("vm-123".to_string()),
        ..CniConfiguration::default()
    };

    cni.set_defaults();

    assert_eq!(vec![DEFAULT_CNI_BIN_DIR.to_string()], cni.bin_path);
    assert_eq!(Some(DEFAULT_CNI_CONF_DIR.to_string()), cni.conf_dir);
    assert_eq!(
        Some(format!("{DEFAULT_CNI_CACHE_DIR}/vm-123")),
        cni.cache_dir
    );
}

#[test]
fn test_cni_configuration_as_runtime_conf() {
    let cni = CniConfiguration {
        if_name: Some("veth0".to_string()),
        args: vec![("K".to_string(), "V".to_string())],
        container_id: Some("vm-123".to_string()),
        net_ns_path: Some("/var/run/netns/vm-123".to_string()),
        ..CniConfiguration::default()
    };

    assert_eq!(
        CniRuntimeConf {
            container_id: Some("vm-123".to_string()),
            net_ns: Some("/var/run/netns/vm-123".to_string()),
            if_name: Some("veth0".to_string()),
            args: vec![("K".to_string(), "V".to_string())],
        },
        cni.as_cni_runtime_conf()
    );
}

#[test]
fn test_network_interface_apply_cni_result_updates_static_configuration() {
    let mut iface = NetworkInterface {
        cni_configuration: Some(CniConfiguration {
            container_id: Some("vm-123".to_string()),
            vm_if_name: Some("eth0".to_string()),
            ..CniConfiguration::default()
        }),
        ..NetworkInterface::default()
    };

    let result = CniResult {
        interfaces: vec![
            CniInterface {
                name: "tap0".to_string(),
                sandbox: "/var/run/netns/vm-123".to_string(),
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
            address: "10.168.0.2/16".parse::<Ipv4Net>().unwrap(),
            gateway: Ipv4Addr::new(10, 168, 0, 1),
        }],
        routes: vec![CniRoute {
            dst: "10.168.0.0/16".parse::<Ipv4Net>().unwrap(),
            gw: Ipv4Addr::new(10, 168, 0, 1),
        }],
        dns: CniDnsConfig {
            nameservers: vec![
                "1.1.1.1".to_string(),
                "8.8.8.8".to_string(),
                "9.9.9.9".to_string(),
            ],
            ..CniDnsConfig::default()
        },
    };

    let netlink = MockNetlinkOps {
        links: vec![Link {
            name: "tap0".to_string(),
            mac_address: Some("11:22:33:44:55:66".to_string()),
            mtu: 1500,
        }],
        ..MockNetlinkOps::default()
    };

    iface.apply_cni_result(&result, &netlink).unwrap();

    assert_eq!(
        Some(
            StaticNetworkConfiguration::new("tap0")
                .with_mac_address("aa:bb:cc:dd:ee:ff")
                .with_ip_configuration(
                    IPConfiguration::new(
                        "10.168.0.2/16".parse::<Ipv4Net>().unwrap(),
                        Ipv4Addr::new(10, 168, 0, 1),
                    )
                    .with_if_name("eth0")
                    .with_nameservers(["1.1.1.1", "8.8.8.8"]),
                )
        ),
        iface.static_configuration
    );
}

#[test]
fn test_network_interface_apply_cni_result_preserves_existing_static_configuration() {
    let existing =
        StaticNetworkConfiguration::new("tap-existing").with_mac_address("00:11:22:33:44:55");
    let mut iface = NetworkInterface {
        static_configuration: Some(existing.clone()),
        cni_configuration: Some(CniConfiguration {
            container_id: Some("vm-123".to_string()),
            ..CniConfiguration::default()
        }),
        ..NetworkInterface::default()
    };

    let result = CniResult::default();
    let netlink = MockNetlinkOps::default();

    iface.apply_cni_result(&result, &netlink).unwrap();
    assert_eq!(Some(existing), iface.static_configuration);
}

#[test]
fn test_network_interfaces_setup_cni() {
    let mut interfaces = NetworkInterfaces::from(vec![NetworkInterface {
        cni_configuration: Some(CniConfiguration {
            network_name: Some("fcnet".to_string()),
            vm_if_name: Some("eth0".to_string()),
            ..CniConfiguration::default()
        }),
        ..NetworkInterface::default()
    }]);

    let cni_ops = MockCniOps {
        result: CniResult {
            interfaces: vec![
                CniInterface {
                    name: "tap0".to_string(),
                    sandbox: "/var/run/netns/vm-123".to_string(),
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
                address: "10.168.0.2/16".parse::<Ipv4Net>().unwrap(),
                gateway: Ipv4Addr::new(10, 168, 0, 1),
            }],
            dns: CniDnsConfig {
                nameservers: vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()],
                ..CniDnsConfig::default()
            },
            ..CniResult::default()
        },
        ..MockCniOps::default()
    };
    let netlink = MockNetlinkOps {
        links: vec![Link {
            name: "tap0".to_string(),
            mac_address: Some("11:22:33:44:55:66".to_string()),
            mtu: 1500,
        }],
        ..MockNetlinkOps::default()
    };

    let cleanup = interfaces
        .setup_cni("vm-123", "/var/run/netns/vm-123", &cni_ops, &netlink)
        .unwrap();

    assert_eq!(2, cleanup.len());
    assert_eq!(
        1,
        cni_ops.init_calls.load(std::sync::atomic::Ordering::SeqCst)
    );
    assert_eq!(
        1,
        cni_ops
            .invoke_calls
            .load(std::sync::atomic::Ordering::SeqCst)
    );

    let iface = interfaces.cni_interface().unwrap();
    let cni_config = iface.cni_configuration.as_ref().unwrap();
    assert_eq!(Some("vm-123".to_string()), cni_config.container_id);
    assert_eq!(
        Some("/var/run/netns/vm-123".to_string()),
        cni_config.net_ns_path
    );
    assert_eq!(vec![DEFAULT_CNI_BIN_DIR.to_string()], cni_config.bin_path);
    assert_eq!(Some(DEFAULT_CNI_CONF_DIR.to_string()), cni_config.conf_dir);
    assert_eq!(
        Some(format!("{DEFAULT_CNI_CACHE_DIR}/vm-123")),
        cni_config.cache_dir
    );

    assert_eq!(
        Some(
            StaticNetworkConfiguration::new("tap0")
                .with_mac_address("aa:bb:cc:dd:ee:ff")
                .with_ip_configuration(
                    IPConfiguration::new(
                        "10.168.0.2/16".parse::<Ipv4Net>().unwrap(),
                        Ipv4Addr::new(10, 168, 0, 1),
                    )
                    .with_if_name("eth0")
                    .with_nameservers(["1.1.1.1", "8.8.8.8"]),
                )
        ),
        iface.static_configuration.clone()
    );
}

#[test]
fn test_network_interfaces_setup_cni_without_cni_interface_returns_empty_cleanups() {
    let mut interfaces = NetworkInterfaces::from(vec![valid_static_network_interface()]);
    let cleanup = interfaces
        .setup_cni(
            "vm-123",
            "/var/run/netns/vm-123",
            &MockCniOps::default(),
            &MockNetlinkOps::default(),
        )
        .unwrap();
    assert!(cleanup.is_empty());
}
