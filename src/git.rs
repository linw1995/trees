use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::domain::{CanonicalPath, CanonicalPathError};
use snafu::{IntoError, ResultExt, Snafu};

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

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ObservationFingerprint {
    pub repository_identity: CanonicalPath,
    pub worktree_identity: CanonicalPath,
    pub head: Option<String>,
    pub detached: Option<bool>,
    pub branch: Option<String>,
    pub existence: WorktreeExistence,
    pub prunable: Option<String>,
    pub clean: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeExistence {
    Present,
    Missing,
    Prunable,
}

impl ObservationFingerprint {
    pub fn from_worktree(
        repository_identity: CanonicalPath,
        worktree: WorktreeInfo,
        clean: bool,
    ) -> Self {
        let existence = if worktree.prunable.is_some() {
            WorktreeExistence::Prunable
        } else {
            WorktreeExistence::Present
        };
        Self {
            repository_identity,
            worktree_identity: worktree.path,
            head: worktree.head,
            detached: Some(worktree.detached),
            branch: worktree.branch,
            existence,
            prunable: worktree.prunable,
            clean,
        }
    }

    pub fn missing(repository_identity: CanonicalPath, worktree_identity: CanonicalPath) -> Self {
        Self {
            repository_identity,
            worktree_identity,
            head: None,
            detached: None,
            branch: None,
            existence: WorktreeExistence::Missing,
            prunable: None,
            clean: false,
        }
    }

    pub fn matches_attached(
        &self,
        repository_identity: &CanonicalPath,
        worktree_identity: &CanonicalPath,
        expected_head: Option<&str>,
    ) -> bool {
        self.matches_attachment(repository_identity, worktree_identity, expected_head) && self.clean
    }

    pub fn matches_attachment(
        &self,
        repository_identity: &CanonicalPath,
        worktree_identity: &CanonicalPath,
        expected_head: Option<&str>,
    ) -> bool {
        self.repository_identity == *repository_identity
            && self.worktree_identity == *worktree_identity
            && self.head.as_deref() == expected_head
            && self.detached == Some(true)
            && self.branch.is_none()
            && self.existence == WorktreeExistence::Present
            && self.prunable.is_none()
    }
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

pub fn inspect_upstream_repository(repository: &CanonicalPath) -> Result<RepositoryInfo, GitError> {
    let common_dir = inspect_repository_identity(repository)?;
    let upstream = list_worktrees(repository)?
        .into_iter()
        .next()
        .ok_or_else(|| GitError::InvalidOutput {
            operation: "worktree list --porcelain".to_owned(),
            output: "repository has no worktrees".to_owned(),
        })?;
    let head = upstream.head.ok_or_else(|| GitError::InvalidOutput {
        operation: "worktree list --porcelain".to_owned(),
        output: "upstream worktree has no HEAD".to_owned(),
    })?;
    Ok(RepositoryInfo {
        root: upstream.path,
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
        .ok_or_else(|| GitError::WorktreeNotFound {
            path: worktree_path.to_owned(),
        })
}

pub fn is_worktree_clean(worktree_path: &Path) -> Result<bool, GitError> {
    let output = run_git(
        worktree_path,
        &[
            arg("status"),
            arg("--porcelain=v1"),
            arg("--untracked-files=all"),
        ],
    )?;
    Ok(output.trim().is_empty())
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

pub fn add_detached_worktree_with_heartbeat<F>(
    repository: &CanonicalPath,
    worktree_path: &Path,
    heartbeat: F,
) -> Result<(), GitError>
where
    F: FnMut() -> Result<(), GitError>,
{
    add_detached_worktree_at_with_heartbeat(repository, worktree_path, "HEAD", heartbeat)
}

pub fn add_detached_worktree_at_with_heartbeat<F>(
    repository: &CanonicalPath,
    worktree_path: &Path,
    revision: &str,
    heartbeat: F,
) -> Result<(), GitError>
where
    F: FnMut() -> Result<(), GitError>,
{
    run_git_with_heartbeat(
        repository.as_path(),
        &[
            arg("worktree"),
            arg("add"),
            arg("--detach"),
            worktree_path.as_os_str().to_owned(),
            arg(revision),
        ],
        heartbeat,
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

pub fn remove_worktree_with_heartbeat<F>(
    repository: &CanonicalPath,
    worktree_path: &Path,
    heartbeat: F,
) -> Result<(), GitError>
where
    F: FnMut() -> Result<(), GitError>,
{
    run_git_with_heartbeat(
        repository.as_path(),
        &[
            arg("worktree"),
            arg("remove"),
            arg("--force"),
            worktree_path.as_os_str().to_owned(),
        ],
        heartbeat,
    )?;
    Ok(())
}

pub fn remove_clean_worktree(
    repository: &CanonicalPath,
    worktree_path: &Path,
) -> Result<(), GitError> {
    run_git(
        repository.as_path(),
        &[
            arg("worktree"),
            arg("remove"),
            worktree_path.as_os_str().to_owned(),
        ],
    )?;
    Ok(())
}

pub fn remove_clean_worktree_with_heartbeat<F>(
    repository: &CanonicalPath,
    worktree_path: &Path,
    heartbeat: F,
) -> Result<(), GitError>
where
    F: FnMut() -> Result<(), GitError>,
{
    run_git_with_heartbeat(
        repository.as_path(),
        &[
            arg("worktree"),
            arg("remove"),
            worktree_path.as_os_str().to_owned(),
        ],
        heartbeat,
    )?;
    Ok(())
}

pub fn inspect_repository_identity_with_heartbeat<F>(
    repository: &CanonicalPath,
    heartbeat: F,
) -> Result<CanonicalPath, GitError>
where
    F: FnMut() -> Result<(), GitError>,
{
    inspect_common_directory_with_heartbeat(repository.as_path(), heartbeat)
}

pub fn inspect_worktree_identity_with_heartbeat<F>(
    worktree_path: &Path,
    heartbeat: F,
) -> Result<CanonicalPath, GitError>
where
    F: FnMut() -> Result<(), GitError>,
{
    inspect_common_directory_with_heartbeat(worktree_path, heartbeat)
}

pub fn list_worktrees_with_heartbeat<F>(
    repository: &CanonicalPath,
    heartbeat: F,
) -> Result<Vec<WorktreeInfo>, GitError>
where
    F: FnMut() -> Result<(), GitError>,
{
    let output = run_git_with_heartbeat(
        repository.as_path(),
        &[arg("worktree"), arg("list"), arg("--porcelain")],
        heartbeat,
    )?;
    parse_worktree_list(repository, &output)
}

pub fn is_worktree_clean_with_heartbeat<F>(
    worktree_path: &Path,
    heartbeat: F,
) -> Result<bool, GitError>
where
    F: FnMut() -> Result<(), GitError>,
{
    let output = run_git_with_heartbeat(
        worktree_path,
        &[
            arg("status"),
            arg("--porcelain=v1"),
            arg("--untracked-files=all"),
        ],
        heartbeat,
    )?;
    Ok(output.trim().is_empty())
}

pub fn checkout_detached_with_heartbeat<F>(
    worktree_path: &Path,
    revision: &str,
    heartbeat: F,
) -> Result<(), GitError>
where
    F: FnMut() -> Result<(), GitError>,
{
    run_git_with_heartbeat(
        worktree_path,
        &[
            arg("checkout"),
            arg("--detach"),
            arg("--no-overwrite-ignore"),
            arg(revision),
        ],
        heartbeat,
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
    Ok(CanonicalPath::from_absolute(path)?)
}

fn path_from_output(repository: &CanonicalPath, output: String) -> Result<CanonicalPath, GitError> {
    let path = PathBuf::from(output.trim());
    let path = if path.is_absolute() {
        path
    } else {
        repository.as_path().join(path)
    };
    Ok(CanonicalPath::resolve(path)?)
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
    Ok(CanonicalPath::resolve(common_dir)?)
}

fn inspect_common_directory_with_heartbeat<F>(
    path: &Path,
    heartbeat: F,
) -> Result<CanonicalPath, GitError>
where
    F: FnMut() -> Result<(), GitError>,
{
    let output = single_line(
        "rev-parse --git-common-dir",
        run_git_with_heartbeat(
            path,
            &[arg("rev-parse"), arg("--git-common-dir")],
            heartbeat,
        )?,
    )?;
    let common_dir = PathBuf::from(output);
    let common_dir = if common_dir.is_absolute() {
        common_dir
    } else {
        path.join(common_dir)
    };
    Ok(CanonicalPath::resolve(common_dir)?)
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
    let mut command_args = Vec::with_capacity(args.len() + 2);
    command_args.push(arg("-C"));
    command_args.push(repository.as_os_str().to_owned());
    command_args.extend_from_slice(args);
    let operation = command_operation("git", &command_args);
    run_command("git", &command_args, &operation)
}

fn run_git_with_heartbeat<F>(
    repository: &Path,
    args: &[OsString],
    heartbeat: F,
) -> Result<String, GitError>
where
    F: FnMut() -> Result<(), GitError>,
{
    let mut command_args = Vec::with_capacity(args.len() + 2);
    command_args.push(arg("-C"));
    command_args.push(repository.as_os_str().to_owned());
    command_args.extend_from_slice(args);
    let operation = command_operation("git", &command_args);
    run_command_with_heartbeat(
        "git",
        &command_args,
        &operation,
        Duration::from_secs(1),
        heartbeat,
    )
}

fn run_command(program: &str, args: &[OsString], operation: &str) -> Result<String, GitError> {
    let output = Command::new(program)
        .args(args)
        .output()
        .context(IoSnafu { operation })?;
    if !output.status.success() {
        return Err(GitError::CommandFailed {
            operation: operation.to_owned(),
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    String::from_utf8(output.stdout).context(InvalidUtf8Snafu { operation })
}

fn run_command_with_heartbeat<F>(
    program: &str,
    args: &[OsString],
    operation: &str,
    poll_interval: Duration,
    mut heartbeat: F,
) -> Result<String, GitError>
where
    F: FnMut() -> Result<(), GitError>,
{
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(IoSnafu { operation })?;
    let mut last_heartbeat = Instant::now();

    loop {
        match child.try_wait().context(IoSnafu { operation })? {
            Some(_) => break,
            None => {
                if last_heartbeat.elapsed() >= poll_interval {
                    if let Err(error) = heartbeat() {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(error);
                    }
                    last_heartbeat = Instant::now();
                } else {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    let output = child.wait_with_output().context(IoSnafu { operation })?;
    if !output.status.success() {
        return Err(GitError::CommandFailed {
            operation: operation.to_owned(),
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    String::from_utf8(output.stdout).context(InvalidUtf8Snafu { operation })
}

fn command_operation(program: &str, args: &[OsString]) -> String {
    std::iter::once(program.to_owned())
        .chain(
            args.iter()
                .map(|value| value.to_string_lossy().into_owned()),
        )
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Snafu)]
pub enum GitError {
    #[snafu(display("failed to run git {operation}: {source}"))]
    Io {
        operation: String,
        source: std::io::Error,
    },
    #[snafu(display("git {operation} failed with status {status:?}: {stderr}"))]
    CommandFailed {
        operation: String,
        status: Option<i32>,
        stderr: String,
    },
    #[snafu(display("git {operation} returned invalid UTF-8"))]
    InvalidUtf8 {
        operation: String,
        source: std::string::FromUtf8Error,
    },
    #[snafu(display("git {operation} returned invalid output: {output:?}"))]
    InvalidOutput { operation: String, output: String },
    #[snafu(display("Git heartbeat failed: {message}"))]
    Heartbeat { message: String },
    #[snafu(display("Git heartbeat failed: {source}"))]
    HeartbeatFailure {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(transparent)]
    Canonicalize { source: CanonicalPathError },
    #[snafu(display("Git did not report worktree: {}", path.display()))]
    WorktreeNotFound { path: PathBuf },
}

impl GitError {
    pub(crate) fn from_heartbeat_source(
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        HeartbeatFailureSnafu.into_error(Box::new(source))
    }

    pub(crate) fn is_heartbeat(&self) -> bool {
        matches!(self, Self::Heartbeat { .. } | Self::HeartbeatFailure { .. })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;
    use std::time::Duration;

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

    #[test]
    fn resolves_the_primary_worktree_as_the_upstream_repository() {
        let (root, repository) = repository();
        let upstream_head = inspect_repository(&repository)
            .expect("upstream repository should be inspectable")
            .head;
        let linked_path = root.join("linked");
        add_detached_worktree(&repository, &linked_path).expect("worktree should be added");
        fs::write(linked_path.join("README"), "linked\n")
            .expect("linked worktree should be updated");
        run_git_in(&linked_path, &["commit", "-qam", "linked"]);

        let from_upstream =
            inspect_upstream_repository(&repository).expect("upstream input should resolve");
        let from_linked = inspect_upstream_repository(
            &CanonicalPath::resolve(&linked_path).expect("linked worktree should resolve"),
        )
        .expect("linked input should resolve");

        assert_eq!(from_upstream.root, repository);
        assert_eq!(from_linked.root, repository);
        assert_eq!(from_upstream.head, upstream_head);
        assert_eq!(from_linked.head, upstream_head);
        assert_eq!(from_linked.common_dir, from_upstream.common_dir);

        remove_worktree(&repository, &linked_path).expect("worktree should be removed");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn creates_a_detached_worktree_at_an_explicit_revision() {
        let (root, repository) = repository();
        let initial_head = inspect_repository(&repository)
            .expect("repository should be inspectable")
            .head;
        fs::write(root.join("README"), "updated\n").expect("test file should be updated");
        run_git_in(&root, &["commit", "-qam", "update"]);
        let worktree_path = repository.as_path().join("workspace");

        add_detached_worktree_at_with_heartbeat(&repository, &worktree_path, &initial_head, || {
            Ok(())
        })
        .expect("worktree should be added at the requested revision");

        let worktree =
            find_worktree(&repository, &worktree_path).expect("worktree should be registered");
        assert_eq!(worktree.head.as_deref(), Some(initial_head.as_str()));
        assert!(worktree.detached);

        remove_worktree(&repository, &worktree_path).expect("worktree should be removed");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn removes_a_clean_worktree_without_force() {
        let (root, repository) = repository();
        let worktree_path = root.join("workspace");
        add_detached_worktree(&repository, &worktree_path).expect("worktree should be added");

        remove_clean_worktree(&repository, &worktree_path)
            .expect("clean worktree should be removed without force");
        assert!(!worktree_path.exists());

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn polls_a_long_running_command_and_invokes_heartbeat() {
        let mut heartbeats = 0;
        run_command_with_heartbeat(
            "sleep",
            &[OsString::from("0.15")],
            "sleep 0.15",
            Duration::from_millis(20),
            || {
                heartbeats += 1;
                Ok(())
            },
        )
        .expect("long-running command should complete");
        assert!(heartbeats >= 2);
    }

    #[test]
    fn stops_waiting_when_heartbeat_fails() {
        let error = run_command_with_heartbeat(
            "sleep",
            &[OsString::from("5")],
            "sleep 5",
            Duration::from_millis(20),
            || {
                Err(GitError::Heartbeat {
                    message: "lease lost".to_owned(),
                })
            },
        )
        .expect_err("heartbeat failure should stop the command");
        assert!(matches!(error, GitError::Heartbeat { message } if message == "lease lost"));
    }

    #[test]
    fn observation_fingerprints_round_trip_and_compare_typed_fields() {
        let (root, repository) = repository();
        let repository_identity =
            inspect_repository_identity(&repository).expect("repository identity should exist");
        let worktree_identity = CanonicalPath::from_absolute(root.join("workspace"))
            .expect("worktree identity should be absolute");
        let fingerprint = ObservationFingerprint::from_worktree(
            repository_identity.clone(),
            WorktreeInfo {
                path: worktree_identity.clone(),
                head: Some("abc123".to_owned()),
                branch: None,
                detached: true,
                bare: false,
                prunable: None,
            },
            true,
        );
        let encoded = serde_json::to_string(&fingerprint).expect("fingerprint should serialize");
        let decoded: ObservationFingerprint =
            serde_json::from_str(&encoded).expect("fingerprint should deserialize");
        assert_eq!(decoded, fingerprint);
        assert!(fingerprint.matches_attached(
            &repository_identity,
            &worktree_identity,
            Some("abc123")
        ));

        let mut changed = fingerprint.clone();
        changed.branch = Some("refs/heads/feature".to_owned());
        assert_ne!(changed, fingerprint);

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn detects_clean_and_dirty_worktrees_without_mutating_them() {
        let (root, repository) = repository();
        let worktree_path = root.join("workspace");
        add_detached_worktree(&repository, &worktree_path).expect("worktree should be added");

        assert!(is_worktree_clean(&worktree_path).expect("status should succeed"));
        fs::write(worktree_path.join("untracked"), "change\n")
            .expect("untracked file should be written");
        assert!(!is_worktree_clean(&worktree_path).expect("status should succeed"));

        remove_worktree(&repository, &worktree_path).expect("worktree should be removed");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn checks_out_a_revision_in_detached_mode() {
        let (root, repository) = repository();
        let initial_head = inspect_repository(&repository)
            .expect("repository should be inspectable")
            .head;
        fs::write(root.join("README"), "updated\n").expect("test file should be updated");
        run_git_in(&root, &["commit", "-qam", "update"]);
        let updated_head = inspect_repository(&repository)
            .expect("repository should be inspectable")
            .head;
        let worktree_path = repository.as_path().join("workspace");
        add_detached_worktree(&repository, &worktree_path).expect("worktree should be added");
        run_git_in(
            &worktree_path,
            &["checkout", "-q", "-b", "feature", &initial_head],
        );

        checkout_detached_with_heartbeat(&worktree_path, &updated_head, || Ok(()))
            .expect("worktree should align to the repository head");

        let worktree =
            find_worktree(&repository, &worktree_path).expect("worktree should remain registered");
        assert!(worktree.detached);
        assert!(worktree.branch.is_none());
        assert_eq!(worktree.head.as_deref(), Some(updated_head.as_str()));
        assert!(is_worktree_clean(&worktree_path).expect("worktree status should succeed"));

        remove_worktree(&repository, &worktree_path).expect("worktree should be removed");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn refuses_to_overwrite_an_ignored_file_during_alignment() {
        let (root, repository) = repository();
        fs::write(root.join(".gitignore"), "generated\n").expect("ignore file should be written");
        run_git_in(&root, &["add", ".gitignore"]);
        run_git_in(&root, &["commit", "-qm", "ignore generated file"]);
        let base_head = inspect_repository(&repository)
            .expect("repository should be inspectable")
            .head;
        fs::write(root.join("generated"), "upstream\n").expect("upstream file should be written");
        run_git_in(&root, &["add", "-f", "generated"]);
        run_git_in(&root, &["commit", "-qm", "track generated file"]);
        let updated_head = inspect_repository(&repository)
            .expect("repository should be inspectable")
            .head;
        let worktree_path = repository.as_path().join("workspace");
        add_detached_worktree_at_with_heartbeat(&repository, &worktree_path, &base_head, || Ok(()))
            .expect("worktree should be added at the base revision");
        fs::write(worktree_path.join("generated"), "local\n")
            .expect("ignored file should be written");

        checkout_detached_with_heartbeat(&worktree_path, &updated_head, || Ok(()))
            .expect_err("alignment should preserve the ignored file");

        assert_eq!(
            fs::read_to_string(worktree_path.join("generated"))
                .expect("ignored file should remain readable"),
            "local\n"
        );
        let worktree =
            find_worktree(&repository, &worktree_path).expect("worktree should remain registered");
        assert_eq!(worktree.head.as_deref(), Some(base_head.as_str()));

        remove_worktree(&repository, &worktree_path).expect("worktree should be removed");
        fs::remove_dir_all(root).expect("test root should be removable");
    }
}
