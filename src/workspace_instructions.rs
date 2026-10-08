use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use snafu::{ResultExt, Snafu};

pub const FILE_NAME: &str = "AGENTS.md";
pub const CONTENT: &str = include_str!("workspace_agents.md");

pub fn validate_worktree_name(name: &std::ffi::OsStr) -> Result<(), InstructionsError> {
    snafu::ensure!(name != FILE_NAME, ReservedNameSnafu);
    Ok(())
}

pub fn create(workspace: &Path) -> Result<(), InstructionsError> {
    let path = workspace.join(FILE_NAME);
    let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => file,
        // Existing instructions belong to the user and must never be overwritten.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
        Err(source) => return Err(IoSnafu { path }.into_error(source)),
    };
    file.write_all(CONTENT.as_bytes()).context(IoSnafu { path })
}

pub fn is_managed(path: &Path) -> Result<bool, InstructionsError> {
    if path.file_name() != Some(std::ffi::OsStr::new(FILE_NAME)) {
        return Ok(false);
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => return Err(IoSnafu { path }.into_error(source)),
    };
    if !metadata.is_file() || metadata.len() != CONTENT.len() as u64 {
        return Ok(false);
    }
    Ok(fs::read(path).context(IoSnafu { path })? == CONTENT.as_bytes())
}

pub fn remove(workspace: &Path) -> Result<(), InstructionsError> {
    let path = workspace.join(FILE_NAME);
    if is_managed(&path)? {
        fs::remove_file(&path).context(IoSnafu { path })?;
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum InstructionsError {
    #[snafu(display("repository name AGENTS.md is reserved for shared workspace instructions"))]
    ReservedName,
    #[snafu(display("workspace instructions operation failed for {}: {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

use snafu::IntoError;
