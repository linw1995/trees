use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use diesel::sqlite::SqliteConnection;

use crate::domain::{
    CanonicalPath, JsonDocument, OperationId, Timestamp, WorkspaceId, WorkspaceState,
};
use crate::git;
use crate::reconciliation;
use crate::storage::{
    begin_operation, find_running_operation, find_workspace, find_workspace_claim,
    list_automatic_workspaces, list_repo_worktrees, record_workspace_gc_failure,
    record_workspace_gc_skipped, record_workspace_reclaimed, OperationIntent, OperationIntentError,
    RepoWorktreeRow,
};
use crate::validation;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct GcDuration {
    seconds: i64,
}

impl GcDuration {
    pub const fn seconds(self) -> i64 {
        self.seconds
    }
}

impl FromStr for GcDuration {
    type Err = GcDurationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err(GcDurationError::new("duration must not be empty"));
        }

        let mut total = 0_i64;
        let mut index = 0;
        while index < value.len() {
            let number_start = index;
            while value.as_bytes()[index].is_ascii_digit() {
                index += 1;
                if index == value.len() {
                    break;
                }
            }
            if number_start == index || index == value.len() {
                return Err(GcDurationError::new(
                    "duration components require a number and a unit",
                ));
            }
            let number = value[number_start..index]
                .parse::<i64>()
                .map_err(|_| GcDurationError::new("duration number is out of range"))?;
            let unit = value.as_bytes()[index] as char;
            index += 1;
            let multiplier = match unit {
                's' => 1,
                'm' => 60,
                'h' => 60 * 60,
                'd' => 24 * 60 * 60,
                'w' => 7 * 24 * 60 * 60,
                _ => {
                    return Err(GcDurationError::new(
                        "duration units must be s, m, h, d, or w",
                    ));
                }
            };
            total = total
                .checked_add(
                    number
                        .checked_mul(multiplier)
                        .ok_or_else(|| GcDurationError::new("duration is out of range"))?,
                )
                .ok_or_else(|| GcDurationError::new("duration is out of range"))?;
        }
        if total <= 0 {
            return Err(GcDurationError::new("duration must be greater than zero"));
        }
        Ok(Self { seconds: total })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GcDurationError {
    message: String,
}

impl GcDurationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for GcDurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GcDurationError {}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct GcCounts {
    pub automatic: usize,
    pub not_checked_out: usize,
    pub checked_out: usize,
    pub age_eligible: usize,
    pub safe_to_reclaim: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GcCandidateReason {
    Eligible,
    Young,
    CheckedOut,
    ActiveOperation,
    Unhealthy,
    UnsafeRoot,
    RepositoryIdentity,
    WorktreeIdentity,
    WorktreeMismatch,
    UnexpectedContent,
    GitError,
}

impl fmt::Display for GcCandidateReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Eligible => "eligible",
            Self::Young => "young",
            Self::CheckedOut => "checked_out",
            Self::ActiveOperation => "active_operation",
            Self::Unhealthy => "unhealthy",
            Self::UnsafeRoot => "unsafe_root",
            Self::RepositoryIdentity => "repository_identity",
            Self::WorktreeIdentity => "worktree_identity",
            Self::WorktreeMismatch => "worktree_mismatch",
            Self::UnexpectedContent => "unexpected_content",
            Self::GitError => "git_error",
        };
        formatter.write_str(value)
    }
}

#[derive(Debug, Clone)]
pub struct GcCandidate {
    pub workspace: crate::storage::WorkspaceRow,
    pub idle_since: Timestamp,
    pub age_eligible: bool,
    pub checked_out: bool,
    pub active_operation: bool,
}

impl GcCandidate {
    pub fn reason(&self) -> GcCandidateReason {
        if !self.age_eligible {
            GcCandidateReason::Young
        } else if self.checked_out {
            GcCandidateReason::CheckedOut
        } else if self.active_operation {
            GcCandidateReason::ActiveOperation
        } else if self.workspace.state != WorkspaceState::Ready {
            GcCandidateReason::Unhealthy
        } else {
            GcCandidateReason::Eligible
        }
    }
}

