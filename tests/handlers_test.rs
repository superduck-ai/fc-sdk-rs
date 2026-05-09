#![allow(non_snake_case)]

use std::os::unix::fs::FileTypeExt;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use firecracker_sdk::fctesting::MockClient;
use firecracker_sdk::{
    Config, FifoLogWriter, Handler, HandlerList, MMDSVersion, Machine, MachineConfiguration,
    NetworkInterface, NoopClient, StaticNetworkConfiguration, VsockDevice, add_vsocks_handler,
    attach_drives_handler, bootstrap_logging_handler, config_mmds_handler,
    create_boot_source_handler, create_log_files_handler, create_machine_handler,
    create_network_interfaces_handler, new_set_metadata_handler,
};

fn compare_handler_names(expected: &[&str], actual: &HandlerList) {
    let actual_names = actual
        .list
        .iter()
        .map(|handler| handler.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(expected, actual_names);
}

fn compare_handler_lists(expected: &HandlerList, actual: &HandlerList) {
    assert_eq!(expected.len(), actual.len());
    for (expected, actual) in expected.list.iter().zip(actual.list.iter()) {
        assert_eq!(expected.name, actual.name);
        assert!(Arc::ptr_eq(&expected.func, &actual.func));
    }
}

#[test]
fn TestHandlerListAppend() {
    let list = HandlerList::default();
    let _ = list.clone().append([Handler::new("foo", |_| Ok(()))]);
    assert_eq!(0, list.len());

    let list = list.append([
        Handler::new("foo", |_| Ok(())),
        Handler::new("bar", |_| Ok(())),
        Handler::new("baz", |_| Ok(())),
    ]);
    compare_handler_names(&["foo", "bar", "baz"], &list);
}

#[test]
fn TestHandlerListPrepend() {
    let list = HandlerList::default();
    let _ = list.clone().prepend([Handler::new("foo", |_| Ok(()))]);
    assert_eq!(0, list.len());

    let list = list.prepend([
        Handler::new("foo", |_| Ok(())),
        Handler::new("bar", |_| Ok(())),
        Handler::new("baz", |_| Ok(())),
    ]);
    compare_handler_names(&["foo", "bar", "baz"], &list);
}

#[test]
fn TestHandlerListRemove() {
    let list = HandlerList::default().append([
        Handler::new("foo", |_| Ok(())),
        Handler::new("bar", |_| Ok(())),
        Handler::new("baz", |_| Ok(())),
        Handler::new("foo", |_| Ok(())),
        Handler::new("baz", |_| Ok(())),
    ]);

    let _ = list.clone().remove("foo");
    assert_eq!(5, list.len());

    let list = list.remove("foo");
    assert_eq!(3, list.len());
    assert_eq!("bar", list.list[0].name);

    let list = list.remove("invalid-name");
    assert_eq!(3, list.len());

    let list = list.remove("baz");
    assert_eq!(1, list.len());

    let list = list.remove("bar");
    assert_eq!(0, list.len());
}

#[test]
fn TestHandlerListClear() {
    let list = HandlerList::default().append([
        Handler::new("foo", |_| Ok(())),
        Handler::new("foo", |_| Ok(())),
        Handler::new("foo", |_| Ok(())),
        Handler::new("foo", |_| Ok(())),
        Handler::new("foo", |_| Ok(())),
        Handler::new("foo", |_| Ok(())),
        Handler::new("foo", |_| Ok(())),
    ]);

    let _ = list.clone().clear();
    assert_eq!(7, list.len());

    let list = list.clear();
    assert_eq!(0, list.len());
}

#[test]
fn TestHandlerListHas() {
    let cases = [
        (
            "foo",
            HandlerList::default().append([Handler::new("foo", |_| Ok(()))]),
            true,
        ),
        ("foo", HandlerList::default(), false),
        (
            "foo",
            HandlerList::default().append([Handler::new("foo1", |_| Ok(()))]),
            false,
        ),
    ];

    for (name, list, expected) in cases {
        assert_eq!(expected, list.has(name));
    }
}

#[test]
fn TestHandlerListSwappend() {
    let replacement = Handler::new("foo", |_| Ok(()));
    let cases = vec![
        (
            HandlerList::default().append([Handler::new("bar", |_| Ok(()))]),
            replacement.clone(),
            HandlerList::default().append([
                Handler::new("bar", |_| Ok(())),
                replacement.clone(),
            ]),
        ),
        (
            HandlerList::default().append([
                Handler::new("bar", |_| Ok(())),
                Handler::new("foo", |_| Ok(())),
            ]),
            replacement.clone(),
            HandlerList::default().append([
                Handler::new("bar", |_| Ok(())),
                replacement.clone(),
            ]),
        ),
        (
            HandlerList::default().append([
                Handler::new("foo", |_| Ok(())),
                Handler::new("bar", |_| Ok(())),
                Handler::new("foo", |_| Ok(())),
            ]),
            replacement.clone(),
            HandlerList::default().append([
                replacement.clone(),
                Handler::new("bar", |_| Ok(())),
                replacement.clone(),
            ]),
        ),
    ];

    for (list, handler, expected) in cases {
        let actual = list.swappend(handler);
        compare_handler_lists(&expected, &actual);
    }
}

#[test]
fn TestHandlerListReplace() {
    let replacement = Handler::new("foo", |_| Ok(()));
    let cases = vec![
        (
            HandlerList::default().append([Handler::new("bar", |_| Ok(()))]),
            replacement.clone(),
            HandlerList::default().append([Handler::new("bar", |_| Ok(()))]),
        ),
        (
            HandlerList::default().append([
                Handler::new("bar", |_| Ok(())),
                Handler::new("foo", |_| Ok(())),
            ]),
            replacement.clone(),
            HandlerList::default().append([
                Handler::new("bar", |_| Ok(())),
                replacement.clone(),
            ]),
        ),
        (
            HandlerList::default().append([
                Handler::new("foo", |_| Ok(())),
                Handler::new("bar", |_| Ok(())),
                Handler::new("foo", |_| Ok(())),
            ]),
            replacement.clone(),
            HandlerList::default().append([
                replacement.clone(),
                Handler::new("bar", |_| Ok(())),
                replacement.clone(),
            ]),
        ),
    ];

    for (list, handler, expected) in cases {
        let actual = list.swap(handler);
        compare_handler_lists(&expected, &actual);
    }
}

#[test]
fn TestHandlerListAppendAfter() {
    let cases = vec![
        (
            HandlerList::default().append([
                Handler::new("foo", |_| Ok(())),
                Handler::new("bar", |_| Ok(())),
                Handler::new("baz", |_| Ok(())),
            ]),
            "not exist",
            Handler::new("qux", |_| Ok(())),
            HandlerList::default().append([
                Handler::new("foo", |_| Ok(())),
                Handler::new("bar", |_| Ok(())),
                Handler::new("baz", |_| Ok(())),
            ]),
        ),
        (
            HandlerList::default().append([
                Handler::new("foo", |_| Ok(())),
                Handler::new("bar", |_| Ok(())),
                Handler::new("baz", |_| Ok(())),
            ]),
            "foo",
            Handler::new("qux", |_| Ok(())),
            HandlerList::default().append([
                Handler::new("foo", |_| Ok(())),
                Handler::new("qux", |_| Ok(())),
                Handler::new("bar", |_| Ok(())),
                Handler::new("baz", |_| Ok(())),
            ]),
        ),
    ];

    for (list, after_name, handler, expected) in cases {
        let actual = list.append_after(after_name, handler);
        compare_handler_lists(&expected, &actual);
    }
}

#[test]
fn TestHandlerListRun() {
    let count = Arc::new(Mutex::new(0));
    let count_foo = count.clone();
    let count_bar = count.clone();
    let count_qux = count.clone();
    let baz_err = "baz error".to_string();
    let baz_err_clone = baz_err.clone();

    let list = HandlerList::default().append([
        Handler::new("foo", move |_| {
            *count_foo.lock().unwrap() += 1;
            Ok(())
        }),
        Handler::new("bar", move |_| {
            *count_bar.lock().unwrap() += 10;
            Ok(())
        }),
        Handler::new("baz", move |_| {
            Err(firecracker_sdk::Error::Process(baz_err_clone.clone()))
        }),
        Handler::new("qux", move |_| {
            *count_qux.lock().unwrap() *= 100;
            Ok(())
        }),
    ]);

    let mut machine = Machine::new_with_client(Config::default(), Box::new(NoopClient)).unwrap();
    let error = list.run(&mut machine).unwrap_err();
    assert_eq!(format!("process error: {baz_err}"), error.to_string());
    assert_eq!(11, *count.lock().unwrap());

    let list = list.remove("baz");
    list.run(&mut machine).unwrap();
    assert_eq!(2200, *count.lock().unwrap());
}

#[test]
fn TestHandlers() {
    let called = Arc::new(Mutex::new(String::new()));
    let metadata = serde_json::json!({ "foo": "bar", "baz": "qux" });

    let cases: Vec<(&str, Handler, Config, MockClient)> = vec![
        (
            firecracker_sdk::BOOTSTRAP_LOGGING_HANDLER_NAME,
            bootstrap_logging_handler(),
            Config {
                log_level: Some("Debug".to_string()),
                log_fifo: Some("/tmp/firecracker.log".to_string()),
                metrics_fifo: Some("/tmp/firecracker-metrics".to_string()),
                ..Config::default()
            },
            MockClient {
                put_logger_fn: Some(Box::new({
                    let called = called.clone();
                    move |_| {
                        *called.lock().unwrap() =
                            firecracker_sdk::BOOTSTRAP_LOGGING_HANDLER_NAME.to_string();
                        Ok(())
                    }
                })),
                put_metrics_fn: Some(Box::new(|_| Ok(()))),
                ..MockClient::default()
            },
        ),
        (
            firecracker_sdk::CREATE_MACHINE_HANDLER_NAME,
            create_machine_handler(),
            Config {
                machine_cfg: MachineConfiguration::new(1, 128),
                ..Config::default()
            },
            MockClient {
                put_machine_configuration_fn: Some(Box::new({
                    let called = called.clone();
                    move |_| {
                        *called.lock().unwrap() =
                            firecracker_sdk::CREATE_MACHINE_HANDLER_NAME.to_string();
                        Ok(())
                    }
                })),
                get_machine_configuration_fn: Some(Box::new(
                    || Ok(MachineConfiguration::default()),
                )),
                ..MockClient::default()
            },
        ),
        (
            firecracker_sdk::CREATE_BOOT_SOURCE_HANDLER_NAME,
            create_boot_source_handler(),
            Config {
                kernel_image_path: "/tmp/vmlinux".to_string(),
                ..Config::default()
            },
            MockClient {
                put_guest_boot_source_fn: Some(Box::new({
                    let called = called.clone();
                    move |_| {
                        *called.lock().unwrap() =
                            firecracker_sdk::CREATE_BOOT_SOURCE_HANDLER_NAME.to_string();
                        Ok(())
                    }
                })),
                ..MockClient::default()
            },
        ),
        (
            firecracker_sdk::ATTACH_DRIVES_HANDLER_NAME,
            attach_drives_handler(),
            Config {
                drives: firecracker_sdk::DrivesBuilder::new("/foo/bar").build(),
                ..Config::default()
            },
            MockClient {
                put_guest_drive_by_id_fn: Some(Box::new({
                    let called = called.clone();
                    move |_, _| {
                        *called.lock().unwrap() =
                            firecracker_sdk::ATTACH_DRIVES_HANDLER_NAME.to_string();
                        Ok(())
                    }
                })),
                ..MockClient::default()
            },
        ),
        (
            firecracker_sdk::CREATE_NETWORK_INTERFACES_HANDLER_NAME,
            create_network_interfaces_handler(),
            Config {
                network_interfaces: vec![NetworkInterface {
                    static_configuration: Some(
                        StaticNetworkConfiguration::new("host").with_mac_address("macaddress"),
                    ),
                    ..NetworkInterface::default()
                }]
                .into(),
                ..Config::default()
            },
            MockClient {
                put_guest_network_interface_by_id_fn: Some(Box::new({
                    let called = called.clone();
                    move |_, _| {
                        *called.lock().unwrap() =
                            firecracker_sdk::CREATE_NETWORK_INTERFACES_HANDLER_NAME.to_string();
                        Ok(())
                    }
                })),
                ..MockClient::default()
            },
        ),
        (
            firecracker_sdk::ADD_VSOCKS_HANDLER_NAME,
            add_vsocks_handler(),
            Config {
                vsock_devices: vec![VsockDevice {
                    path: "path".to_string(),
                    cid: 123,
                    ..VsockDevice::default()
                }],
                ..Config::default()
            },
            MockClient {
                put_guest_vsock_fn: Some(Box::new({
                    let called = called.clone();
                    move |_| {
                        *called.lock().unwrap() =
                            firecracker_sdk::ADD_VSOCKS_HANDLER_NAME.to_string();
                        Ok(())
                    }
                })),
                ..MockClient::default()
            },
        ),
        (
            firecracker_sdk::NEW_SET_METADATA_HANDLER_NAME,
            new_set_metadata_handler(metadata.clone()),
            Config::default(),
            MockClient {
                put_mmds_fn: Some(Box::new({
                    let called = called.clone();
                    let metadata = metadata.clone();
                    move |body| {
                        *called.lock().unwrap() =
                            firecracker_sdk::NEW_SET_METADATA_HANDLER_NAME.to_string();
                        assert_eq!(&metadata, body);
                        Ok(())
                    }
                })),
                ..MockClient::default()
            },
        ),
        (
            firecracker_sdk::CONFIG_MMDS_HANDLER_NAME,
            config_mmds_handler(),
            Config {
                mmds_address: Some("169.254.169.254".parse().unwrap()),
                network_interfaces: vec![NetworkInterface {
                    static_configuration: Some(
                        StaticNetworkConfiguration::new("host").with_mac_address("macaddress"),
                    ),
                    allow_mmds: true,
                    ..NetworkInterface::default()
                }]
                .into(),
                ..Config::default()
            },
            MockClient {
                put_mmds_config_fn: Some(Box::new({
                    let called = called.clone();
                    move |config| {
                        *called.lock().unwrap() =
                            firecracker_sdk::CONFIG_MMDS_HANDLER_NAME.to_string();
                        assert_eq!(Some("169.254.169.254".to_string()), config.ipv4_address);
                        assert_eq!(vec!["1".to_string()], config.network_interfaces);
                        assert_eq!(Some("V1".to_string()), config.version);
                        Ok(())
                    }
                })),
                ..MockClient::default()
            },
        ),
        (
            firecracker_sdk::CONFIG_MMDS_HANDLER_NAME,
            config_mmds_handler(),
            Config {
                mmds_version: MMDSVersion::V2,
                network_interfaces: vec![NetworkInterface {
                    static_configuration: Some(
                        StaticNetworkConfiguration::new("host").with_mac_address("macaddress"),
                    ),
                    allow_mmds: true,
                    ..NetworkInterface::default()
                }]
                .into(),
                ..Config::default()
            },
            MockClient {
                put_mmds_config_fn: Some(Box::new({
                    let called = called.clone();
                    move |config| {
                        *called.lock().unwrap() =
                            firecracker_sdk::CONFIG_MMDS_HANDLER_NAME.to_string();
                        assert_eq!(Some("V2".to_string()), config.version);
                        assert_eq!(vec!["1".to_string()], config.network_interfaces);
                        Ok(())
                    }
                })),
                ..MockClient::default()
            },
        ),
    ];

    for (expected_name, handler, config, client) in cases {
        *called.lock().unwrap() = String::new();
        let mut machine = Machine::new_with_client(config, Box::new(client)).unwrap();
        (handler.func)(&mut machine).unwrap();
        assert_eq!(expected_name, called.lock().unwrap().as_str());
    }
}

#[test]
fn test_create_log_files_handler_creates_and_cleans_up_fifo_paths() {
    let temp_dir = tempfile::tempdir().unwrap();
    let log_fifo = temp_dir.path().join("firecracker.log");
    let metrics_file = temp_dir.path().join("firecracker.metrics");

    let mut machine = Machine::new_with_client(
        Config {
            log_fifo: Some(log_fifo.display().to_string()),
            metrics_path: Some(metrics_file.display().to_string()),
            ..Config::default()
        },
        Box::new(NoopClient),
    )
    .unwrap();

    (create_log_files_handler().func)(&mut machine).unwrap();

    assert!(std::fs::metadata(&log_fifo).unwrap().file_type().is_fifo());
    assert!(std::fs::metadata(&metrics_file).unwrap().is_file());

    machine.wait().unwrap();

    assert!(!log_fifo.exists());
    assert!(metrics_file.exists());
}

#[test]
fn test_create_log_files_handler_captures_fifo_log_to_writer() {
    let temp_dir = tempfile::tempdir().unwrap();
    let log_fifo = temp_dir.path().join("firecracker.log");
    let captured_log = temp_dir.path().join("captured.log");
    let writer = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(true)
        .open(&captured_log)
        .unwrap();

    let mut machine = Machine::new_with_client(
        Config {
            log_fifo: Some(log_fifo.display().to_string()),
            fifo_log_writer: Some(FifoLogWriter::new(writer)),
            ..Config::default()
        },
        Box::new(NoopClient),
    )
    .unwrap();

    (create_log_files_handler().func)(&mut machine).unwrap();

    let mut fifo_writer = std::fs::OpenOptions::new()
        .write(true)
        .open(&log_fifo)
        .unwrap();
    std::io::Write::write_all(&mut fifo_writer, b"hello from fifo\n").unwrap();
    drop(fifo_writer);

    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let contents = std::fs::read_to_string(&captured_log).unwrap();
        if contents.contains("hello from fifo") {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for fifo capture"
        );
        thread::sleep(Duration::from_millis(10));
    }

    machine.signal_exit();
    machine.wait().unwrap();
}
