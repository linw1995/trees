use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use diesel::sqlite::SqliteConnection;
use snafu::{OptionExt, ResultExt, Snafu};

use crate::domain::{CanonicalPath, JsonDocument, Timestamp, WorkspaceId, WorkspaceState};
use crate::git;
use crate::reconciliation;
use crate::storage::{
    begin_operation, find_running_operation, find_workspace, find_workspace_claim,
    list_automatic_workspaces, list_repo_worktrees, record_workspace_explicitly_removed,
    record_workspace_gc_failure, record_workspace_gc_skipped, record_workspace_reclaimed,
    record_workspace_remove_failure, record_workspace_remove_skipped, renew_operation_lease,
    OperationIntent, OperationIntentError, RepoWorktreeRow,
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
            return EmptySnafu.fail();
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
                return ComponentSnafu.fail();
            }
            let number = value[number_start..index]
                .parse::<i64>()
                .context(NumberSnafu)?;
            let unit = value.as_bytes()[index] as char;
            index += 1;
            let multiplier = match unit {
                's' => 1,
                'm' => 60,
                'h' => 60 * 60,
                'd' => 24 * 60 * 60,
                'w' => 7 * 24 * 60 * 60,
                _ => {
                    return UnitSnafu.fail();
                }
            };
            total = total
                .checked_add(number.checked_mul(multiplier).context(RangeSnafu)?)
                .context(RangeSnafu)?;
        }
        if total <= 0 {
            return NonPositiveSnafu.fail();
        }
        Ok(Self { seconds: total })
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Snafu)]
pub enum GcDurationError {
    #[snafu(display("duration must not be empty"))]
    Empty,
    #[snafu(display("duration components require a number and a unit"))]
    Component,
    #[snafu(display("duration number is out of range"))]
    Number { source: std::num::ParseIntError },
    #[snafu(display("duration units must be s, m, h, d, or w"))]
    Unit,
    #[snafu(display("duration is out of range"))]
    Range,
    #[snafu(display("duration must be greater than zero"))]
    NonPositive,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct GcCounts {
    pub automatic: usize,
    pub unclaimed: usize,
    pub claimed: usize,
    pub age_eligible: usize,
    pub safe_to_reclaim: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GcCandidateReason {
    Eligible,
    Young,
    Claimed,
    ActiveOperation,
    ExpiredOperation,
    Reclaimed,
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
            Self::Claimed => "claimed",
            Self::ActiveOperation => "active_operation",
            Self::ExpiredOperation => "expired_operation",
            Self::Reclaimed => "reclaimed",
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
    pub claimed: bool,
    pub active_operation: bool,
    pub physical_reason: Option<GcCandidateReason>,
    pub operation_expired: bool,
}

impl GcCandidate {
    fn recoverable_expired_operation(&self, force: bool) -> bool {
        self.age_eligible
            && !self.claimed
            && self.active_operation
            && self.operation_expired
            && self.workspace.state != WorkspaceState::Reclaimed
            && (force || self.workspace.state == WorkspaceState::Ready)
    }

    fn database_reason(&self, force: bool) -> Option<GcCandidateReason> {
        if !self.age_eligible {
            Some(GcCandidateReason::Young)
        } else if self.claimed {
            Some(GcCandidateReason::Claimed)
        } else if self.active_operation {
            Some(GcCandidateReason::ActiveOperation)
        } else if self.workspace.state == WorkspaceState::Reclaimed
            || (!force && self.workspace.state != WorkspaceState::Ready)
        {
            Some(GcCandidateReason::Unhealthy)
        } else {
            None
        }
    }

    pub fn reason(&self, force: bool) -> GcCandidateReason {
        self.database_reason(force)
            .or(self.physical_reason)
            .unwrap_or(GcCandidateReason::Eligible)
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
                candidate.reason(force) == GcCandidateReason::Eligible
                    || candidate.recoverable_expired_operation(force)
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

#[derive(Debug, Clone)]
pub struct RemovalPreflight {
    pub workspace: crate::storage::WorkspaceRow,
    pub reason: GcCandidateReason,
}

impl RemovalPreflight {
    pub fn can_execute(&self) -> bool {
        matches!(
            self.reason,
            GcCandidateReason::Eligible | GcCandidateReason::ExpiredOperation
        )
    }
}

#[derive(Debug)]
pub struct RemovalReport {
    pub workspace_path: CanonicalPath,
    pub removed: bool,
    pub reason: GcCandidateReason,
    pub error: Option<String>,
}

struct StartedRemoval {
    operation: crate::storage::OperationRow,
    lease_id: crate::domain::LeaseId,
}

enum PreparedRemoval {
    Ready {
        workspace: crate::storage::WorkspaceRow,
        plan: RemovalPlan,
    },
    Complete(RemovalReport),
}

enum GcCandidateResult {
    Reclaimed(CanonicalPath),
    Skipped(GcSkipped),
    Failed(GcFailure),
}

pub fn scan(connection: &mut SqliteConnection, older_than: GcDuration) -> Result<GcScan, GcError> {
    scan_with_force(connection, older_than, false)
}

pub fn scan_with_force(
    connection: &mut SqliteConnection,
    older_than: GcDuration,
    force: bool,
) -> Result<GcScan, GcError> {
    let cutoff = Timestamp::before_seconds(older_than.seconds());
    let workspaces = list_automatic_workspaces(connection).context(DatabaseSnafu)?;
    let mut counts = GcCounts {
        automatic: workspaces.len(),
        ..GcCounts::default()
    };
    let mut candidates = Vec::with_capacity(workspaces.len());
    for workspace in workspaces {
        let claim = find_workspace_claim(connection, &workspace.id).context(DatabaseSnafu)?;
        let claimed = claim.is_some();
        if claimed {
            counts.claimed += 1;
        } else {
            counts.unclaimed += 1;
        }
        let idle_since = workspace
            .last_released_at
            .clone()
            .unwrap_or_else(|| workspace.created_at.clone());
        let age_eligible = idle_since < cutoff;
        if age_eligible {
            counts.age_eligible += 1;
        }
        let running_operation =
            find_running_operation(connection, &workspace.id).context(DatabaseSnafu)?;
        let active_operation = running_operation.is_some();
        let operation_expired =
            running_operation.is_some_and(|running| running.lease.lease_expires_at.has_expired());
        let mut candidate = GcCandidate {
            workspace,
            idle_since,
            age_eligible,
            claimed,
            active_operation,
            physical_reason: None,
            operation_expired,
        };
        if candidate.database_reason(force).is_none() {
            let repositories =
                list_repo_worktrees(connection, &candidate.workspace.id).context(DatabaseSnafu)?;
            candidate.physical_reason =
                prepare_removal(&candidate.workspace, &repositories, force).err();
        }
        if candidate.reason(force) == GcCandidateReason::Eligible {
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
    older_than: GcDuration,
    force: bool,
) -> Result<GcExecutionReport, GcError> {
    let preflight = scan_with_force(connection, older_than, force)?;
    recover_expired_automatic_operations(connection, &preflight.cutoff, force)?;
    let scan = scan_with_force(connection, older_than, force)?;
    let mut report = GcExecutionReport {
        scan: scan.clone(),
        reclaimed: Vec::new(),
        skipped: Vec::new(),
        failed: Vec::new(),
    };
    for candidate in &scan.candidates {
        match execute_candidate(connection, candidate, &scan, force)? {
            GcCandidateResult::Reclaimed(path) => report.reclaimed.push(path),
            GcCandidateResult::Skipped(skipped) => report.skipped.push(skipped),
            GcCandidateResult::Failed(failure) => {
                report.failed.push(failure);
                break;
            }
        }
    }
    Ok(report)
}

pub fn scan_removal(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    force: bool,
) -> Result<RemovalPreflight, GcError> {
    let workspace = find_workspace(connection, workspace_id).map_err(|error| match error {
        diesel::result::Error::NotFound => GcError::WorkspaceNotFound {
            workspace_id: *workspace_id,
        },
        source => GcError::Database { source },
    })?;
    let reason = removal_reason(connection, &workspace, force)?;
    Ok(RemovalPreflight { workspace, reason })
}

fn removal_reason(
    connection: &mut SqliteConnection,
    workspace: &crate::storage::WorkspaceRow,
    force: bool,
) -> Result<GcCandidateReason, GcError> {
    if workspace.state == WorkspaceState::Reclaimed {
        return Ok(GcCandidateReason::Reclaimed);
    }
    if find_workspace_claim(connection, &workspace.id)
        .context(DatabaseSnafu)?
        .is_some()
    {
        return Ok(GcCandidateReason::Claimed);
    }
    if let Some(operation) =
        find_running_operation(connection, &workspace.id).context(DatabaseSnafu)?
    {
        return Ok(if operation.lease.lease_expires_at.has_expired() {
            GcCandidateReason::ExpiredOperation
        } else {
            GcCandidateReason::ActiveOperation
        });
    }
    if workspace_is_unhealthy(workspace, force) {
        return Ok(GcCandidateReason::Unhealthy);
    }
    let repositories = list_repo_worktrees(connection, &workspace.id).context(DatabaseSnafu)?;
    Ok(prepare_removal(workspace, &repositories, force)
        .err()
        .unwrap_or(GcCandidateReason::Eligible))
}

pub fn remove_workspace(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    force: bool,
) -> Result<RemovalReport, GcError> {
    let preflight = scan_removal(connection, workspace_id, force)?;
    let workspace_path = preflight.workspace.canonical_path.clone();
    if let Some(report) = admit_removal_preflight(connection, workspace_id, preflight)? {
        return Ok(report);
    }

    let refreshed = find_workspace(connection, workspace_id).context(DatabaseSnafu)?;
    if refreshed.state == WorkspaceState::Reclaimed {
        return Ok(removal_rejected(
            workspace_path,
            GcCandidateReason::Reclaimed,
        ));
    }
    let Some(started) = begin_removal_operation(connection, workspace_id, &refreshed, force)?
    else {
        return Ok(removal_rejected(
            workspace_path,
            GcCandidateReason::ActiveOperation,
        ));
    };
    if let Some(report) =
        reconcile_started_removal(connection, workspace_id, &workspace_path, &started, force)?
    {
        return Ok(report);
    }
    match prepare_started_removal(connection, workspace_id, &started.lease_id, force)? {
        PreparedRemoval::Ready { workspace, plan } => {
            execute_prepared_removal(connection, workspace_id, started, workspace, plan, force)
        }
        PreparedRemoval::Complete(report) => Ok(report),
    }
}

fn admit_removal_preflight(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    preflight: RemovalPreflight,
) -> Result<Option<RemovalReport>, GcError> {
    if preflight.reason == GcCandidateReason::ExpiredOperation {
        reconciliation::recover_expired_operation(connection, workspace_id)
            .context(ReconciliationSnafu)?;
        return Ok(None);
    }
    Ok((preflight.reason != GcCandidateReason::Eligible)
        .then(|| removal_rejected(preflight.workspace.canonical_path, preflight.reason)))
}

fn begin_removal_operation(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    workspace: &crate::storage::WorkspaceRow,
    force: bool,
) -> Result<Option<StartedRemoval>, GcError> {
    let details_json = removal_details(&workspace.canonical_path, force, None);
    let intent = OperationIntent::new(
        *workspace_id,
        "remove",
        Timestamp::after_seconds(300),
        "remove workspace",
        details_json.clone(),
    );
    let lease_id = intent.lease_id;
    match begin_operation(connection, &intent) {
        Ok(operation) => Ok(Some(StartedRemoval {
            operation,
            lease_id,
        })),
        Err(OperationIntentError::WorkspaceBusy { .. }) => Ok(None),
        Err(OperationIntentError::Database { source }) => Err(GcError::Database { source }),
    }
}

fn reconcile_started_removal(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    workspace_path: &CanonicalPath,
    started: &StartedRemoval,
    force: bool,
) -> Result<Option<RemovalReport>, GcError> {
    if let Err(error) = reconciliation::reconcile_workspace_with_lease(
        connection,
        workspace_id,
        &started.operation.id,
        &started.lease_id,
    ) {
        let error_text = error.to_string();
        finish_removal_failure(
            connection,
            &started.lease_id,
            workspace_id,
            removal_details(workspace_path, force, Some(&error_text)),
            &error_text,
        )?;
        return Ok(Some(removal_failed(
            workspace_path.clone(),
            GcCandidateReason::GitError,
            error_text,
        )));
    }
    Ok(None)
}

fn prepare_started_removal(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    lease_id: &crate::domain::LeaseId,
    force: bool,
) -> Result<PreparedRemoval, GcError> {
    let workspace = find_workspace(connection, workspace_id).context(DatabaseSnafu)?;
    let reason = removal_reason_while_owned(connection, &workspace, force)?;
    if reason != GcCandidateReason::Eligible {
        return finish_skipped_removal(
            connection,
            workspace_id,
            lease_id,
            workspace,
            force,
            reason,
        );
    }

    let repositories = list_repo_worktrees(connection, workspace_id).context(DatabaseSnafu)?;
    match prepare_removal(&workspace, &repositories, force) {
        Ok(plan) => Ok(PreparedRemoval::Ready { workspace, plan }),
        Err(reason) => {
            finish_skipped_removal(connection, workspace_id, lease_id, workspace, force, reason)
        }
    }
}

fn finish_skipped_removal(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    lease_id: &crate::domain::LeaseId,
    workspace: crate::storage::WorkspaceRow,
    force: bool,
    reason: GcCandidateReason,
) -> Result<PreparedRemoval, GcError> {
    finish_removal_skip(
        connection,
        lease_id,
        workspace_id,
        removal_details(&workspace.canonical_path, force, None),
        reason,
    )?;
    Ok(PreparedRemoval::Complete(removal_rejected(
        workspace.canonical_path,
        reason,
    )))
}

fn execute_prepared_removal(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    started: StartedRemoval,
    workspace: crate::storage::WorkspaceRow,
    removal_plan: RemovalPlan,
    force: bool,
) -> Result<RemovalReport, GcError> {
    if let Some(report) = remove_workspace_physical_state(
        connection,
        workspace_id,
        &started,
        &workspace,
        &removal_plan,
        force,
    )? {
        return Ok(report);
    }
    persist_removed_workspace(
        connection,
        workspace_id,
        &started.lease_id,
        workspace,
        force,
    )
}

fn remove_workspace_physical_state(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    started: &StartedRemoval,
    workspace: &crate::storage::WorkspaceRow,
    removal_plan: &RemovalPlan,
    force: bool,
) -> Result<Option<RemovalReport>, GcError> {
    if let Err(error) = remove_physical_workspace(removal_plan, force, || {
        renew_gc_lease(connection, &started.lease_id)
    }) {
        let error_text = error.to_string();
        let _ = reconciliation::reconcile_workspace_with_lease(
            connection,
            workspace_id,
            &started.operation.id,
            &started.lease_id,
        );
        finish_removal_failure(
            connection,
            &started.lease_id,
            workspace_id,
            removal_details(&workspace.canonical_path, force, Some(&error_text)),
            &error_text,
        )?;
        return Ok(Some(removal_failed(
            workspace.canonical_path.clone(),
            GcCandidateReason::GitError,
            error_text,
        )));
    }
    Ok(None)
}

fn persist_removed_workspace(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    lease_id: &crate::domain::LeaseId,
    workspace: crate::storage::WorkspaceRow,
    force: bool,
) -> Result<RemovalReport, GcError> {
    if let Err(error) = record_workspace_explicitly_removed(
        connection,
        lease_id,
        workspace_id,
        Some(removal_details(&workspace.canonical_path, force, None)),
    ) {
        let error_text = error.to_string();
        finish_removal_failure(
            connection,
            lease_id,
            workspace_id,
            removal_details(&workspace.canonical_path, force, Some(&error_text)),
            &error_text,
        )?;
        return Ok(removal_failed(
            workspace.canonical_path,
            GcCandidateReason::GitError,
            error_text,
        ));
    }
    Ok(RemovalReport {
        workspace_path: workspace.canonical_path,
        removed: true,
        reason: GcCandidateReason::Eligible,
        error: None,
    })
}

fn removal_reason_while_owned(
    connection: &mut SqliteConnection,
    workspace: &crate::storage::WorkspaceRow,
    force: bool,
) -> Result<GcCandidateReason, GcError> {
    if workspace.state == WorkspaceState::Reclaimed {
        return Ok(GcCandidateReason::Reclaimed);
    }
    if find_workspace_claim(connection, &workspace.id)
        .context(DatabaseSnafu)?
        .is_some()
    {
        return Ok(GcCandidateReason::Claimed);
    }
    if workspace_is_unhealthy(workspace, force) {
        return Ok(GcCandidateReason::Unhealthy);
    }
    Ok(GcCandidateReason::Eligible)
}

fn removal_rejected(workspace_path: CanonicalPath, reason: GcCandidateReason) -> RemovalReport {
    RemovalReport {
        workspace_path,
        removed: false,
        reason,
        error: None,
    }
}

fn removal_failed(
    workspace_path: CanonicalPath,
    reason: GcCandidateReason,
    error: String,
) -> RemovalReport {
    RemovalReport {
        workspace_path,
        removed: false,
        reason,
        error: Some(error),
    }
}

fn removal_details(
    workspace_path: &CanonicalPath,
    force: bool,
    error: Option<&str>,
) -> JsonDocument {
    JsonDocument::from_serializable(&serde_json::json!({
        "workspace_path": workspace_path,
        "forced": force,
        "command": "remove",
        "error": error,
    }))
    .expect("removal details should serialize")
}

fn finish_removal_skip(
    connection: &mut SqliteConnection,
    lease_id: &crate::domain::LeaseId,
    workspace_id: &WorkspaceId,
    details_json: JsonDocument,
    reason: GcCandidateReason,
) -> Result<(), GcError> {
    let error_json = JsonDocument::from_serializable(&serde_json::json!({
        "reason": reason.to_string(),
    }))
    .expect("removal skip error should serialize");
    record_workspace_remove_skipped(
        connection,
        lease_id,
        workspace_id,
        Some(details_json),
        error_json,
    )
    .context(DatabaseSnafu)
}

fn finish_removal_failure(
    connection: &mut SqliteConnection,
    lease_id: &crate::domain::LeaseId,
    workspace_id: &WorkspaceId,
    details_json: JsonDocument,
    error: &str,
) -> Result<(), GcError> {
    let error_json = JsonDocument::from_serializable(&serde_json::json!({
        "error": error,
    }))
    .expect("removal error should serialize");
    record_workspace_remove_failure(
        connection,
        lease_id,
        workspace_id,
        Some(details_json),
        error_json,
    )
    .context(DatabaseSnafu)
}

fn execute_candidate(
    connection: &mut SqliteConnection,
    candidate: &GcCandidate,
    scan: &GcScan,
    force: bool,
) -> Result<GcCandidateResult, GcError> {
    let reason = candidate.reason(force);
    if reason != GcCandidateReason::Eligible {
        return Ok(GcCandidateResult::Skipped(GcSkipped {
            workspace_path: candidate.workspace.canonical_path.clone(),
            reason,
        }));
    }

    let Some((operation, lease_id)) = begin_gc_operation(connection, candidate, scan, force)?
    else {
        return Ok(GcCandidateResult::Skipped(GcSkipped {
            workspace_path: candidate.workspace.canonical_path.clone(),
            reason: GcCandidateReason::ActiveOperation,
        }));
    };
    let workspace_id = candidate.workspace.id;
    let details_json = gc_details(&candidate.workspace.canonical_path, scan, force, None);
    if let Err(error) = reconciliation::reconcile_workspace_with_lease(
        connection,
        &workspace_id,
        &operation.id,
        &lease_id,
    ) {
        return finish_candidate_failure(
            connection,
            &lease_id,
            &workspace_id,
            candidate.workspace.canonical_path.clone(),
            details_json,
            error.to_string(),
        );
    }

    execute_started_candidate(
        connection,
        &operation.id,
        &lease_id,
        &workspace_id,
        scan,
        force,
        details_json,
    )
}

fn execute_started_candidate(
    connection: &mut SqliteConnection,
    operation_id: &crate::domain::OperationId,
    lease_id: &crate::domain::LeaseId,
    workspace_id: &WorkspaceId,
    scan: &GcScan,
    force: bool,
    details_json: JsonDocument,
) -> Result<GcCandidateResult, GcError> {
    let workspace = find_workspace(connection, workspace_id).context(DatabaseSnafu)?;
    let claim = find_workspace_claim(connection, workspace_id).context(DatabaseSnafu)?;
    if claim.is_some() {
        finish_gc_skip(
            connection,
            lease_id,
            workspace_id,
            details_json,
            GcCandidateReason::Claimed,
        )?;
        return Ok(GcCandidateResult::Skipped(GcSkipped {
            workspace_path: workspace.canonical_path,
            reason: GcCandidateReason::Claimed,
        }));
    }
    if workspace_is_unhealthy(&workspace, force) {
        finish_gc_skip(
            connection,
            lease_id,
            workspace_id,
            details_json,
            GcCandidateReason::Unhealthy,
        )?;
        return Ok(GcCandidateResult::Skipped(GcSkipped {
            workspace_path: workspace.canonical_path,
            reason: GcCandidateReason::Unhealthy,
        }));
    }

    let repositories = list_repo_worktrees(connection, workspace_id).context(DatabaseSnafu)?;
    let removal_plan = match prepare_removal(&workspace, &repositories, force) {
        Ok(plan) => plan,
        Err(reason) => {
            finish_gc_skip(connection, lease_id, workspace_id, details_json, reason)?;
            return Ok(GcCandidateResult::Skipped(GcSkipped {
                workspace_path: workspace.canonical_path,
                reason,
            }));
        }
    };
    execute_removal(
        connection,
        operation_id,
        lease_id,
        workspace,
        removal_plan,
        scan,
        force,
    )
}

fn workspace_is_unhealthy(workspace: &crate::storage::WorkspaceRow, force: bool) -> bool {
    workspace.state == WorkspaceState::Reclaimed
        || (!force && workspace.state != WorkspaceState::Ready)
}

fn execute_removal(
    connection: &mut SqliteConnection,
    operation_id: &crate::domain::OperationId,
    lease_id: &crate::domain::LeaseId,
    workspace: crate::storage::WorkspaceRow,
    removal_plan: RemovalPlan,
    scan: &GcScan,
    force: bool,
) -> Result<GcCandidateResult, GcError> {
    if let Err(error) = remove_physical_workspace(&removal_plan, force, || {
        renew_gc_lease(connection, lease_id)
    }) {
        let error_text = error.to_string();
        let _ = reconciliation::reconcile_workspace_with_lease(
            connection,
            &workspace.id,
            operation_id,
            lease_id,
        );
        return finish_candidate_failure(
            connection,
            lease_id,
            &workspace.id,
            workspace.canonical_path.clone(),
            gc_details(&workspace.canonical_path, scan, force, Some(&error_text)),
            error_text,
        );
    }

    if let Err(error) = record_workspace_reclaimed(
        connection,
        lease_id,
        &workspace.id,
        Some(gc_details(&workspace.canonical_path, scan, force, None)),
    ) {
        let error_text = error.to_string();
        return finish_candidate_failure(
            connection,
            lease_id,
            &workspace.id,
            workspace.canonical_path.clone(),
            gc_details(&workspace.canonical_path, scan, force, Some(&error_text)),
            error_text,
        );
    }
    Ok(GcCandidateResult::Reclaimed(workspace.canonical_path))
}

fn finish_candidate_failure(
    connection: &mut SqliteConnection,
    lease_id: &crate::domain::LeaseId,
    workspace_id: &WorkspaceId,
    workspace_path: CanonicalPath,
    details_json: JsonDocument,
    error: String,
) -> Result<GcCandidateResult, GcError> {
    finish_gc_failure(connection, lease_id, workspace_id, details_json, &error)?;
    Ok(GcCandidateResult::Failed(GcFailure {
        workspace_path,
        error,
    }))
}

fn recover_expired_automatic_operations(
    connection: &mut SqliteConnection,
    cutoff: &Timestamp,
    force: bool,
) -> Result<(), GcError> {
    for workspace in list_automatic_workspaces(connection).context(DatabaseSnafu)? {
        let claim = find_workspace_claim(connection, &workspace.id).context(DatabaseSnafu)?;
        if claim.is_some() {
            continue;
        }
        let idle_since = workspace
            .last_released_at
            .clone()
            .unwrap_or_else(|| workspace.created_at.clone());
        if idle_since >= *cutoff
            || workspace.state == WorkspaceState::Reclaimed
            || (!force && workspace.state != WorkspaceState::Ready)
        {
            continue;
        }
        let Some(running_operation) =
            find_running_operation(connection, &workspace.id).context(DatabaseSnafu)?
        else {
            continue;
        };
        if !running_operation.lease.lease_expires_at.has_expired() {
            continue;
        }
        reconciliation::recover_expired_operation(connection, &workspace.id)
            .context(ReconciliationSnafu)?;
    }
    Ok(())
}

fn begin_gc_operation(
    connection: &mut SqliteConnection,
    candidate: &GcCandidate,
    scan: &GcScan,
    force: bool,
) -> Result<Option<(crate::storage::OperationRow, crate::domain::LeaseId)>, GcError> {
    let intent_json = gc_details(&candidate.workspace.canonical_path, scan, force, None);
    let intent = OperationIntent::new(
        candidate.workspace.id,
        "gc",
        Timestamp::after_seconds(300),
        "reclaim workspace",
        intent_json,
    );
    let lease_id = intent.lease_id;
    match begin_operation(connection, &intent) {
        Ok(operation) => Ok(Some((operation, lease_id))),
        Err(OperationIntentError::WorkspaceBusy { .. }) => Ok(None),
        Err(OperationIntentError::Database { source }) => Err(GcError::Database { source }),
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
            "unclaimed": scan.counts.unclaimed,
            "claimed": scan.counts.claimed,
            "age_eligible": scan.counts.age_eligible,
            "safe_to_reclaim": scan.counts.safe_to_reclaim,
        },
        "error": error,
    }))
    .expect("GC details should serialize")
}

fn finish_gc_skip(
    connection: &mut SqliteConnection,
    lease_id: &crate::domain::LeaseId,
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
        lease_id,
        workspace_id,
        Some(details_json),
        error_json,
    )
    .context(DatabaseSnafu)
}

fn finish_gc_failure(
    connection: &mut SqliteConnection,
    lease_id: &crate::domain::LeaseId,
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
        lease_id,
        workspace_id,
        Some(details_json),
        error_json,
    )
    .context(DatabaseSnafu)
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
    force: bool,
) -> Result<RemovalPlan, GcCandidateReason> {
    let Some(workspace_root) = workspace.canonical_path.as_path().parent() else {
        return Err(GcCandidateReason::UnsafeRoot);
    };
    let expected_paths = repositories
        .iter()
        .map(|repository| repository.worktree_path.as_path().to_owned())
        .collect::<Vec<_>>();
    validate_repository_layout(workspace, repositories, workspace_root)?;
    let Some(extra_entries) =
        prepare_workspace_entries(workspace, workspace_root, &expected_paths, force)?
    else {
        return Ok(RemovalPlan {
            workspace_path: workspace.canonical_path.as_path().to_owned(),
            worktrees: Vec::new(),
            extra_entries: Vec::new(),
        });
    };
    let worktrees = prepare_worktree_removals(repositories, force)?;
    Ok(RemovalPlan {
        workspace_path: workspace.canonical_path.as_path().to_owned(),
        worktrees,
        extra_entries,
    })
}

fn validate_repository_layout(
    workspace: &crate::storage::WorkspaceRow,
    repositories: &[RepoWorktreeRow],
    workspace_root: &Path,
) -> Result<(), GcCandidateReason> {
    let workspace_root_is_worktree = repositories.len() == 1
        && repositories[0].worktree_path.as_path() == workspace.canonical_path.as_path();
    for repository in repositories {
        if (!workspace_root_is_worktree
            && repository.worktree_path.as_path().parent()
                != Some(workspace.canonical_path.as_path()))
            || !repository
                .worktree_path
                .as_path()
                .starts_with(workspace_root)
        {
            return Err(GcCandidateReason::UnsafeRoot);
        }
        let actual_identity = git::inspect_repository_identity(&repository.source_path)
            .map_err(|_| GcCandidateReason::RepositoryIdentity)?;
        if actual_identity != repository.repository_identity {
            return Err(GcCandidateReason::RepositoryIdentity);
        }
    }
    Ok(())
}

fn prepare_workspace_entries(
    workspace: &crate::storage::WorkspaceRow,
    workspace_root: &Path,
    expected_paths: &[PathBuf],
    force: bool,
) -> Result<Option<Vec<PathBuf>>, GcCandidateReason> {
    if !workspace_directory_exists(workspace.canonical_path.as_path())? {
        if force {
            return Ok(None);
        }
        return Err(GcCandidateReason::WorktreeMismatch);
    }

    if workspace_root_is_worktree(workspace, expected_paths) {
        if !force {
            validation::validate_workspace_root(
                workspace.canonical_path.as_path(),
                workspace_root,
                expected_paths,
            )
            .map_err(workspace_root_reason)?;
        }
        return Ok(Some(Vec::new()));
    }

    let extra_entries = unexpected_workspace_entries(workspace, expected_paths)?;
    if !force && !extra_entries.is_empty() {
        return Err(GcCandidateReason::UnexpectedContent);
    }
    if !force {
        validation::validate_workspace_root(
            workspace.canonical_path.as_path(),
            workspace_root,
            expected_paths,
        )
        .map_err(workspace_root_reason)?;
    }
    Ok(if force {
        Some(extra_entries)
    } else {
        Some(Vec::new())
    })
}

fn workspace_directory_exists(workspace_path: &Path) -> Result<bool, GcCandidateReason> {
    match fs::symlink_metadata(workspace_path) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            Ok(true)
        }
        Ok(_) => Err(GcCandidateReason::UnsafeRoot),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(GcCandidateReason::UnsafeRoot),
    }
}