#[derive(Debug, Clone)]
pub struct GcScan {
    pub cutoff: Timestamp,
    pub counts: GcCounts,
    pub candidates: Vec<GcCandidate>,
}

impl GcScan {
    pub fn execution_candidate_count(&self, force: bool) -> usize {
        self.candidates
            .iter()
            .filter(|candidate| {
                execution_skip_reason(candidate, force) == Some(GcCandidateReason::Eligible)
            })
            .count()
    }
}

#[derive(Debug, Clone)]
pub struct GcSkipped {
    pub workspace_path: CanonicalPath,
    pub reason: GcCandidateReason,
}

#[derive(Debug, Clone)]
pub struct GcFailure {
    pub workspace_path: CanonicalPath,
    pub error: String,
}

#[derive(Debug)]
pub struct GcExecutionReport {
    pub scan: GcScan,
    pub reclaimed: Vec<CanonicalPath>,
    pub skipped: Vec<GcSkipped>,
    pub failed: Vec<GcFailure>,
}

pub fn scan(
    connection: &mut SqliteConnection,
    workspace_root: &CanonicalPath,
    older_than: GcDuration,
) -> Result<GcScan, GcError> {
    let cutoff = Timestamp::before_seconds(older_than.seconds());
    let workspaces =
        list_automatic_workspaces(connection, workspace_root).map_err(GcError::Database)?;
    let mut counts = GcCounts {
        automatic: workspaces.len(),
        ..GcCounts::default()
    };
    let mut candidates = Vec::with_capacity(workspaces.len());
    for workspace in workspaces {
        let claim = find_workspace_claim(connection, &workspace.id).map_err(GcError::Database)?;
        let checked_out = claim.is_some();
        if checked_out {
            counts.checked_out += 1;
        } else {
            counts.not_checked_out += 1;
        }
        let idle_since = workspace
            .last_checked_in_at
            .clone()
            .unwrap_or_else(|| workspace.created_at.clone());
        let age_eligible = idle_since < cutoff;
        if age_eligible {
            counts.age_eligible += 1;
        }
        let active_operation = find_running_operation(connection, &workspace.id)
            .map_err(GcError::Database)?
            .is_some();
        let candidate = GcCandidate {
            workspace,
            idle_since,
            age_eligible,
            checked_out,
            active_operation,
        };
        if candidate.reason() == GcCandidateReason::Eligible {
            counts.safe_to_reclaim += 1;
        }
        candidates.push(candidate);
    }
    Ok(GcScan {
        cutoff,
        counts,
        candidates,
    })
}

