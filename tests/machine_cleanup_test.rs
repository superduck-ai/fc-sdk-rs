use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};

use firecracker_sdk::cni::internal::{Link, MockNetlinkOps};
use firecracker_sdk::cni::{CniDnsConfig, CniInterface, CniIpConfig, CniResult};
use firecracker_sdk::fctesting::MockClient;
use firecracker_sdk::{
    AsyncResultExt, CleanupFn, CniConfiguration, CniNetworkOperations, CommandStdio, Config, Error,
    Handler, HandlerList, MachineConfiguration, NetworkInterface, NetworkInterfaces,
    RealCniNetworkOperations, VMCommandBuilder, with_client, with_cni_network_ops,
    with_netlink_ops, with_process_runner,
};
use ipnet::Ipv4Net;

struct RecordingCniOps {
    cleanup_order: Arc<Mutex<Vec<String>>>,
}

impl RecordingCniOps {
    fn new(cleanup_order: Arc<Mutex<Vec<String>>>) -> Self {
        Self { cleanup_order }
    }
}

impl CniNetworkOperations for RecordingCniOps {
    fn initialize_netns(&self, _net_ns_path: &str) -> firecracker_sdk::Result<Vec<CleanupFn>> {
        let cleanup_order = Arc::clone(&self.cleanup_order);
        Ok(vec![Box::new(move || {
            cleanup_order
                .lock()
                .unwrap()
                .push("initialize_netns".to_string());
            Ok(())
        })])
    }

    fn invoke_cni(
        &self,
        config: &CniConfiguration,
    ) -> firecracker_sdk::Result<(CniResult, Vec<CleanupFn>)> {
        let vm_id = config.container_id.clone().unwrap();
        let net_ns_path = config.net_ns_path.clone().unwrap();
        let cleanup_order = Arc::clone(&self.cleanup_order);

        Ok((
            CniResult {
                interfaces: vec![
                    CniInterface {
                        name: "tap0".to_string(),
                        sandbox: net_ns_path,
                        mac: Some("11:22:33:44:55:66".to_string()),
                    },
                    CniInterface {
                        name: "tap0".to_string(),
                        sandbox: vm_id,
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
            vec![Box::new(move || {
                cleanup_order.lock().unwrap().push("invoke_cni".to_string());
                Ok(())
            })],
        ))
    }
}

fn test_command(socket_path: &str) -> firecracker_sdk::VMCommand {
    VMCommandBuilder::default()
        .with_bin("/bin/sh")
        .with_args(["-c", "touch \"$1\"; sleep 60", "sh", socket_path])
        .with_stdin(CommandStdio::Null)
        .with_stdout(CommandStdio::Null)
        .with_stderr(CommandStdio::Null)
        .build()
}

fn cni_config() -> Config {
    Config {
        disable_validation: true,
        machine_cfg: MachineConfiguration::new(1, 128),
        vmid: "vm-123".to_string(),
        network_interfaces: NetworkInterfaces::from(vec![NetworkInterface {
            cni_configuration: Some(CniConfiguration {
                network_name: Some("fcnet".to_string()),
                vm_if_name: Some("eth0".to_string()),
                ..CniConfiguration::default()
            }),
            ..NetworkInterface::default()
        }]),
        ..Config::default()
    }
}

fn netlink_ops() -> MockNetlinkOps {
    MockNetlinkOps {
        links: vec![Link {
            name: "tap0".to_string(),
            mac_address: Some("11:22:33:44:55:66".to_string()),
            mtu: 1500,
        }],
        ..MockNetlinkOps::default()
    }
}

#[test]
fn test_start_failure_runs_cleanup_in_reverse_order() {
    let cleanup_order = Arc::new(Mutex::new(Vec::new()));
    let mut machine = firecracker_sdk::new_machine(
        cni_config(),
        [
            with_client(Box::new(MockClient::default())),
            with_cni_network_ops(Box::new(RecordingCniOps::new(Arc::clone(&cleanup_order)))),
            with_netlink_ops(Box::new(netlink_ops())),
        ],
    )
    .unwrap();

    machine.handlers.validation = HandlerList::default();
    machine.handlers.fc_init = HandlerList::default().append([
        Handler::new("setup_network", |machine| machine.setup_network()),
        Handler::new("fail", |_machine| Err(Error::Process("boom".to_string()))),
    ]);

    let error = machine.start().unwrap_err();
    assert_eq!("process error: boom", error.to_string());
    assert_eq!(
        vec!["invoke_cni".to_string(), "initialize_netns".to_string()],
        *cleanup_order.lock().unwrap()
    );
    assert!(
        machine.cfg.network_interfaces[0]
            .static_configuration
            .is_some()
    );
}

#[test]
fn test_wait_runs_cleanup_once_in_reverse_order() {
    let cleanup_order = Arc::new(Mutex::new(Vec::new()));
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("machine.sock");
    let mut config = cni_config();
    config.socket_path = socket_path.display().to_string();
    config.net_ns = Some(temp_dir.path().join("machine.netns").display().to_string());

    let mut machine = firecracker_sdk::new_machine(
        config,
        [
            with_client(Box::new(MockClient::default())),
            with_process_runner(test_command(&socket_path.display().to_string())),
            with_cni_network_ops(Box::new(RecordingCniOps::new(Arc::clone(&cleanup_order)))),
            with_netlink_ops(Box::new(netlink_ops())),
        ],
    )
    .unwrap();

    machine.handlers.validation = HandlerList::default();
    machine.handlers.fc_init = HandlerList::default().append([
        Handler::new("setup_network", |machine| machine.setup_network()),
        Handler::new_async("start_vmm", |machine| {
            Box::pin(async move { machine.start_vmm().await })
        }),
    ]);

    let real_netns_cleanups = RealCniNetworkOperations
        .initialize_netns(machine.cfg.net_ns.as_deref().unwrap())
        .unwrap();

    machine.start().unwrap();
    machine.stop_vmm().unwrap();
    assert_eq!(
        vec!["invoke_cni".to_string(), "initialize_netns".to_string()],
        *cleanup_order.lock().unwrap()
    );

    let first_wait_error = machine.wait().unwrap_err().to_string();
    assert!(first_wait_error.contains("firecracker exited:"));
    let second_wait_error = machine.wait().unwrap_err().to_string();
    assert_eq!(first_wait_error, second_wait_error);
    assert_eq!(
        vec!["invoke_cni".to_string(), "initialize_netns".to_string()],
        *cleanup_order.lock().unwrap()
    );

    for cleanup in real_netns_cleanups.into_iter().rev() {
        cleanup().unwrap();
    }
}
