use crate::models::{DRIVE_CACHE_TYPE_WRITEBACK, Drive, RateLimiter};

pub const ROOT_DRIVE_NAME: &str = "root_drive";

pub type DriveOpt = Box<dyn Fn(&mut Drive) + Send + Sync + 'static>;

pub fn with_drive_id(id: impl Into<String>) -> DriveOpt {
    let id = id.into();
    Box::new(move |drive| {
        drive.drive_id = Some(id.clone());
    })
}

pub fn with_read_only(flag: bool) -> DriveOpt {
    Box::new(move |drive| {
        drive.is_read_only = Some(flag);
    })
}

pub fn with_partuuid(partuuid: impl Into<String>) -> DriveOpt {
    let partuuid = partuuid.into();
    Box::new(move |drive| {
        drive.partuuid = Some(partuuid.clone());
    })
}

pub fn with_rate_limiter(limiter: RateLimiter) -> DriveOpt {
    Box::new(move |drive| {
        drive.rate_limiter = Some(limiter.clone());
    })
}

pub fn with_cache_type(cache_type: impl Into<String>) -> DriveOpt {
    let cache_type = cache_type.into();
    Box::new(move |drive| {
        drive.cache_type = Some(cache_type.clone());
    })
}

pub fn with_io_engine(io_engine: impl Into<String>) -> DriveOpt {
    let io_engine = io_engine.into();
    Box::new(move |drive| {
        drive.io_engine = Some(io_engine.clone());
    })
}

#[derive(Debug, Clone, Default)]
pub struct DrivesBuilder {
    root_drive: Drive,
    drives: Vec<Drive>,
}

impl DrivesBuilder {
    pub fn new(root_drive_path: impl Into<String>) -> Self {
        let root_drive_path = root_drive_path.into();
        Self {
            root_drive: Drive {
                drive_id: Some(ROOT_DRIVE_NAME.to_string()),
                path_on_host: Some(root_drive_path),
                is_root_device: Some(true),
                is_read_only: Some(false),
                ..Drive::default()
            },
            drives: Vec::new(),
        }
    }

    pub fn with_root_drive(
        mut self,
        root_drive_path: impl Into<String>,
        opts: impl IntoIterator<Item = DriveOpt>,
    ) -> Self {
        self.root_drive = Drive {
            drive_id: Some(ROOT_DRIVE_NAME.to_string()),
            path_on_host: Some(root_drive_path.into()),
            is_root_device: Some(true),
            is_read_only: Some(false),
            ..Drive::default()
        };

        for opt in opts {
            opt(&mut self.root_drive);
        }

        self
    }

    pub fn add_drive(
        mut self,
        path: impl Into<String>,
        read_only: bool,
        opts: impl IntoIterator<Item = DriveOpt>,
    ) -> Self {
        let mut drive = Drive {
            drive_id: Some(self.drives.len().to_string()),
            path_on_host: Some(path.into()),
            is_root_device: Some(false),
            is_read_only: Some(read_only),
            ..Drive::default()
        };

        for opt in opts {
            opt(&mut drive);
        }

        self.drives.push(drive);
        self
    }

    pub fn build(self) -> Vec<Drive> {
        let mut drives = self.drives;
        drives.push(self.root_drive);
        drives
    }
}

#[allow(dead_code)]
pub(crate) fn _default_cache_type() -> &'static str {
    DRIVE_CACHE_TYPE_WRITEBACK
}