pub fn execute(
    connection: &mut SqliteConnection,
    workspace_root: &CanonicalPath,
    older_than: GcDuration,
    force: bool,
) -> Result<GcExecutionReport, GcError> {
    let scan = scan(connection, workspace_root, older_than)?;
    let mut report = GcExecutionReport {
        scan: scan.clone(),
        reclaimed: Vec::new(),
        skipped: Vec::new(),
        failed: Vec::new(),
    };
    for candidate in scan.candidates.iter().cloned() {
        let Some(reason) = execution_skip_reason(&candidate, force) else {
            continue;
        };
        if reason != GcCandidateReason::Eligible {
            report.skipped.push(GcSkipped {
                workspace_path: candidate.workspace.canonical_path,
                reason,
            });
            continue;
        }

        let Some(operation) = begin_gc_operation(connection, &candidate, &scan, force)? else {
            report.skipped.push(GcSkipped {
                workspace_path: candidate.workspace.canonical_path,
                reason: GcCandidateReason::ActiveOperation,
            });
            continue;
        };
        let workspace_id = candidate.workspace.id;
        let details_json = gc_details(&candidate.workspace.canonical_path, &scan, force, None);
        if let Err(error) =
            reconciliation::reconcile_workspace(connection, &workspace_id, &operation.id)
        {
            let error_text = error.to_string();
            finish_gc_failure(
                connection,
                &operation.id,
                &workspace_id,
                details_json,
                &error_text,
            )?;
            report.failed.push(GcFailure {
                workspace_path: candidate.workspace.canonical_path,
                error: error_text,
            });
            break;
        }

        let workspace = find_workspace(connection, &workspace_id).map_err(GcError::Database)?;
        let claim = find_workspace_claim(connection, &workspace_id).map_err(GcError::Database)?;
        if claim.is_some() {
            finish_gc_skip(
                connection,
                &operation.id,
                &workspace_id,
                details_json,
                GcCandidateReason::CheckedOut,
            )?;
            report.skipped.push(GcSkipped {
                workspace_path: workspace.canonical_path,
                reason: GcCandidateReason::CheckedOut,
            });
            continue;
        }
        if workspace.state == WorkspaceState::Reclaimed
            || (!force && workspace.state != WorkspaceState::Ready)
        {
            finish_gc_skip(
                connection,
                &operation.id,
                &workspace_id,
                details_json,
                GcCandidateReason::Unhealthy,
            )?;
            report.skipped.push(GcSkipped {
                workspace_path: workspace.canonical_path,
                reason: GcCandidateReason::Unhealthy,
            });
            continue;
        }

        let repositories =
            list_repo_worktrees(connection, &workspace_id).map_err(GcError::Database)?;
        let removal_plan = match prepare_removal(&workspace, &repositories, workspace_root, force) {
            Ok(plan) => plan,
            Err(reason) => {
                finish_gc_skip(
                    connection,
                    &operation.id,
                    &workspace_id,
                    details_json,
                    reason,
                )?;
                report.skipped.push(GcSkipped {
                    workspace_path: workspace.canonical_path,
                    reason,
                });
                continue;
            }
        };
        if let Err(error) = remove_physical_workspace(&removal_plan, force) {
            let error_text = error.to_string();
            let _ = reconciliation::reconcile_workspace(connection, &workspace_id, &operation.id);
            finish_gc_failure(
                connection,
                &operation.id,
                &workspace_id,
                gc_details(&workspace.canonical_path, &scan, force, Some(&error_text)),
                &error_text,
            )?;
            report.failed.push(GcFailure {
                workspace_path: workspace.canonical_path,
                error: error_text,
            });
            break;
        }

        if let Err(error) = record_workspace_reclaimed(
            connection,
            &operation.id,
            &workspace_id,
            Some(gc_details(&workspace.canonical_path, &scan, force, None)),
        ) {
            let error_text = error.to_string();
            finish_gc_failure(
                connection,
                &operation.id,
                &workspace_id,
                gc_details(&workspace.canonical_path, &scan, force, Some(&error_text)),
                &error_text,
            )?;
            report.failed.push(GcFailure {
                workspace_path: workspace.canonical_path,
                error: error_text,
            });
            break;
        }
        report.reclaimed.push(workspace.canonical_path);
    }
    Ok(report)
}

fn execution_skip_reason(candidate: &GcCandidate, force: bool) -> Option<GcCandidateReason> {
    if !candidate.age_eligible {
        return Some(GcCandidateReason::Young);
    }
    if candidate.checked_out {
        return Some(GcCandidateReason::CheckedOut);
    }
    if candidate.active_operation {
        return Some(GcCandidateReason::ActiveOperation);
    }
    if candidate.workspace.state == WorkspaceState::Reclaimed {
        return Some(GcCandidateReason::Unhealthy);
    }
    if !force && candidate.workspace.state != WorkspaceState::Ready {
        return Some(GcCandidateReason::Unhealthy);
    }
    Some(GcCandidateReason::Eligible)
}

fn begin_gc_operation(
    connection: &mut SqliteConnection,
    candidate: &GcCandidate,
    scan: &GcScan,
    force: bool,
) -> Result<Option<crate::storage::OperationRow>, GcError> {
    let intent_json = gc_details(&candidate.workspace.canonical_path, scan, force, None);
    let intent = OperationIntent::new(
        candidate.workspace.id,
        "gc",
        format!("process:{}", std::process::id()),
        Timestamp::after_seconds(300),
        "reclaim workspace",
        intent_json,
    );
    match begin_operation(connection, &intent) {
        Ok(operation) => Ok(Some(operation)),
        Err(OperationIntentError::WorkspaceBusy(_)) => Ok(None),
        Err(OperationIntentError::Database(error)) => Err(GcError::Database(error)),
    }
}