fn workspace_root_is_worktree(
    workspace: &crate::storage::WorkspaceRow,
    expected_paths: &[PathBuf],
) -> bool {
    expected_paths == [workspace.canonical_path.as_path()]
}

fn unexpected_workspace_entries(
    workspace: &crate::storage::WorkspaceRow,
    expected_paths: &[PathBuf],
) -> Result<Vec<PathBuf>, GcCandidateReason> {
    Ok(fs::read_dir(workspace.canonical_path.as_path())
        .map_err(|_| GcCandidateReason::UnexpectedContent)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| GcCandidateReason::UnexpectedContent)?
        .into_iter()
        .filter(|entry| !expected_paths.iter().any(|expected| expected == entry))
        .collect())
}

fn workspace_root_reason(error: validation::WorkspaceRootError) -> GcCandidateReason {
    match error {
        validation::WorkspaceRootError::UnexpectedEntry { .. } => {
            GcCandidateReason::UnexpectedContent
        }
        validation::WorkspaceRootError::NotDirectory { .. }
        | validation::WorkspaceRootError::OutsideManagedRoot { .. }
        | validation::WorkspaceRootError::NotAbsolute { .. } => GcCandidateReason::UnsafeRoot,
        validation::WorkspaceRootError::ReadDirectory { .. } => {
            GcCandidateReason::UnexpectedContent
        }
    }
}

