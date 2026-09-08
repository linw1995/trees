use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use snafu::{ResultExt, Snafu};

use crate::paths::{self, StateDirectoryError};
use crate::validation::{self, ValidationError};

#[derive(Debug)]
pub struct WorkspaceLock {
    _file: File,
}

impl WorkspaceLock {
    pub fn acquire(workspace_path: &Path) -> Result<Self, WorkspaceLockError> {
        let workspace_path = validation::resolve_workspace_path(workspace_path)?;
        let state_directory = paths::ensure_state_directory()?;
        let lock_path = state_directory.join(lock_file_name(&workspace_path.into_path_buf()));
        Self::acquire_at(lock_path)
    }

    fn acquire_at(lock_path: PathBuf) -> Result<Self, WorkspaceLockError> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .context(IoSnafu { path: &lock_path })?;
        file.try_lock_exclusive().map_err(|source| {
            if source.kind() == io::ErrorKind::WouldBlock {
                WorkspaceLockError::Busy {
                    path: lock_path.clone(),
                }
            } else {
                WorkspaceLockError::Io {
                    path: lock_path.clone(),
                    source,
                }
            }
        })?;

        Ok(Self { _file: file })
    }
}

fn lock_file_name(workspace_path: &Path) -> String {
    let encoded = workspace_path
        .to_string_lossy()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("codex-{encoded}.lock")
}

#[derive(Debug, Snafu)]
pub enum WorkspaceLockError {
    #[snafu(transparent)]
    Validation { source: ValidationError },
    #[snafu(transparent)]
    StateDirectory { source: StateDirectoryError },
    #[snafu(display(
        "workspace already has an active Codex client (lock: {})",
        path.display()
    ))]
    Busy { path: PathBuf },
    #[snafu(display("failed to lock workspace {}: {source}", path.display()))]
    Io { path: PathBuf, source: io::Error },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn rejects_a_second_lock_holder_and_releases_on_drop() {
        let root = std::env::temp_dir().join(format!("trees-codex-lock-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("lock test directory should be created");
        let lock_path = root.join("workspace.lock");

        let first = WorkspaceLock::acquire_at(lock_path.clone()).expect("first lock should work");
        let error = WorkspaceLock::acquire_at(lock_path.clone())
            .expect_err("second lock holder should fail");
        assert!(matches!(error, WorkspaceLockError::Busy { .. }));

        drop(first);
        let second = WorkspaceLock::acquire_at(lock_path.clone()).expect("lock should release");
        drop(second);
        fs::remove_dir_all(root).expect("lock test directory should be removable");
    }

    #[test]
    fn encodes_workspace_paths_without_path_separators() {
        let name = lock_file_name(Path::new("/workspace/one"));
        assert_eq!(name, "codex-2f776f726b73706163652f6f6e65.lock");
    }
}
