use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::domain::{CanonicalPath, CanonicalPathError};

#[derive(Debug, Clone)]
pub struct RepositoryInfo {
    pub root: CanonicalPath,
    pub common_dir: CanonicalPath,
    pub head: String,
}

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: CanonicalPath,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub prunable: Option<String>,
}

pub fn inspect_repository(repository: &CanonicalPath) -> Result<RepositoryInfo, GitError> {
    let root = path_from_output(
        repository,
        run_git(
            repository.as_path(),
            &[arg("rev-parse"), arg("--show-toplevel")],
        )?,
    )?;
    let common_dir = inspect_repository_identity(repository)?;
    let head = single_line(
        "rev-parse HEAD",
        run_git(repository.as_path(), &[arg("rev-parse"), arg("HEAD")])?,
    )?;

    Ok(RepositoryInfo {
        root,
        common_dir,
        head,
    })
}

pub fn inspect_repository_identity(repository: &CanonicalPath) -> Result<CanonicalPath, GitError> {
    inspect_common_directory(repository.as_path())
}

pub fn inspect_worktree_identity(worktree_path: &Path) -> Result<CanonicalPath, GitError> {
    inspect_common_directory(worktree_path)
}

pub fn list_worktrees(repository: &CanonicalPath) -> Result<Vec<WorktreeInfo>, GitError> {
    let output = run_git(
        repository.as_path(),
        &[arg("worktree"), arg("list"), arg("--porcelain")],
    )?;
    parse_worktree_list(repository, &output)
}

pub fn find_worktree(
    repository: &CanonicalPath,
    worktree_path: &Path,
) -> Result<WorktreeInfo, GitError> {
    list_worktrees(repository)?
        .into_iter()
        .find(|worktree| worktree.path.as_path() == worktree_path)
        .ok_or_else(|| GitError::WorktreeNotFound(worktree_path.to_owned()))
}

pub fn add_detached_worktree(
    repository: &CanonicalPath,
    worktree_path: &Path,
) -> Result<(), GitError> {
    run_git(
        repository.as_path(),
        &[
            arg("worktree"),
            arg("add"),
            arg("--detach"),
            worktree_path.as_os_str().to_owned(),
            arg("HEAD"),
        ],
    )?;
    Ok(())
}

pub fn remove_worktree(repository: &CanonicalPath, worktree_path: &Path) -> Result<(), GitError> {
    run_git(
        repository.as_path(),
        &[
            arg("worktree"),
            arg("remove"),
            arg("--force"),
            worktree_path.as_os_str().to_owned(),
        ],
    )?;
    Ok(())
}

fn parse_worktree_list(
    repository: &CanonicalPath,
    output: &str,
) -> Result<Vec<WorktreeInfo>, GitError> {
    let mut worktrees = Vec::new();
    let mut current: Option<WorktreeInfoBuilder> = None;

    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(builder) = current.take() {
                worktrees.push(builder.build()?);
            }
            current = Some(WorktreeInfoBuilder {
                path: Some(absolute_worktree_path(repository, path)?),
                ..Default::default()
            });
            continue;
        }

        let builder = current.as_mut().ok_or_else(|| GitError::InvalidOutput {
            operation: "worktree list --porcelain".to_owned(),
            output: output.to_owned(),
        })?;
        if let Some(head) = line.strip_prefix("HEAD ") {
            builder.head = Some(head.to_owned());
        } else if let Some(branch) = line.strip_prefix("branch ") {
            builder.branch = Some(branch.to_owned());
        } else if line == "detached" {
            builder.detached = true;
        } else if line == "bare" {
            builder.bare = true;
        } else if let Some(reason) = line.strip_prefix("prunable ") {
            builder.prunable = Some(reason.to_owned());
        }
    }

    if let Some(builder) = current {
        worktrees.push(builder.build()?);
    }

    Ok(worktrees)
}

#[derive(Default)]
struct WorktreeInfoBuilder {
    path: Option<CanonicalPath>,
    head: Option<String>,
    branch: Option<String>,
    detached: bool,
    bare: bool,
    prunable: Option<String>,
}

impl WorktreeInfoBuilder {
    fn build(self) -> Result<WorktreeInfo, GitError> {
        Ok(WorktreeInfo {
            path: self.path.ok_or_else(|| GitError::InvalidOutput {
                operation: "worktree list --porcelain".to_owned(),
                output: "worktree entry has no path".to_owned(),
            })?,
            head: self.head,
            branch: self.branch,
            detached: self.detached,
            bare: self.bare,
            prunable: self.prunable,
        })
    }
}

fn absolute_worktree_path(
    repository: &CanonicalPath,
    output: &str,
) -> Result<CanonicalPath, GitError> {
    let path = PathBuf::from(output);
    let path = if path.is_absolute() {
        path
    } else {
        repository.as_path().join(path)
    };
    CanonicalPath::from_absolute(path).map_err(GitError::Canonicalize)
}

