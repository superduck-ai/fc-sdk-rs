#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use firecracker_sdk::fctesting::MockClient;
use firecracker_sdk::{
    CniConfiguration, CommandStdio, Config, Error, Handler, HandlerList, IPConfiguration, Machine,
    MachineConfiguration, NetworkInterface, NetworkInterfaces, RealCniNetworkOperations,
    RealNetlinkOps, VMCommandBuilder, with_client, with_process_runner,
};
use ipnet::Ipv4Net;

unsafe extern "C" {
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
}

fn is_root() -> bool {
    unsafe { libc_geteuid() == 0 }
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

fn build_cni_interface(
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

fn assert_static_configuration(interfaces: &NetworkInterfaces) {
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

fn cleanup_test_command(socket_path: &Path) -> firecracker_sdk::VMCommand {
    VMCommandBuilder::default()
        .with_bin("/bin/sh")
        .with_args(vec![
            "-c".to_string(),
            "touch \"$1\"; sleep 60".to_string(),
            "sh".to_string(),
            socket_path.display().to_string(),
        ])
        .with_stdin(CommandStdio::Null)
        .with_stdout(CommandStdio::Null)
        .with_stderr(CommandStdio::Null)
        .build()
}

#[test]
fn test_real_cni_network_ops_with_conf_file() {
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
    let mut interfaces = build_cni_interface(
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

    assert_static_configuration(&interfaces);
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
fn test_real_cni_network_ops_with_inline_network_config_and_machine_defaults() {
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
        network_interfaces: build_cni_interface(
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
    machine.handlers.validation = HandlerList::default();
    machine.handlers.fc_init = HandlerList::default().append([
        Handler::new("setup_network", |machine| machine.setup_network()),
        Handler::new("fail", |_machine| Err(Error::Process("boom".to_string()))),
    ]);

    let error = machine.start().unwrap_err();
    assert_eq!("process error: boom", error.to_string());
    assert_static_configuration(&machine.cfg.network_interfaces);
    assert_eq!(
        format!(
            "DEL|vm-inline|{}|veth0\nADD|vm-inline|{}|veth0\nDEL|vm-inline|{}|veth0\n",
            net_ns_path.display(),
            net_ns_path.display(),
            net_ns_path.display()
        ),
        fs::read_to_string(&log_path).unwrap()
    );
    assert!(!net_ns_path.exists());
}

#[test]
fn test_machine_wait_runs_real_cni_cleanup() {
    if !is_root() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    let conf_dir = temp_dir.path().join("conf");
    let cache_dir = temp_dir.path().join("cache");
    let netns_dir = temp_dir.path().join("netns");
    let socket_path = temp_dir.path().join("machine.sock");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&conf_dir).unwrap();

    let log_path = temp_dir.path().join("plugin.log");
    let input_path = temp_dir.path().join("plugin-input.json");
    let network_name = "fcnet-cleanup";
    write_fake_cni_plugin(&bin_dir, &log_path, &input_path, network_name);

    let conf_path = conf_dir.join(format!("{network_name}.conflist"));
    fs::write(
        &conf_path,
        format!(
            r#"{{
  "cniVersion": "0.3.1",
  "name": "{network_name}",
  "plugins": [
    {{ "type": "fake-cni" }}
  ]
}}"#
        ),
    )
    .unwrap();

    let net_ns_path = netns_dir.join("vm-cleanup");
    let mut machine = Machine::new_with_opts(
        Config {
            disable_validation: true,
            vmid: "vm-cleanup".to_string(),
            socket_path: socket_path.display().to_string(),
            net_ns: Some(net_ns_path.display().to_string()),
            machine_cfg: MachineConfiguration::new(1, 128),
            network_interfaces: build_cni_interface(
                Some(&conf_dir),
                &cache_dir,
                &bin_dir,
                &net_ns_path,
                network_name,
                None,
            ),
            forward_signals: Some(Vec::new()),
            ..Config::default()
        },
        [
            with_client(Box::new(MockClient::default())),
            with_process_runner(cleanup_test_command(&socket_path)),
        ],
    )
    .unwrap();

    machine.handlers.validation = HandlerList::default();
    machine.handlers.fc_init = HandlerList::default().append([
        Handler::new("setup_network", |machine| machine.setup_network()),
        Handler::new("start_vmm", |machine| machine.start_vmm()),
    ]);

    machine.start().unwrap();

    assert!(net_ns_path.exists());
    assert!(
        cache_dir
            .join("results/fcnet-cleanup-vm-cleanup-veth0")
            .exists()
    );

    machine.stop_vmm().unwrap();
    assert!(machine.wait().is_err());

    assert_eq!(
        format!(
            "DEL|vm-cleanup|{}|veth0\nADD|vm-cleanup|{}|veth0\nDEL|vm-cleanup|{}|veth0\n",
            net_ns_path.display(),
            net_ns_path.display(),
            net_ns_path.display()
        ),
        fs::read_to_string(&log_path).unwrap()
    );
    assert!(
        !cache_dir
            .join("results/fcnet-cleanup-vm-cleanup-veth0")
            .exists()
    );
    assert!(!net_ns_path.exists());
}