fn prepare_worktree_removals(
    repositories: &[RepoWorktreeRow],
    force: bool,
) -> Result<Vec<WorktreeRemoval>, GcCandidateReason> {
    repositories
        .iter()
        .map(|repository| prepare_worktree_removal(repository, force))
        .collect::<Result<Vec<_>, _>>()
        .map(|removals| removals.into_iter().flatten().collect())
}

fn prepare_worktree_removal(
    repository: &RepoWorktreeRow,
    force: bool,
) -> Result<Option<WorktreeRemoval>, GcCandidateReason> {
    let listed = git::list_worktrees(&repository.source_path)
        .map_err(|_| GcCandidateReason::GitError)?
        .into_iter()
        .find(|worktree| worktree.path.as_path() == repository.worktree_path.as_path());
    let worktree_exists = worktree_path_exists(repository)?;
    if let Some(worktree) = listed.as_ref() {
        ensure_unbranched_worktree(worktree)?;
    }
    if !force {
        validate_clean_worktree(repository, listed.as_ref(), worktree_exists)?;
    }
    let Some(worktree) = listed else {
        return Ok(worktree_exists.then(|| WorktreeRemoval {
            repository: repository.source_path.clone(),
            path: repository.worktree_path.as_path().to_owned(),
            remove_from_git: false,
        }));
    };
    if force && worktree_exists {
        validate_worktree_identity(repository)?;
    }
    Ok(Some(WorktreeRemoval {
        repository: repository.source_path.clone(),
        path: worktree.path.into_path_buf(),
        remove_from_git: true,
    }))
}

