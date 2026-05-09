#![allow(non_snake_case)]

use firecracker_sdk::{
    DRIVE_CACHE_TYPE_WRITEBACK, DRIVE_IO_ENGINE_ASYNC, Drive, DrivesBuilder, ROOT_DRIVE_NAME,
    with_cache_type, with_drive_id, with_io_engine, with_partuuid,
};
use pretty_assertions::assert_eq;

#[test]
fn TestDrivesBuilder() {
    let expected_path = "/path/to/rootfs";
    let expected_drives = vec![Drive {
        drive_id: Some(ROOT_DRIVE_NAME.to_string()),
        path_on_host: Some(expected_path.to_string()),
        is_root_device: Some(true),
        is_read_only: Some(false),
        ..Drive::default()
    }];

    let drives = DrivesBuilder::new(expected_path).build();
    assert_eq!(expected_drives, drives);
}

#[test]
fn TestDrivesBuilderWithRootDrive() {
    let expected_path = "/path/to/rootfs";
    let expected_drives = vec![Drive {
        drive_id: Some("foo".to_string()),
        path_on_host: Some(expected_path.to_string()),
        is_root_device: Some(true),
        is_read_only: Some(false),
        ..Drive::default()
    }];

    let drives = DrivesBuilder::new(expected_path)
        .with_root_drive(expected_path, vec![with_drive_id("foo")])
        .build();

    assert_eq!(expected_drives, drives);
}

#[test]
fn TestDrivesBuilderWithCacheType() {
    let expected_path = "/path/to/rootfs";
    let expected_drives = vec![Drive {
        drive_id: Some(ROOT_DRIVE_NAME.to_string()),
        path_on_host: Some(expected_path.to_string()),
        is_root_device: Some(true),
        is_read_only: Some(false),
        cache_type: Some(DRIVE_CACHE_TYPE_WRITEBACK.to_string()),
        ..Drive::default()
    }];

    let drives = DrivesBuilder::new(expected_path)
        .with_root_drive(
            expected_path,
            vec![
                with_drive_id(ROOT_DRIVE_NAME),
                with_cache_type(DRIVE_CACHE_TYPE_WRITEBACK),
            ],
        )
        .build();

    assert_eq!(expected_drives, drives);
}

#[test]
fn TestDrivesBuilderAddDrive() {
    let root_path = "/root/path";
    let drives = DrivesBuilder::new(root_path)
        .add_drive("/2", true, Vec::new())
        .add_drive("/3", false, Vec::new())
        .add_drive("/4", false, vec![with_partuuid("uuid")])
        .add_drive(
            "/5",
            true,
            vec![with_cache_type(DRIVE_CACHE_TYPE_WRITEBACK)],
        )
        .build();

    let expected = vec![
        Drive {
            drive_id: Some("0".to_string()),
            path_on_host: Some("/2".to_string()),
            is_root_device: Some(false),
            is_read_only: Some(true),
            ..Drive::default()
        },
        Drive {
            drive_id: Some("1".to_string()),
            path_on_host: Some("/3".to_string()),
            is_root_device: Some(false),
            is_read_only: Some(false),
            ..Drive::default()
        },
        Drive {
            drive_id: Some("2".to_string()),
            path_on_host: Some("/4".to_string()),
            is_root_device: Some(false),
            is_read_only: Some(false),
            partuuid: Some("uuid".to_string()),
            ..Drive::default()
        },
        Drive {
            drive_id: Some("3".to_string()),
            path_on_host: Some("/5".to_string()),
            is_root_device: Some(false),
            is_read_only: Some(true),
            cache_type: Some(DRIVE_CACHE_TYPE_WRITEBACK.to_string()),
            ..Drive::default()
        },
        Drive {
            drive_id: Some(ROOT_DRIVE_NAME.to_string()),
            path_on_host: Some(root_path.to_string()),
            is_root_device: Some(true),
            is_read_only: Some(false),
            ..Drive::default()
        },
    ];

    assert_eq!(expected, drives);
}

#[test]
fn TestDrivesBuilderWithIoEngine() {
    let expected_path = "/path/to/rootfs";
    let expected = vec![Drive {
        drive_id: Some(ROOT_DRIVE_NAME.to_string()),
        path_on_host: Some(expected_path.to_string()),
        is_root_device: Some(true),
        is_read_only: Some(false),
        io_engine: Some(DRIVE_IO_ENGINE_ASYNC.to_string()),
        ..Drive::default()
    }];

    let drives = DrivesBuilder::new(expected_path)
        .with_root_drive(
            expected_path,
            vec![
                with_drive_id(ROOT_DRIVE_NAME),
                with_io_engine(DRIVE_IO_ENGINE_ASYNC),
            ],
        )
        .build();

    assert_eq!(expected, drives);
}
