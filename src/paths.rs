use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::domain::WorkspaceId;

const APPLICATION_NAME: &str = "trees";
const DATABASE_NAME: &str = "db.sqlite";
const WORKSPACES_DIRECTORY_NAME: &str = "workspaces";

#[derive(Debug)]
pub enum PathError {
    HomeDirectoryUnavailable,
}

impl fmt::Display for PathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HomeDirectoryUnavailable => {
                formatter.write_str("the user home directory is unavailable")
            }
        }
    }
}

impl std::error::Error for PathError {}

pub fn state_directory() -> Result<PathBuf, PathError> {
    Ok(platform_state_base()?.join(APPLICATION_NAME))
}

pub fn database_path() -> Result<PathBuf, PathError> {
    Ok(state_directory()?.join(DATABASE_NAME))
}

pub fn managed_workspace_directory() -> Result<PathBuf, PathError> {
    Ok(platform_data_base()?
        .join(APPLICATION_NAME)
        .join(WORKSPACES_DIRECTORY_NAME))
}

pub fn generated_workspace_path(workspace_id: &WorkspaceId) -> Result<PathBuf, PathError> {
    Ok(managed_workspace_directory()?.join(format!("ws-{workspace_id}")))
}

pub fn ensure_state_directory() -> Result<PathBuf, StateDirectoryError> {
    let path = state_directory()?;
    if let Err(source) = fs::create_dir_all(&path) {
        return Err(StateDirectoryError {
            kind: StateDirectoryErrorKind::Io { path, source },
        });
    }

    Ok(path)
}

#[derive(Debug)]
pub struct StateDirectoryError {
    kind: StateDirectoryErrorKind,
}

impl StateDirectoryError {
    pub fn new(path: PathBuf, source: io::Error) -> Self {
        Self {
            kind: StateDirectoryErrorKind::Io { path, source },
        }
    }
}

#[derive(Debug)]
enum StateDirectoryErrorKind {
    Path(PathError),
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for StateDirectoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            StateDirectoryErrorKind::Path(error) => error.fmt(formatter),
            StateDirectoryErrorKind::Io { path, source } => write!(
                formatter,
                "failed to create state directory {}: {}",
                path.display(),
                source
            ),
        }
    }
}

impl std::error::Error for StateDirectoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            StateDirectoryErrorKind::Path(error) => Some(error),
            StateDirectoryErrorKind::Io { source, .. } => Some(source),
        }
    }
}

impl From<PathError> for StateDirectoryError {
    fn from(error: PathError) -> Self {
        Self {
            kind: StateDirectoryErrorKind::Path(error),
        }
    }
}

fn platform_state_base() -> Result<PathBuf, PathError> {
    platform_state_base_impl()
}

fn platform_data_base() -> Result<PathBuf, PathError> {
    platform_data_base_impl()
}

#[cfg(target_os = "linux")]
fn platform_state_base_impl() -> Result<PathBuf, PathError> {
    if let Some(path) = non_empty_environment_path("XDG_STATE_HOME") {
        Ok(path)
    } else {
        Ok(home_directory()?.join(".local").join("state"))
    }
}

#[cfg(target_os = "linux")]
fn platform_data_base_impl() -> Result<PathBuf, PathError> {
    if let Some(path) = non_empty_environment_path("XDG_DATA_HOME") {
        Ok(path)
    } else {
        Ok(home_directory()?.join(".local").join("share"))
    }
}

#[cfg(target_os = "macos")]
fn platform_state_base_impl() -> Result<PathBuf, PathError> {
    Ok(home_directory()?
        .join("Library")
        .join("Application Support"))
}

#[cfg(target_os = "macos")]
fn platform_data_base_impl() -> Result<PathBuf, PathError> {
    Ok(home_directory()?
        .join("Library")
        .join("Application Support"))
}

#[cfg(target_os = "windows")]
fn platform_state_base_impl() -> Result<PathBuf, PathError> {
    if let Some(path) = non_empty_environment_path("LOCALAPPDATA") {
        Ok(path)
    } else {
        Ok(home_directory()?.join("AppData").join("Local"))
    }
}

#[cfg(target_os = "windows")]
fn platform_data_base_impl() -> Result<PathBuf, PathError> {
    if let Some(path) = non_empty_environment_path("LOCALAPPDATA") {
        Ok(path)
    } else {
        Ok(home_directory()?.join("AppData").join("Local"))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_state_base_impl() -> Result<PathBuf, PathError> {
    Ok(home_directory()?.join(".local").join("state"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_data_base_impl() -> Result<PathBuf, PathError> {
    Ok(home_directory()?.join(".local").join("share"))
}

fn home_directory() -> Result<PathBuf, PathError> {
    #[cfg(target_os = "windows")]
    {
        return env::var_os("USERPROFILE")
            .or_else(|| env::var_os("HOME"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or(PathError::HomeDirectoryUnavailable);
    }

    #[cfg(not(target_os = "windows"))]
    {
        env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or(PathError::HomeDirectoryUnavailable)
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn non_empty_environment_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_path_uses_global_database_name() {
        let path = database_path().expect("database path should resolve");

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(DATABASE_NAME)
        );
        assert_eq!(
            path.parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str()),
            Some(APPLICATION_NAME)
        );
    }

    #[test]
    fn state_directory_is_absolute() {
        let path = state_directory().expect("state directory should resolve");

        assert!(path.is_absolute());
    }

    #[test]
    fn managed_workspace_directory_is_absolute_and_named_workspaces() {
        let path = managed_workspace_directory().expect("workspace directory should resolve");

        assert!(path.is_absolute());
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("workspaces")
        );
    }

    #[test]
    fn generated_workspace_path_contains_the_workspace_id() {
        let id = WorkspaceId::new();
        let path = generated_workspace_path(&id).expect("generated path should resolve");
        let expected_name = format!("ws-{id}");

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(expected_name.as_str())
        );
    }
}