fn worktree_path_exists(repository: &RepoWorktreeRow) -> Result<bool, GcCandidateReason> {
    match fs::symlink_metadata(repository.worktree_path.as_path()) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(GcCandidateReason::WorktreeIdentity)
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(GcCandidateReason::WorktreeMismatch),
    }
}

fn ensure_unbranched_worktree(worktree: &git::WorktreeInfo) -> Result<(), GcCandidateReason> {
    if !worktree.detached || worktree.branch.is_some() {
        return Err(GcCandidateReason::WorktreeMismatch);
    }
    Ok(())
}

fn validate_clean_worktree(
    repository: &RepoWorktreeRow,
    worktree: Option<&git::WorktreeInfo>,
    worktree_exists: bool,
) -> Result<(), GcCandidateReason> {
    let Some(worktree) = worktree else {
        return Err(GcCandidateReason::WorktreeMismatch);
    };
    if worktree.prunable.is_some() || !worktree_exists {
        return Err(GcCandidateReason::WorktreeMismatch);
    }
    validate_worktree_identity(repository)?;
    if worktree.head.as_deref() != repository.last_head.as_deref()
        || !git::is_worktree_clean(repository.worktree_path.as_path())
            .map_err(|_| GcCandidateReason::GitError)?
    {
        return Err(GcCandidateReason::WorktreeMismatch);
    }
    Ok(())
}