fn gc_details(
    workspace_path: &CanonicalPath,
    scan: &GcScan,
    force: bool,
    error: Option<&str>,
) -> JsonDocument {
    JsonDocument::from_serializable(&serde_json::json!({
        "workspace_path": workspace_path,
        "cutoff": scan.cutoff,
        "forced": force,
        "counts": {
            "automatic": scan.counts.automatic,
            "not_checked_out": scan.counts.not_checked_out,
            "checked_out": scan.counts.checked_out,
            "age_eligible": scan.counts.age_eligible,
            "safe_to_reclaim": scan.counts.safe_to_reclaim,
        },
        "error": error,
    }))
    .expect("GC details should serialize")
}

fn finish_gc_skip(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
    workspace_id: &WorkspaceId,
    details_json: JsonDocument,
    reason: GcCandidateReason,
) -> Result<(), GcError> {
    let error_json = JsonDocument::from_serializable(&serde_json::json!({
        "reason": reason.to_string(),
    }))
    .expect("GC skip error should serialize");
    record_workspace_gc_skipped(
        connection,
        operation_id,
        workspace_id,
        Some(details_json),
        error_json,
    )
    .map_err(GcError::Database)
}

fn finish_gc_failure(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
    workspace_id: &WorkspaceId,
    details_json: JsonDocument,
    error: &str,
) -> Result<(), GcError> {
    let error_json = JsonDocument::from_serializable(&serde_json::json!({
        "error": error,
    }))
    .expect("GC error should serialize");
    record_workspace_gc_failure(
        connection,
        operation_id,
        workspace_id,
        Some(details_json),
        error_json,
    )
    .map_err(GcError::Database)
}

struct RemovalPlan {
    workspace_path: PathBuf,
    worktrees: Vec<WorktreeRemoval>,
    extra_entries: Vec<PathBuf>,
}

struct WorktreeRemoval {
    repository: CanonicalPath,
    path: PathBuf,
    remove_from_git: bool,
}