fn path_from_output(repository: &CanonicalPath, output: String) -> Result<CanonicalPath, GitError> {
    let path = PathBuf::from(output.trim());
    let path = if path.is_absolute() {
        path
    } else {
        repository.as_path().join(path)
    };
    CanonicalPath::resolve(path).map_err(GitError::Canonicalize)
}

fn inspect_common_directory(path: &Path) -> Result<CanonicalPath, GitError> {
    let output = single_line(
        "rev-parse --git-common-dir",
        run_git(path, &[arg("rev-parse"), arg("--git-common-dir")])?,
    )?;
    let common_dir = PathBuf::from(output);
    let common_dir = if common_dir.is_absolute() {
        common_dir
    } else {
        path.join(common_dir)
    };
    CanonicalPath::resolve(common_dir).map_err(GitError::Canonicalize)
}

fn single_line(operation: &str, output: String) -> Result<String, GitError> {
    let value = output.trim();
    if value.is_empty() || value.lines().count() != 1 {
        return Err(GitError::InvalidOutput {
            operation: operation.to_owned(),
            output,
        });
    }
    Ok(value.to_owned())
}

fn arg(value: &str) -> OsString {
    OsString::from(value)
}

fn run_git(repository: &Path, args: &[OsString]) -> Result<String, GitError> {
    let operation = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .map_err(|source| GitError::Io {
            operation: operation.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(GitError::CommandFailed {
            operation,
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    String::from_utf8(output.stdout).map_err(|_| GitError::InvalidUtf8 { operation })
}

#[derive(Debug)]
pub enum GitError {
    Io {
        operation: String,
        source: std::io::Error,
    },
    CommandFailed {
        operation: String,
        status: Option<i32>,
        stderr: String,
    },
    InvalidUtf8 {
        operation: String,
    },
    InvalidOutput {
        operation: String,
        output: String,
    },
    Canonicalize(CanonicalPathError),
    WorktreeNotFound(PathBuf),
}

impl fmt::Display for GitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => {
                write!(formatter, "failed to run git {operation}: {source}")
            }
            Self::CommandFailed {
                operation,
                status,
                stderr,
            } => write!(
                formatter,
                "git {operation} failed with status {:?}: {}",
                status, stderr
            ),
            Self::InvalidUtf8 { operation } => {
                write!(formatter, "git {operation} returned invalid UTF-8")
            }
            Self::InvalidOutput { operation, output } => {
                write!(
                    formatter,
                    "git {operation} returned invalid output: {output:?}"
                )
            }
            Self::Canonicalize(error) => error.fmt(formatter),
            Self::WorktreeNotFound(path) => {
                write!(formatter, "Git did not report worktree: {}", path.display())
            }
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Canonicalize(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use super::*;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("trees-git-{}", uuid::Uuid::now_v7()))
    }

    fn run_git_in(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repository() -> (PathBuf, CanonicalPath) {
        let root = test_root();
        fs::create_dir_all(&root).expect("test root should be created");
        run_git_in(&root, &["init", "-q"]);
        run_git_in(&root, &["config", "user.email", "trees@example.invalid"]);
        run_git_in(&root, &["config", "user.name", "trees tests"]);
        fs::write(root.join("README"), "test\n").expect("test file should be written");
        run_git_in(&root, &["add", "README"]);
        run_git_in(&root, &["commit", "-qm", "initial"]);
        let repository = CanonicalPath::resolve(&root).expect("repository should resolve");
        (root, repository)
    }

    #[test]
    fn creates_lists_and_removes_a_detached_worktree() {
        let (root, repository) = repository();
        let worktree_path = repository.as_path().join("workspace").join("repo");
        fs::create_dir_all(worktree_path.parent().unwrap()).expect("workspace parent should exist");

        let info = inspect_repository(&repository).expect("repository should be inspectable");
        assert_eq!(info.root, repository);
        assert!(!info.head.is_empty());
        assert!(info.common_dir.as_path().ends_with(".git"));

        add_detached_worktree(&repository, &worktree_path).expect("worktree should be added");
        let worktrees = list_worktrees(&repository).expect("worktrees should be listed");
        let created = worktrees
            .iter()
            .find(|worktree| worktree.path.as_path() == worktree_path)
            .expect("created worktree should be listed");
        assert!(created.detached);
        assert_eq!(created.head.as_deref(), Some(info.head.as_str()));
        assert_eq!(
            inspect_repository_identity(&repository).expect("repository identity should exist"),
            inspect_worktree_identity(&worktree_path).expect("worktree identity should exist")
        );
        assert!(worktree_path.join("README").exists());

        remove_worktree(&repository, &worktree_path).expect("worktree should be removed");
        assert!(!worktree_path.exists());
        assert!(!list_worktrees(&repository)
            .expect("worktrees should be listed")
            .iter()
            .any(|worktree| worktree.path.as_path() == worktree_path));
        fs::remove_dir_all(root).expect("test root should be removable");
    }
}