fn validate_worktree_identity(repository: &RepoWorktreeRow) -> Result<(), GcCandidateReason> {
    let identity = git::inspect_worktree_identity(repository.worktree_path.as_path())
        .map_err(|_| GcCandidateReason::WorktreeIdentity)?;
    if identity != repository.repository_identity {
        return Err(GcCandidateReason::WorktreeIdentity);
    }
    Ok(())
}

fn remove_physical_workspace<F>(
    plan: &RemovalPlan,
    force: bool,
    mut heartbeat: F,
) -> Result<(), GcPhysicalError>
where
    F: FnMut() -> Result<(), GcPhysicalError>,
{
    for worktree in &plan.worktrees {
        heartbeat()?;
        if worktree.remove_from_git {
            let mut git_heartbeat = || heartbeat().map_err(git::GitError::from_heartbeat_source);
            if force {
                git::remove_worktree_with_heartbeat(
                    &worktree.repository,
                    &worktree.path,
                    &mut git_heartbeat,
                )?;
            } else {
                git::remove_clean_worktree_with_heartbeat(
                    &worktree.repository,
                    &worktree.path,
                    &mut git_heartbeat,
                )?;
            }
        } else if worktree.path.exists() {
            remove_path_with_heartbeat(&worktree.path, &mut heartbeat)?;
        }
    }
    for entry in &plan.extra_entries {
        heartbeat()?;
        remove_path_with_heartbeat(entry, &mut heartbeat)?;
    }
    if plan.workspace_path.exists() {
        heartbeat()?;
        fs::remove_dir(&plan.workspace_path).context(IoSnafu {
            path: &plan.workspace_path,
        })?;
    }
    Ok(())
}