fn prepare_removal(
    workspace: &crate::storage::WorkspaceRow,
    repositories: &[RepoWorktreeRow],
    workspace_root: &CanonicalPath,
    force: bool,
) -> Result<RemovalPlan, GcCandidateReason> {
    if workspace.canonical_path.as_path().parent() != Some(workspace_root.as_path()) {
        return Err(GcCandidateReason::UnsafeRoot);
    }
    for repository in repositories {
        if repository.worktree_path.as_path().parent() != Some(workspace.canonical_path.as_path())
            || !repository
                .worktree_path
                .as_path()
                .starts_with(workspace_root.as_path())
        {
            return Err(GcCandidateReason::UnsafeRoot);
        }
        let actual_identity = git::inspect_repository_identity(&repository.source_path)
            .map_err(|_| GcCandidateReason::RepositoryIdentity)?;
        if actual_identity != repository.repository_identity {
            return Err(GcCandidateReason::RepositoryIdentity);
        }
    }
    let workspace_exists = match fs::symlink_metadata(workspace.canonical_path.as_path()) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => true,
        Ok(_) => return Err(GcCandidateReason::UnsafeRoot),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Err(GcCandidateReason::UnsafeRoot),
    };
    if !workspace_exists {
        if force {
            return Ok(RemovalPlan {
                workspace_path: workspace.canonical_path.as_path().to_owned(),
                worktrees: Vec::new(),
                extra_entries: Vec::new(),
            });
        }
        return Err(GcCandidateReason::WorktreeMismatch);
    }

    let expected_paths = repositories
        .iter()
        .map(|repository| repository.worktree_path.as_path().to_owned())
        .collect::<Vec<_>>();
    let extra_entries = fs::read_dir(workspace.canonical_path.as_path())
        .map_err(|_| GcCandidateReason::UnexpectedContent)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| GcCandidateReason::UnexpectedContent)?
        .into_iter()
        .filter(|entry| !expected_paths.iter().any(|expected| expected == entry))
        .collect::<Vec<_>>();
    if !force && !extra_entries.is_empty() {
        return Err(GcCandidateReason::UnexpectedContent);
    }
    if !force {
        validation::validate_workspace_root(
            workspace.canonical_path.as_path(),
            workspace_root.as_path(),
            &expected_paths,
        )
        .map_err(|error| match error {
            validation::WorkspaceRootError::UnexpectedEntry(_) => {
                GcCandidateReason::UnexpectedContent
            }
            validation::WorkspaceRootError::NotDirectory(_)
            | validation::WorkspaceRootError::OutsideManagedRoot { .. }
            | validation::WorkspaceRootError::NotAbsolute { .. } => GcCandidateReason::UnsafeRoot,
            validation::WorkspaceRootError::ReadDirectory { .. } => {
                GcCandidateReason::UnexpectedContent
            }
        })?;
    }

    let mut worktrees = Vec::with_capacity(repositories.len());
    for repository in repositories {
        let listed = git::list_worktrees(&repository.source_path)
            .map_err(|_| GcCandidateReason::GitError)?
            .into_iter()
            .find(|worktree| worktree.path.as_path() == repository.worktree_path.as_path());
        let worktree_exists = match fs::symlink_metadata(repository.worktree_path.as_path()) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(GcCandidateReason::WorktreeIdentity);
            }
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => return Err(GcCandidateReason::WorktreeMismatch),
        };
        if !force {
            let Some(worktree) = listed.as_ref() else {
                return Err(GcCandidateReason::WorktreeMismatch);
            };
            if worktree.prunable.is_some() || !worktree_exists {
                return Err(GcCandidateReason::WorktreeMismatch);
            }
            let identity = git::inspect_worktree_identity(repository.worktree_path.as_path())
                .map_err(|_| GcCandidateReason::WorktreeIdentity)?;
            if identity != repository.repository_identity {
                return Err(GcCandidateReason::WorktreeIdentity);
            }
            if !worktree.detached
                || worktree.branch.is_some()
                || worktree.head.as_deref() != repository.last_head.as_deref()
                || !git::is_worktree_clean(repository.worktree_path.as_path())
                    .map_err(|_| GcCandidateReason::GitError)?
            {
                return Err(GcCandidateReason::WorktreeMismatch);
            }
        }
        let Some(worktree) = listed else {
            if worktree_exists {
                worktrees.push(WorktreeRemoval {
                    repository: repository.source_path.clone(),
                    path: repository.worktree_path.as_path().to_owned(),
                    remove_from_git: false,
                });
            }
            continue;
        };
        if force && worktree_exists {
            let identity = git::inspect_worktree_identity(repository.worktree_path.as_path())
                .map_err(|_| GcCandidateReason::WorktreeIdentity)?;
            if identity != repository.repository_identity {
                return Err(GcCandidateReason::WorktreeIdentity);
            }
        }
        worktrees.push(WorktreeRemoval {
            repository: repository.source_path.clone(),
            path: worktree.path.into_path_buf(),
            remove_from_git: true,
        });
    }
    Ok(RemovalPlan {
        workspace_path: workspace.canonical_path.as_path().to_owned(),
        worktrees,
        extra_entries: if force { extra_entries } else { Vec::new() },
    })
}

fn remove_physical_workspace(plan: &RemovalPlan, force: bool) -> Result<(), GcPhysicalError> {
    for worktree in &plan.worktrees {
        if worktree.remove_from_git {
            if force {
                git::remove_worktree(&worktree.repository, &worktree.path)
                    .map_err(GcPhysicalError::Git)?;
            } else {
                git::remove_clean_worktree(&worktree.repository, &worktree.path)
                    .map_err(GcPhysicalError::Git)?;
            }
        } else if worktree.path.exists() {
            remove_path(&worktree.path)?;
        }
    }
    for entry in &plan.extra_entries {
        remove_path(entry)?;
    }
    if plan.workspace_path.exists() {
        fs::remove_dir(&plan.workspace_path).map_err(|source| GcPhysicalError::Io {
            path: plan.workspace_path.clone(),
            source,
        })?;
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<(), GcPhysicalError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| GcPhysicalError::Io {
        path: path.to_owned(),
        source,
    })?;
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path).map_err(|source| GcPhysicalError::Io {
            path: path.to_owned(),
            source,
        })?;
    } else {
        fs::remove_file(path).map_err(|source| GcPhysicalError::Io {
            path: path.to_owned(),
            source,
        })?;
    }
    Ok(())
}

