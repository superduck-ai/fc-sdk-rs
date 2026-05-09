use crate::models::MemoryBackend;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotConfig {
    pub mem_file_path: Option<String>,
    pub mem_backend: Option<MemoryBackend>,
    pub snapshot_path: Option<String>,
    pub enable_diff_snapshots: bool,
    pub resume_vm: bool,
}

impl SnapshotConfig {
    pub fn with_paths(mem_file_path: impl Into<String>, snapshot_path: impl Into<String>) -> Self {
        let mem_file_path = mem_file_path.into();
        let snapshot_path = snapshot_path.into();

        Self {
            mem_file_path: (!mem_file_path.is_empty()).then_some(mem_file_path),
            snapshot_path: (!snapshot_path.is_empty()).then_some(snapshot_path),
            ..Self::default()
        }
    }

    pub fn get_mem_backend_path(&self) -> Option<&str> {
        self.mem_backend
            .as_ref()
            .and_then(|backend| backend.backend_path.as_deref())
            .or(self.mem_file_path.as_deref())
    }
}