fn remove_path_with_heartbeat<F>(path: &Path, heartbeat: &mut F) -> Result<(), GcPhysicalError>
where
    F: FnMut() -> Result<(), GcPhysicalError>,
{
    heartbeat()?;
    let metadata = fs::symlink_metadata(path).context(IoSnafu { path })?;
    if metadata.file_type().is_dir() {
        for entry in fs::read_dir(path).context(IoSnafu { path })? {
            let entry = entry.context(IoSnafu { path })?;
            remove_path_with_heartbeat(&entry.path(), heartbeat)?;
        }
        fs::remove_dir(path).context(IoSnafu { path })?;
    } else {
        fs::remove_file(path).context(IoSnafu { path })?;
    }
    Ok(())
}

fn renew_gc_lease(
    connection: &mut SqliteConnection,
    lease_id: &crate::domain::LeaseId,
) -> Result<(), GcPhysicalError> {
    match renew_operation_lease(connection, lease_id).context(LeaseRenewalSnafu)? {
        true => Ok(()),
        false => Err(GcPhysicalError::Lease {
            message: "operation lease is no longer owned".to_owned(),
        }),
    }
}

#[derive(Debug, Snafu)]
enum GcPhysicalError {
    #[snafu(transparent)]
    Git { source: crate::git::GitError },
    #[snafu(display("GC operation lease renewal failed: {message}"))]
    Lease { message: String },
    #[snafu(display("GC operation lease renewal failed: {source}"))]
    LeaseRenewal { source: diesel::result::Error },
    #[snafu(display("GC filesystem operation failed for {}: {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Snafu)]
pub enum GcError {
    #[snafu(display("workspace not found: {workspace_id}"))]
    WorkspaceNotFound { workspace_id: WorkspaceId },
    #[snafu(display("GC database operation failed: {source}"))]
    Database { source: diesel::result::Error },
    #[snafu(display("GC reconciliation operation failed: {source}"))]
    Reconciliation {
        source: reconciliation::ReconciliationError,
    },
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
        ensure_origin_repository, ensure_workspace_pool, insert_managed_workspace,
        insert_workspace_claim, NewManagedWorkspace, NewWorkspaceClaim,
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
    ) {
        let root = test_root();
        let source_path = root.join("source");
        repository(&source_path);
        let database_path = root.join("state.sqlite");
        let mut connection = database::connect(&database_path).expect("database should open");
        let mut plan = prepare_automatic(&AutomaticCreateRequest {
            repositories: vec![source_path],
            offline: false,
        })
        .expect("automatic allocation plan should be prepared");
        plan.workspace_root = CanonicalPath::from_absolute(root.join("managed"))
            .expect("workspace root should be absolute");
        let acquire = provision_automatic(&mut connection, &plan)
            .expect("automatic workspace should be provisioned");
        release_automatic_workspace(&mut connection, &acquire.workspace_path, acquire.claim_id)
            .expect("workspace should be released");
        let mut workspace =
            crate::storage::find_workspace_by_path(&mut connection, &acquire.workspace_path)
                .expect("workspace lookup should succeed")
                .expect("workspace should exist");
        let old = timestamp("2020-01-01T00:00:00Z");
        diesel::update(crate::schema::workspaces::table.find(workspace.id))
            .set((
                crate::schema::workspaces::created_at.eq(old.clone()),
                crate::schema::workspaces::last_released_at.eq(Some(old)),
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
        (
            root,
            database_path,
            connection,
            workspace,
            source,
            worktree.worktree_path.into_path_buf(),
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
        let error = "999999999999999999999999999999999999999999s"
            .parse::<GcDuration>()
            .expect_err("out-of-range duration should fail");
        assert_eq!(error.to_string(), "duration number is out of range");
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn formats_gc_candidate_reasons() {
        let reasons = [
            (GcCandidateReason::Eligible, "eligible"),
            (GcCandidateReason::Young, "young"),
            (GcCandidateReason::Claimed, "claimed"),
            (GcCandidateReason::ActiveOperation, "active_operation"),
            (GcCandidateReason::ExpiredOperation, "expired_operation"),
            (GcCandidateReason::Reclaimed, "reclaimed"),
            (GcCandidateReason::Unhealthy, "unhealthy"),
            (GcCandidateReason::UnsafeRoot, "unsafe_root"),
            (GcCandidateReason::RepositoryIdentity, "repository_identity"),
            (GcCandidateReason::WorktreeIdentity, "worktree_identity"),
            (GcCandidateReason::WorktreeMismatch, "worktree_mismatch"),
            (GcCandidateReason::UnexpectedContent, "unexpected_content"),
            (GcCandidateReason::GitError, "git_error"),
        ];

        for (reason, expected) in reasons {
            assert_eq!(reason.to_string(), expected);
        }
    }

    #[test]
    fn identifies_recoverable_expired_operations() {
        let candidate =
            |state, age_eligible, claimed, active_operation, operation_expired| GcCandidate {
                workspace: crate::storage::WorkspaceRow {
                    id: WorkspaceId::new(),
                    canonical_path: CanonicalPath::from_absolute("/tmp/workspace")
                        .expect("workspace path should be absolute"),
                    state,
                    created_at: timestamp("2020-01-01T00:00:00Z"),
                    updated_at: timestamp("2020-01-01T00:00:00Z"),
                    last_reconciled_at: None,
                    management_mode: WorkspaceManagementMode::Automatic,
                    pool_id: None,
                    last_released_at: None,
                    reclaimed_at: None,
                },
                idle_since: timestamp("2020-01-01T00:00:00Z"),
                age_eligible,
                claimed,
                active_operation,
                physical_reason: None,
                operation_expired,
            };

        assert!(candidate(WorkspaceState::Ready, true, false, true, true)
            .recoverable_expired_operation(false));
        assert!(candidate(WorkspaceState::Ready, true, false, true, true)
            .recoverable_expired_operation(true));
        assert!(!candidate(WorkspaceState::Ready, false, false, true, true)
            .recoverable_expired_operation(false));
        assert!(!candidate(WorkspaceState::Ready, true, true, true, true)
            .recoverable_expired_operation(false));
        assert!(!candidate(WorkspaceState::Ready, true, false, false, true)
            .recoverable_expired_operation(false));
        assert!(!candidate(WorkspaceState::Ready, true, false, true, false)
            .recoverable_expired_operation(false));
        assert!(
            !candidate(WorkspaceState::Reclaimed, true, false, true, true)
                .recoverable_expired_operation(true)
        );
        assert!(
            !candidate(WorkspaceState::Degraded, true, false, true, true)
                .recoverable_expired_operation(false)
        );

        let unhealthy = candidate(WorkspaceState::Degraded, true, false, false, false);
        assert_eq!(unhealthy.reason(false), GcCandidateReason::Unhealthy);
        assert_eq!(unhealthy.reason(true), GcCandidateReason::Eligible);
    }

    #[test]
    fn removes_nested_paths_while_renewing() {
        let root = test_root();
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("nested directory should be created");
        fs::write(nested.join("file"), "content\n").expect("nested file should be written");
        let mut heartbeat_count = 0;
        remove_path_with_heartbeat(&root, &mut || {
            heartbeat_count += 1;
            Ok(())
        })
        .expect("nested path should be removed");

        assert!(!root.exists());
        assert!(heartbeat_count >= 3);
    }

    #[test]
    fn stops_path_removal_when_heartbeat_fails() {
        let root = test_root();
        fs::create_dir_all(&root).expect("directory should be created");
        let mut heartbeat_count = 0;
        let error = remove_path_with_heartbeat(&root, &mut || {
            heartbeat_count += 1;
            Err(GcPhysicalError::Lease {
                message: "expired".to_owned(),
            })
        })
        .expect_err("failed heartbeat should stop removal");

        assert!(matches!(error, GcPhysicalError::Lease { message } if message == "expired"));
        assert_eq!(heartbeat_count, 1);
        fs::remove_dir_all(root).expect("test directory should be removable");
    }

    #[test]
    fn scans_idle_and_claimed_automatic_workspaces() {
        let root = test_root();
        let database_path = root.join("state.sqlite");
        let workspace_root = CanonicalPath::from_absolute(root.join("managed"))
            .expect("workspace root should be absolute");
        fs::create_dir_all(&root).expect("GC test root should be created");
        let mut connection = database::connect(&database_path).expect("database should open");
        let old = timestamp("2020-01-01T00:00:00Z");
        let young = timestamp("2099-01-01T00:00:00Z");
        let repository_path = CanonicalPath::from_absolute("/repo/example")
            .expect("repository path should be absolute");
        let repository_id =
            ensure_origin_repository(&mut connection, &repository_path, &repository_path)
                .expect("origin repository should be available")
                .id;
        let pool_id = Some(
            ensure_workspace_pool(
                &mut connection,
                &RepositorySetKey::from_repository_ids(&[repository_id]),
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
            let workspace_path =
                CanonicalPath::from_absolute(workspace_root.as_path().join(id.to_string()))
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
                    pool_id,
                    last_released_at: Some(idle_since),
                    reclaimed_at: None,
                },
            )
            .expect("workspace should be inserted");
            if claim_kind.is_some() {
                let claim = WorkspaceClaim::new(id);
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
                pool_id: None,
                last_released_at: Some(old.clone()),
                reclaimed_at: None,
            },
        )
        .expect("manual workspace should be inserted");

        let result = scan(
            &mut connection,
            "30d".parse().expect("duration should parse"),
        )
        .expect("GC scan should succeed");
        assert_eq!(result.counts.automatic, 4);
        assert_eq!(result.counts.unclaimed, 3);
        assert_eq!(result.counts.claimed, 1);
        assert_eq!(result.counts.age_eligible, 3);
        assert_eq!(result.counts.safe_to_reclaim, 0);
        assert_eq!(workspace_ids.len(), 4);

        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("GC test root should be removable");
    }

    #[test]
    fn reclaims_an_old_clean_automatic_workspace() {
        let (root, database_path, mut connection, workspace, source, worktree) =
            automatic_workspace_fixture();

        let report = execute(
            &mut connection,
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
        let operation_id = crate::schema::operations::table
            .filter(crate::schema::operations::workspace_id.eq(workspace.id))
            .order(crate::schema::operations::started_at.desc())
            .select(crate::schema::operations::id)
            .first::<crate::domain::OperationId>(&mut connection)
            .expect("GC operation should exist");
        assert_eq!(
            crate::storage::operation_state(&mut connection, &operation_id)
                .expect("GC operation state should exist"),
            Some(crate::domain::OperationState::Succeeded)
        );
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
    fn recovers_an_expired_operation_before_gc() {
        let (root, database_path, mut connection, workspace, source, worktree) =
            automatic_workspace_fixture();
        let stale_intent = OperationIntent::new(
            workspace.id,
            "create",
            Timestamp::now(),
            "recover stale operation",
            JsonDocument::parse(r#"{"recovery":true}"#).unwrap(),
        );
        let stale_operation =
            begin_operation(&mut connection, &stale_intent).expect("stale operation should start");
        let stale_lease =
            crate::storage::find_operation_lease(&mut connection, &stale_operation.id)
                .expect("stale operation lease should be queryable")
                .expect("stale operation lease should exist");
        diesel::update(crate::schema::operation_leases::table.find(stale_lease.id))
            .set(crate::schema::operation_leases::lease_expires_at.eq(Timestamp::now()))
            .execute(&mut connection)
            .expect("stale operation lease should expire");

        let report = execute(
            &mut connection,
            "30d".parse().expect("duration should parse"),
            false,
        )
        .expect("GC should recover the stale operation");
        assert_eq!(report.reclaimed, vec![workspace.canonical_path]);
        assert_eq!(
            crate::storage::operation_state(&mut connection, &stale_operation.id)
                .expect("stale operation state should be queryable"),
            Some(crate::domain::OperationState::Succeeded)
        );
        assert!(!worktree.exists());
        assert_eq!(
            crate::git::list_worktrees(&source)
                .expect("source worktrees should be readable")
                .len(),
            1
        );

        drop(connection);
        fs::remove_file(database_path).expect("database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn gc_does_not_recover_an_expired_young_operation() {
        let (root, database_path, mut connection, workspace, source, worktree) =
            automatic_workspace_fixture();
        diesel::update(crate::schema::workspaces::table.find(workspace.id))
            .set(crate::schema::workspaces::last_released_at.eq(timestamp("2099-01-01T00:00:00Z")))
            .execute(&mut connection)
            .expect("workspace should be made young");
        let stale_intent = OperationIntent::new(
            workspace.id,
            "create",
            Timestamp::now(),
            "recover stale operation",
            JsonDocument::parse(r#"{"recovery":true}"#).unwrap(),
        );
        let stale_operation =
            begin_operation(&mut connection, &stale_intent).expect("stale operation should start");
        let stale_lease =
            crate::storage::find_operation_lease(&mut connection, &stale_operation.id)
                .expect("stale operation lease should be queryable")
                .expect("stale operation lease should exist");
        diesel::update(crate::schema::operation_leases::table.find(stale_lease.id))
            .set(crate::schema::operation_leases::lease_expires_at.eq(Timestamp::now()))
            .execute(&mut connection)
            .expect("stale operation lease should expire");

        let report = execute(
            &mut connection,
            "30d".parse().expect("duration should parse"),
            false,
        )
        .expect("GC should skip the young workspace");
        assert!(report.reclaimed.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].reason, GcCandidateReason::Young);
        assert_eq!(
            crate::storage::operation_state(&mut connection, &stale_operation.id)
                .expect("stale operation state should be queryable"),
            Some(crate::domain::OperationState::Running)
        );
        assert!(workspace.canonical_path.as_path().exists());

        crate::git::remove_worktree(&source, &worktree).expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn gc_does_not_recover_an_expired_claimed_operation() {
        let (root, database_path, mut connection, workspace, source, worktree) =
            automatic_workspace_fixture();
        let claim = crate::workspace::allocate_automatic_workspace(
            &mut connection,
            &prepare_automatic(&AutomaticCreateRequest {
                repositories: vec![source.as_path().to_owned()],
                offline: false,
            })
            .expect("automatic allocation plan should be prepared"),
        )
        .expect("workspace should be claimed");
        let stale_intent = OperationIntent::new(
            workspace.id,
            "create",
            Timestamp::now(),
            "recover stale operation",
            JsonDocument::parse(r#"{"recovery":true}"#).unwrap(),
        );
        let stale_operation =
            begin_operation(&mut connection, &stale_intent).expect("stale operation should start");
        let stale_lease =
            crate::storage::find_operation_lease(&mut connection, &stale_operation.id)
                .expect("stale operation lease should be queryable")
                .expect("stale operation lease should exist");
        diesel::update(crate::schema::operation_leases::table.find(stale_lease.id))
            .set(crate::schema::operation_leases::lease_expires_at.eq(Timestamp::now()))
            .execute(&mut connection)
            .expect("stale operation lease should expire");

        let report = execute(
            &mut connection,
            "30d".parse().expect("duration should parse"),
            true,
        )
        .expect("GC should skip the claimed workspace");
        assert!(report.reclaimed.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].reason, GcCandidateReason::Claimed);
        assert_eq!(
            crate::storage::operation_state(&mut connection, &stale_operation.id)
                .expect("stale operation state should be queryable"),
            Some(crate::domain::OperationState::Running)
        );
        assert_eq!(
            crate::storage::find_workspace_claim(&mut connection, &workspace.id)
                .expect("workspace claim should be queryable")
                .expect("workspace claim should remain active")
                .id,
            claim.claim_id
        );

        crate::git::remove_worktree(&source, &worktree).expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn force_reclaims_an_old_dirty_workspace_and_extra_content() {
        let (root, database_path, mut connection, workspace, source, worktree) =
            automatic_workspace_fixture();
        fs::write(worktree.join("local-change"), "dirty\n").expect("worktree should become dirty");
        fs::write(workspace.canonical_path.as_path().join("extra"), "extra\n")
            .expect("workspace should contain extra content");

        let report = execute(
            &mut connection,
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
    fn normal_gc_skips_a_workspace_that_is_dirty_during_scan() {
        let (root, database_path, mut connection, workspace, source, worktree) =
            automatic_workspace_fixture();
        fs::write(worktree.join("local-change"), "dirty\n").expect("worktree should become dirty");

        let report = execute(
            &mut connection,
            "30d".parse().expect("duration should parse"),
            false,
        )
        .expect("GC execution should complete with a skip");
        assert!(report.reclaimed.is_empty());
        assert!(report.failed.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(
            report.skipped[0].reason,
            GcCandidateReason::WorktreeMismatch
        );
        assert!(workspace.canonical_path.as_path().exists());
        assert_eq!(
            crate::storage::find_workspace(&mut connection, &workspace.id)
                .expect("workspace lookup should succeed")
                .state,
            WorkspaceState::Ready
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