#[derive(Debug)]
enum GcPhysicalError {
    Git(crate::git::GitError),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for GcPhysicalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Git(error) => error.fmt(formatter),
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "GC filesystem operation failed for {}: {source}",
                    path.display()
                )
            }
        }
    }
}

#[derive(Debug)]
pub enum GcError {
    Database(diesel::result::Error),
}

impl fmt::Display for GcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "GC database operation failed: {error}"),
        }
    }
}

impl std::error::Error for GcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    use diesel::prelude::*;

    use super::*;
    use crate::claim::WorkspaceClaim;
    use crate::database;
    use crate::domain::{WorkspaceId, WorkspaceManagementMode};
    use crate::pool::RepositorySetKey;
    use crate::storage::{
        ensure_workspace_pool, insert_managed_workspace, insert_workspace_claim,
        NewManagedWorkspace, NewWorkspaceClaim,
    };
    use crate::workspace::{
        prepare_automatic, provision_automatic, release_automatic_workspace, AutomaticCreateRequest,
    };

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("trees-gc-{}", WorkspaceId::new()))
    }

    fn timestamp(value: &str) -> Timestamp {
        Timestamp::parse(value).expect("timestamp should be valid")
    }

    fn run_git(path: &std::path::Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .expect("git should run");
        assert!(output.status.success());
    }

    fn repository(path: &std::path::Path) {
        fs::create_dir_all(path).expect("repository should be created");
        run_git(path, &["init", "-q"]);
        run_git(path, &["config", "user.email", "trees@example.invalid"]);
        run_git(path, &["config", "user.name", "trees tests"]);
        fs::write(path.join("README"), "test\n").expect("test file should be written");
        run_git(path, &["add", "README"]);
        run_git(path, &["commit", "-qm", "initial"]);
    }

    fn automatic_workspace_fixture() -> (
        PathBuf,
        PathBuf,
        SqliteConnection,
        crate::storage::WorkspaceRow,
        CanonicalPath,
        PathBuf,
        CanonicalPath,
    ) {
        let root = test_root();
        let source_path = root.join("source");
        repository(&source_path);
        let database_path = root.join("state.sqlite");
        let mut connection = database::connect(&database_path).expect("database should open");
        let mut plan = prepare_automatic(&AutomaticCreateRequest {
            repositories: vec![source_path],
        })
        .expect("automatic allocation plan should be prepared");
        plan.workspace_root = CanonicalPath::from_absolute(root.join("managed"))
            .expect("workspace root should be absolute");
        let checkout = provision_automatic(&mut connection, &plan)
            .expect("automatic workspace should be provisioned");
        release_automatic_workspace(&mut connection, &checkout.workspace_path, checkout.claim_id)
            .expect("workspace should be checked in");
        let mut workspace =
            crate::storage::find_workspace_by_path(&mut connection, &checkout.workspace_path)
                .expect("workspace lookup should succeed")
                .expect("workspace should exist");
        let old = timestamp("2020-01-01T00:00:00Z");
        diesel::update(crate::schema::workspaces::table.find(workspace.id))
            .set((
                crate::schema::workspaces::created_at.eq(old.clone()),
                crate::schema::workspaces::last_checked_in_at.eq(Some(old)),
            ))
            .execute(&mut connection)
            .expect("workspace should be aged");
        workspace = crate::storage::find_workspace(&mut connection, &workspace.id)
            .expect("workspace lookup should succeed");
        let worktree = crate::storage::list_repo_worktrees(&mut connection, &workspace.id)
            .expect("worktree lookup should succeed")
            .into_iter()
            .next()
            .expect("workspace should have a worktree");
        let source = worktree.source_path.clone();
        let workspace_root = crate::storage::find_workspace_pool_by_id(
            &mut connection,
            &workspace
                .pool_key
                .expect("automatic workspace should reference a pool"),
        )
        .expect("workspace pool lookup should succeed")
        .workspace_root;
        (
            root,
            database_path,
            connection,
            workspace,
            source,
            worktree.worktree_path.into_path_buf(),
            workspace_root,
        )
    }

    #[test]
    fn parses_compound_gc_durations() {
        assert_eq!(
            "1w2d3h4m5s".parse::<GcDuration>().unwrap().seconds(),
            788_645
        );
        assert!("0s".parse::<GcDuration>().is_err());
        assert!("1x".parse::<GcDuration>().is_err());
    }

    #[test]
    fn scans_idle_and_checked_out_automatic_workspaces_by_root() {
        let root = test_root();
        let database_path = root.join("state.sqlite");
        let workspace_root = CanonicalPath::from_absolute(root.join("managed"))
            .expect("workspace root should be absolute");
        fs::create_dir_all(&root).expect("GC test root should be created");
        let mut connection = database::connect(&database_path).expect("database should open");
        let old = timestamp("2020-01-01T00:00:00Z");
        let young = timestamp("2099-01-01T00:00:00Z");
        let pool_key = Some(
            ensure_workspace_pool(
                &mut connection,
                &workspace_root,
                &RepositorySetKey::from_repositories(&[CanonicalPath::from_absolute(
                    "/repo/example",
                )
                .expect("repository path should be absolute")]),
            )
            .expect("workspace pool should be available")
            .id,
        );
        let entries = [
            (WorkspaceState::Ready, old.clone(), None),
            (WorkspaceState::Ready, young, None),
            (WorkspaceState::Ready, old.clone(), Some("active")),
            (WorkspaceState::Degraded, old.clone(), None),
        ];
        let mut workspace_ids = Vec::new();
        for (state, idle_since, claim_kind) in entries {
            let id = WorkspaceId::new();
            workspace_ids.push((id, claim_kind));
            let workspace_path = CanonicalPath::from_absolute(root.join(id.to_string()))
                .expect("workspace path should be absolute");
            insert_managed_workspace(
                &mut connection,
                &NewManagedWorkspace {
                    id,
                    canonical_path: workspace_path,
                    state,
                    created_at: old.clone(),
                    updated_at: old.clone(),
                    last_reconciled_at: None,
                    management_mode: WorkspaceManagementMode::Automatic,
                    pool_key,
                    last_checked_in_at: Some(idle_since),
                    reclaimed_at: None,
                },
            )
            .expect("workspace should be inserted");
            if let Some(claim_kind) = claim_kind {
                let claim = WorkspaceClaim::new(id, claim_kind);
                insert_workspace_claim(&mut connection, &NewWorkspaceClaim::from(&claim))
                    .expect("claim should be inserted");
            }
        }
        let manual_id = WorkspaceId::new();
        insert_managed_workspace(
            &mut connection,
            &NewManagedWorkspace {
                id: manual_id,
                canonical_path: CanonicalPath::from_absolute(root.join("manual"))
                    .expect("workspace path should be absolute"),
                state: WorkspaceState::Ready,
                created_at: old.clone(),
                updated_at: old.clone(),
                last_reconciled_at: None,
                management_mode: WorkspaceManagementMode::Manual,
                pool_key: None,
                last_checked_in_at: Some(old.clone()),
                reclaimed_at: None,
            },
        )
        .expect("manual workspace should be inserted");

        let result = scan(
            &mut connection,
            &workspace_root,
            "30d".parse().expect("duration should parse"),
        )
        .expect("GC scan should succeed");
        assert_eq!(result.counts.automatic, 4);
        assert_eq!(result.counts.not_checked_out, 3);
        assert_eq!(result.counts.checked_out, 1);
        assert_eq!(result.counts.age_eligible, 3);
        assert_eq!(result.counts.safe_to_reclaim, 1);
        assert_eq!(workspace_ids.len(), 4);

        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("GC test root should be removable");
    }

    #[test]
    fn reclaims_an_old_clean_automatic_workspace() {
        let (root, database_path, mut connection, workspace, source, worktree, workspace_root) =
            automatic_workspace_fixture();

        let report = execute(
            &mut connection,
            &workspace_root,
            "30d".parse().expect("duration should parse"),
            false,
        )
        .expect("GC execution should succeed");
        assert_eq!(
            report.reclaimed.as_slice(),
            std::slice::from_ref(&workspace.canonical_path)
        );
        assert!(report.skipped.is_empty());
        assert!(report.failed.is_empty());
        assert!(!workspace.canonical_path.as_path().exists());
        assert_eq!(
            crate::storage::find_workspace(&mut connection, &workspace.id)
                .expect("workspace lookup should succeed")
                .state,
            WorkspaceState::Reclaimed
        );
        assert_eq!(
            crate::storage::list_repo_worktrees(&mut connection, &workspace.id)
                .expect("worktree lookup should succeed")[0]
                .state,
            crate::domain::RepoWorktreeState::Reclaimed
        );
        let operation = crate::schema::operations::table
            .filter(crate::schema::operations::workspace_id.eq(workspace.id))
            .order(crate::schema::operations::started_at.desc())
            .select(crate::storage::OperationRow::as_select())
            .first(&mut connection)
            .expect("GC operation should exist");
        assert_eq!(operation.state, crate::domain::OperationState::Succeeded);
        assert_eq!(
            crate::git::list_worktrees(&source)
                .expect("source worktrees should be readable")
                .len(),
            1
        );
        assert!(!worktree.exists());

        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("GC test root should be removable");
    }

    #[test]
    fn force_reclaims_an_old_dirty_workspace_and_extra_content() {
        let (root, database_path, mut connection, workspace, source, worktree, workspace_root) =
            automatic_workspace_fixture();
        fs::write(worktree.join("local-change"), "dirty\n").expect("worktree should become dirty");
        fs::write(workspace.canonical_path.as_path().join("extra"), "extra\n")
            .expect("workspace should contain extra content");

        let report = execute(
            &mut connection,
            &workspace_root,
            "30d".parse().expect("duration should parse"),
            true,
        )
        .expect("forced GC execution should succeed");
        assert_eq!(
            report.reclaimed.as_slice(),
            std::slice::from_ref(&workspace.canonical_path)
        );
        assert!(report.skipped.is_empty());
        assert!(report.failed.is_empty());
        assert!(!workspace.canonical_path.as_path().exists());
        assert_eq!(
            crate::storage::find_workspace(&mut connection, &workspace.id)
                .expect("workspace lookup should succeed")
                .state,
            WorkspaceState::Reclaimed
        );
        assert_eq!(
            crate::git::list_worktrees(&source)
                .expect("source worktrees should be readable")
                .len(),
            1
        );

        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("GC test root should be removable");
    }

    #[test]
    fn normal_gc_skips_a_workspace_that_becomes_dirty_after_scan() {
        let (root, database_path, mut connection, workspace, source, worktree, workspace_root) =
            automatic_workspace_fixture();
        fs::write(worktree.join("local-change"), "dirty\n").expect("worktree should become dirty");

        let report = execute(
            &mut connection,
            &workspace_root,
            "30d".parse().expect("duration should parse"),
            false,
        )
        .expect("GC execution should complete with a skip");
        assert!(report.reclaimed.is_empty());
        assert!(report.failed.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].reason, GcCandidateReason::Unhealthy);
        assert!(workspace.canonical_path.as_path().exists());
        assert_eq!(
            crate::storage::find_workspace(&mut connection, &workspace.id)
                .expect("workspace lookup should succeed")
                .state,
            WorkspaceState::Degraded
        );
        assert_eq!(
            crate::git::list_worktrees(&source)
                .expect("source worktrees should be readable")
                .len(),
            2
        );

        crate::git::remove_worktree(&source, &worktree)
            .expect("dirty test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("GC test root should be removable");
    }
}
