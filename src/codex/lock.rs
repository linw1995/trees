use std::fmt;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::paths::{self, StateDirectoryError};
use crate::validation::{self, ValidationError};

#[derive(Debug)]
pub struct WorkspaceLock {
    _file: File,
}

impl WorkspaceLock {
    pub fn acquire(workspace_path: &Path) -> Result<Self, WorkspaceLockError> {
        let workspace_path = validation::resolve_workspace_path(workspace_path)
            .map_err(WorkspaceLockError::Validation)?;
        let state_directory =
            paths::ensure_state_directory().map_err(WorkspaceLockError::StateDirectory)?;
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
            .map_err(|source| WorkspaceLockError::Io {
                path: lock_path.clone(),
                source,
            })?;
        file.try_lock_exclusive().map_err(|source| {
            if source.kind() == io::ErrorKind::WouldBlock {
                WorkspaceLockError::Busy(lock_path.clone())
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

#[derive(Debug)]
pub enum WorkspaceLockError {
    Validation(ValidationError),
    StateDirectory(StateDirectoryError),
    Busy(PathBuf),
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for WorkspaceLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(formatter),
            Self::StateDirectory(error) => error.fmt(formatter),
            Self::Busy(path) => write!(
                formatter,
                "workspace already has an active Codex client (lock: {})",
                path.display()
            ),
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "failed to lock workspace {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for WorkspaceLockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(error) => Some(error),
            Self::StateDirectory(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::Busy(_) => None,
        }
    }
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
        assert!(matches!(error, WorkspaceLockError::Busy(_)));

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
