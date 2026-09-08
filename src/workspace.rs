use std::fs;
use std::path::PathBuf;

use diesel::sqlite::SqliteConnection;
use serde::Serialize;
use snafu::{ResultExt, Snafu};

use crate::claim::WorkspaceClaim;
use crate::domain::{
    CanonicalPath, ClaimId, JsonDocument, LeaseId, OperationState, OriginRepositoryId, PoolId,
    RepoWorktreeId, RepoWorktreeState, Timestamp, WorkspaceId, WorkspaceManagementMode,
    WorkspaceState,
};
use crate::git::{self, GitError};
use crate::naming::{self, NamingError, WorktreePlan};
use crate::reconciliation::{self, ReconciliationError};
use crate::storage::{
    append_event, begin_operation, ensure_origin_repository, finalize_automatic_creation,
    finalize_creation as finalize_persisted_creation, find_workspace, find_workspace_by_path,
    find_workspace_claim, find_workspace_claim_by_id, find_workspace_pool,
    insert_managed_workspace, insert_repo_worktree, insert_workspace_claim,
    insert_workspace_pool_repositories, persist_operation_intent, persist_operation_step_intent,
    record_operation_transition, record_repo_worktree_transition, record_workspace_acquire,
    record_workspace_acquire_failure, record_workspace_release, record_workspace_release_rejection,
    record_workspace_transition, record_worktree_step_result, release_workspace_claim,
    try_begin_operation, with_retrying_short_transaction, EventDraft, NewManagedWorkspace,
    NewRepoWorktree, NewWorkspacePoolRepository, OperationIntent, OperationIntentError,
    RepoWorktreeRow, TransitionMetadata, WorkspaceRow,
};
use crate::validation::{self, ValidationError};

#[derive(Debug, Clone)]
pub struct CreateRequest {
    pub workspace_path: PathBuf,
    pub repositories: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct AutomaticCreateRequest {
    pub repositories: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct AutomaticAllocationPlan {
    pub workspace_root: CanonicalPath,
    pub repositories: Vec<AutomaticRepositoryPlan>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct AutomaticRepositoryPlan {
    pub source_path: CanonicalPath,
    pub repository_identity: CanonicalPath,
    pub head: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomaticClaimResult {
    pub workspace_path: CanonicalPath,
    pub pool_id: PoolId,
    pub claim_id: ClaimId,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseResult {
    pub workspace_path: CanonicalPath,
    pub claim_id: ClaimId,
    pub released_at: Timestamp,
}

#[derive(Debug, Clone)]
pub enum ReleaseTarget {
    WorkspacePath(CanonicalPath),
    CurrentDirectory(CanonicalPath),
    ClaimId(ClaimId),
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct CreationPlan {
    pub workspace_path: CanonicalPath,
    pub repositories: Vec<RepositoryPlan>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct RepositoryPlan {
    pub source_path: CanonicalPath,
    pub repository_identity: CanonicalPath,
    pub worktree_path: PathBuf,
    pub head: String,
}

#[derive(Debug, Clone)]
pub struct TrackedRepository {
    pub id: RepoWorktreeId,
    pub origin_repository_id: OriginRepositoryId,
    pub plan: RepositoryPlan,
}

#[derive(Debug, Clone)]
pub struct CreationContext {
    pub workspace_id: WorkspaceId,
    pub operation_id: crate::domain::OperationId,
    pub(crate) lease_id: LeaseId,
    pub plan: CreationPlan,
    pub repositories: Vec<TrackedRepository>,
}

pub fn prepare_automatic(
    request: &AutomaticCreateRequest,
) -> Result<AutomaticAllocationPlan, WorkspaceError> {
    let repositories = validation::validate_repositories(&request.repositories)?;
    let mut plans = Vec::with_capacity(repositories.len());
    for source_path in repositories {
        let info = git::inspect_upstream_repository(&source_path)?;
        plans.push(AutomaticRepositoryPlan {
            source_path: info.root,
            repository_identity: info.common_dir,
            head: info.head,
        });
    }
    let workspace_root = CanonicalPath::from_absolute(
        crate::paths::managed_workspace_directory().context(PathSnafu)?,
    )
    .map_err(|source| WorkspaceError::Validation {
        source: ValidationError::Canonicalize { source },
    })?;

    Ok(AutomaticAllocationPlan {
        workspace_root,
        repositories: plans,
    })
}

fn resolve_repository_set(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
) -> Result<crate::pool::RepositorySetKey, WorkspaceError> {
    let repository_ids = plan
        .repositories
        .iter()
        .map(|repository| {
            ensure_origin_repository(
                connection,
                &repository.repository_identity,
                &repository.source_path,
            )
            .map(|origin| origin.id)
            .context(DatabaseSnafu)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(crate::pool::RepositorySetKey::from_repository_ids(
        &repository_ids,
    ))
}

pub fn allocate_automatic_workspace(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
) -> Result<AutomaticClaimResult, WorkspaceError> {
    for candidate in list_idle_automatic_candidates(connection, plan)? {
        match acquire_automatic_candidate(connection, plan, &candidate) {
            Ok(result) => return Ok(result),
            Err(error) if is_retryable_allocation_error(&error) => continue,
            Err(error) => return Err(error),
        }
    }
    provision_automatic(connection, plan)
}

fn list_idle_automatic_candidates(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
) -> Result<Vec<crate::storage::WorkspaceRow>, WorkspaceError> {
    let repository_set = resolve_repository_set(connection, plan)?;
    let Some(pool) = find_workspace_pool(connection, &repository_set).context(DatabaseSnafu)?
    else {
        return Ok(Vec::new());
    };
    let candidates = crate::storage::list_automatic_workspace_candidates(connection, &pool.id)
        .context(DatabaseSnafu)?;
    let mut idle_candidates = Vec::new();
    for workspace in candidates {
        match reconciliation::recover_expired_operation(connection, &workspace.id)
            .context(ReconciliationSnafu)?
        {
            reconciliation::RecoveryOutcome::LeaseActive => continue,
            reconciliation::RecoveryOutcome::NoRunningOperation
            | reconciliation::RecoveryOutcome::Succeeded
            | reconciliation::RecoveryOutcome::RolledBack
            | reconciliation::RecoveryOutcome::Failed => {}
        }
        if crate::storage::find_running_operation(connection, &workspace.id)
            .context(DatabaseSnafu)?
            .is_some()
        {
            continue;
        }
        if find_workspace_claim(connection, &workspace.id)
            .context(DatabaseSnafu)?
            .is_some()
        {
            continue;
        }
        idle_candidates.push(workspace);
    }
    let mut candidates = idle_candidates;
    candidates.sort_by(|left, right| {
        let left_idle = left.last_released_at.as_ref().unwrap_or(&left.created_at);
        let right_idle = right.last_released_at.as_ref().unwrap_or(&right.created_at);
        left_idle
            .cmp(right_idle)
            .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
    });
    Ok(candidates)
}

fn is_retryable_allocation_error(error: &WorkspaceError) -> bool {
    match error {
        WorkspaceError::OperationActive { workspace_id: _ }
        | WorkspaceError::NotAutomatic { path: _ }
        | WorkspaceError::NotReusable { path: _ }
        | WorkspaceError::ClaimActive { workspace_id: _ }
        | WorkspaceError::Database {
            source: diesel::result::Error::NotFound,
        }
        | WorkspaceError::Database {
            source:
                diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::UniqueViolation,
                    _,
                ),
        } => true,
        WorkspaceError::Database { source: error } => is_retryable_database_error(error),
        _ => false,
    }
}

fn is_retryable_database_error(error: &diesel::result::Error) -> bool {
    match error {
        diesel::result::Error::DatabaseError(_, information) => {
            information.message().contains("locked")
        }
        _ => false,
    }
}

pub fn acquire_automatic_candidate(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
    candidate: &crate::storage::WorkspaceRow,
) -> Result<AutomaticClaimResult, WorkspaceError> {
    let intent_json = JsonDocument::from_serializable(plan)?;
    let intent = OperationIntent::new(
        candidate.id,
        "acquire",
        Timestamp::after_seconds(300),
        "acquire workspace claim",
        intent_json,
    );
    let lease_id = intent.lease_id;
    let operation = begin_operation(connection, &intent).map_err(map_operation_error)?;
    let boundary = match reconciliation::reconcile_workspace_for_access_with_lease(
        connection,
        &candidate.id,
        &operation.id,
        &lease_id,
    ) {
        Ok(boundary) => boundary,
        Err(error) => {
            let primary = WorkspaceError::Reconciliation { source: error };
            fail_operation(connection, &lease_id, &primary);
            return Err(primary);
        }
    };
    if boundary.workspace.management_mode != crate::domain::WorkspaceManagementMode::Automatic {
        let primary = WorkspaceError::NotAutomatic {
            path: candidate.canonical_path.clone(),
        };
        fail_operation(connection, &lease_id, &primary);
        return Err(primary);
    }
    let Some(pool_id) = boundary.workspace.pool_id else {
        let primary = WorkspaceError::RepositorySetMismatch {
            workspace_id: candidate.id,
        };
        fail_operation(connection, &lease_id, &primary);
        return Err(primary);
    };
    if boundary.claim.is_some() {
        let primary = WorkspaceError::ClaimActive {
            workspace_id: candidate.id,
        };
        fail_operation(connection, &lease_id, &primary);
        return Err(primary);
    }
    if boundary.summary.workspace_state != WorkspaceState::Ready {
        let primary = WorkspaceError::NotReusable {
            path: candidate.canonical_path.clone(),
        };
        fail_operation(connection, &lease_id, &primary);
        return Err(primary);
    }
    if let Err(primary) = align_acquired_worktrees(connection, candidate, &lease_id, plan) {
        fail_operation(connection, &lease_id, &primary);
        return Err(primary);
    }

    let claim = WorkspaceClaim::new(candidate.id);
    let details_json = acquire_details(&claim, pool_id);
    let record_result =
        record_workspace_acquire(connection, &lease_id, &claim, Some(details_json.clone()));
    if let Err(error) = record_result {
        let primary = WorkspaceError::Database { source: error };
        fail_operation(connection, &lease_id, &primary);
        return Err(primary);
    }

    let final_boundary = match reconciliation::reconcile_workspace_for_access_with_lease(
        connection,
        &candidate.id,
        &operation.id,
        &lease_id,
    ) {
        Ok(boundary) => boundary,
        Err(error) => {
            let primary = WorkspaceError::Reconciliation { source: error };
            return Err(fail_acquire(
                connection,
                &lease_id,
                &candidate.id,
                &claim.id,
                primary,
                Some(details_json),
            ));
        }
    };
    if final_boundary.summary.workspace_state != WorkspaceState::Ready
        || final_boundary.claim.as_ref().map(|value| value.id) != Some(claim.id)
    {
        let primary = WorkspaceError::NotReusable {
            path: candidate.canonical_path.clone(),
        };
        return Err(fail_acquire(
            connection,
            &lease_id,
            &candidate.id,
            &claim.id,
            primary,
            Some(details_json),
        ));
    }

    if let Err(error) = record_operation_transition(
        connection,
        &lease_id,
        OperationState::Succeeded,
        TransitionMetadata::new("operation_succeeded", "trees")
            .with_pending_step("acquire complete")
            .with_details(details_json.clone()),
    ) {
        let primary = WorkspaceError::Database { source: error };
        fail_operation(connection, &lease_id, &primary);
        return Err(primary);
    }

    Ok(AutomaticClaimResult {
        workspace_path: candidate.canonical_path.clone(),
        pool_id,
        claim_id: claim.id,
    })
}

fn align_acquired_worktrees(
    connection: &mut SqliteConnection,
    workspace: &WorkspaceRow,
    lease_id: &LeaseId,
    plan: &AutomaticAllocationPlan,
) -> Result<(), WorkspaceError> {
    let alignments = prepare_worktree_alignments(connection, workspace, |repository, _| {
        let Some(target) = plan
            .repositories
            .iter()
            .find(|target| target.repository_identity == repository.repository_identity)
        else {
            return Err(WorkspaceError::RepositorySetMismatch {
                workspace_id: workspace.id,
            });
        };
        Ok(target.head.clone())
    })?;
    if alignments.len() != plan.repositories.len() {
        return Err(WorkspaceError::RepositorySetMismatch {
            workspace_id: workspace.id,
        });
    }

    execute_worktree_alignments(connection, lease_id, alignments)
}

fn acquire_details(claim: &WorkspaceClaim, pool_id: PoolId) -> JsonDocument {
    JsonDocument::from_serializable(&serde_json::json!({
        "pool_id": pool_id,
        "claim": claim,
    }))
    .expect("acquire details should serialize")
}

fn map_operation_error(error: OperationIntentError) -> WorkspaceError {
    match error {
        OperationIntentError::WorkspaceBusy { workspace_id } => {
            WorkspaceError::OperationActive { workspace_id }
        }
        OperationIntentError::Database { source } => WorkspaceError::Database { source },
    }
}

fn fail_operation(connection: &mut SqliteConnection, lease_id: &LeaseId, error: &WorkspaceError) {
    let _ = record_operation_transition(
        connection,
        lease_id,
        OperationState::Failed,
        TransitionMetadata::new("operation_failed", "trees")
            .with_pending_step("operation failed")
            .with_error(error_document(error)),
    );
}

fn fail_acquire(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    workspace_id: &WorkspaceId,
    claim_id: &ClaimId,
    primary: WorkspaceError,
    details_json: Option<JsonDocument>,
) -> WorkspaceError {
    let error_json = error_document(&primary);
    match record_workspace_acquire_failure(
        connection,
        lease_id,
        workspace_id,
        claim_id,
        details_json,
        error_json,
    ) {
        Ok(()) => primary,
        Err(error) => WorkspaceError::Rollback {
            primary: Box::new(primary),
            rollback: Box::new(WorkspaceError::Database { source: error }),
        },
    }
}

pub fn release_automatic_workspace_by_target(
    connection: &mut SqliteConnection,
    target: ReleaseTarget,
) -> Result<ReleaseResult, WorkspaceError> {
    let (workspace_path, claim_id) = match target {
        ReleaseTarget::WorkspacePath(path) => {
            release_identity_for_workspace_path(connection, &path)?
        }
        ReleaseTarget::CurrentDirectory(path) => {
            release_identity_for_current_directory(connection, &path)?
        }
        ReleaseTarget::ClaimId(claim_id) => release_identity_for_claim(connection, claim_id)?,
    };
    release_automatic_workspace(connection, &workspace_path, claim_id)
}

fn release_identity_for_workspace_path(
    connection: &mut SqliteConnection,
    workspace_path: &CanonicalPath,
) -> Result<(CanonicalPath, ClaimId), WorkspaceError> {
    let workspace = find_workspace_by_path(connection, workspace_path)
        .context(DatabaseSnafu)?
        .ok_or_else(|| WorkspaceError::WorkspaceNotFound {
            path: workspace_path.clone(),
        })?;
    release_identity_for_workspace(connection, workspace)
}

fn release_identity_for_current_directory(
    connection: &mut SqliteConnection,
    current_directory: &CanonicalPath,
) -> Result<(CanonicalPath, ClaimId), WorkspaceError> {
    for ancestor in current_directory.as_path().ancestors() {
        let path = CanonicalPath::from_absolute(ancestor)
            .expect("an ancestor of an absolute path should be absolute");
        if let Some(workspace) = find_workspace_by_path(connection, &path).context(DatabaseSnafu)? {
            return release_identity_for_workspace(connection, workspace);
        }
    }
    Err(WorkspaceError::WorkspaceNotFound {
        path: current_directory.clone(),
    })
}

fn release_identity_for_claim(
    connection: &mut SqliteConnection,
    claim_id: ClaimId,
) -> Result<(CanonicalPath, ClaimId), WorkspaceError> {
    let claim = match find_workspace_claim_by_id(connection, &claim_id) {
        Ok(claim) => claim,
        Err(diesel::result::Error::NotFound) => {
            return Err(WorkspaceError::ClaimNotFound { claim_id });
        }
        Err(error) => return Err(WorkspaceError::Database { source: error }),
    };
    let workspace = find_workspace(connection, &claim.workspace_id).context(DatabaseSnafu)?;
    if workspace.management_mode != WorkspaceManagementMode::Automatic {
        return Err(WorkspaceError::NotAutomatic {
            path: workspace.canonical_path,
        });
    }
    Ok((workspace.canonical_path, claim.id))
}

fn release_identity_for_workspace(
    connection: &mut SqliteConnection,
    workspace: WorkspaceRow,
) -> Result<(CanonicalPath, ClaimId), WorkspaceError> {
    if workspace.management_mode != WorkspaceManagementMode::Automatic {
        return Err(WorkspaceError::NotAutomatic {
            path: workspace.canonical_path,
        });
    }
    let claim = find_workspace_claim(connection, &workspace.id)
        .context(DatabaseSnafu)?
        .ok_or_else(|| WorkspaceError::WorkspaceUnclaimed {
            path: workspace.canonical_path.clone(),
        })?;
    Ok((workspace.canonical_path, claim.id))
}

pub fn release_automatic_workspace(
    connection: &mut SqliteConnection,
    workspace_path: &CanonicalPath,
    claim_id: ClaimId,
) -> Result<ReleaseResult, WorkspaceError> {
    let workspace = validate_release_target(connection, workspace_path, claim_id)?;

    let intent_json = JsonDocument::from_serializable(&serde_json::json!({
        "workspace_path": workspace_path,
        "claim_id": claim_id,
    }))?;
    let intent = OperationIntent::new(
        workspace.id,
        "release",
        Timestamp::after_seconds(300),
        "release workspace claim",
        intent_json,
    );
    let lease_id = intent.lease_id;
    let operation = try_begin_operation(connection, &intent).map_err(map_operation_error)?;
    let boundary = match reconciliation::reconcile_workspace_for_access_with_lease(
        connection,
        &workspace.id,
        &operation.id,
        &lease_id,
    ) {
        Ok(boundary) => boundary,
        Err(error) => {
            let primary = WorkspaceError::Reconciliation { source: error };
            fail_operation(connection, &lease_id, &primary);
            return Err(primary);
        }
    };
    if boundary.workspace.management_mode != WorkspaceManagementMode::Automatic {
        let primary = WorkspaceError::NotAutomatic {
            path: boundary.workspace.canonical_path,
        };
        fail_operation(connection, &lease_id, &primary);
        return Err(primary);
    }
    let Some(active_claim) = boundary.claim.as_ref() else {
        let primary = WorkspaceError::ClaimNotFound { claim_id };
        fail_operation(connection, &lease_id, &primary);
        return Err(primary);
    };
    if active_claim.id != claim_id {
        let primary = WorkspaceError::ClaimNotFound { claim_id };
        fail_operation(connection, &lease_id, &primary);
        return Err(primary);
    }
    let details_json = release_details(workspace_path, claim_id);
    if let Err(primary) = align_release_worktrees(connection, &workspace, &lease_id) {
        return Err(fail_release(
            connection,
            &lease_id,
            &workspace.id,
            primary,
            Some(details_json),
        ));
    }
    let final_boundary = match reconciliation::reconcile_workspace_for_access_with_lease(
        connection,
        &workspace.id,
        &operation.id,
        &lease_id,
    ) {
        Ok(boundary) => boundary,
        Err(error) => {
            let primary = WorkspaceError::Reconciliation { source: error };
            return Err(fail_release(
                connection,
                &lease_id,
                &workspace.id,
                primary,
                Some(details_json),
            ));
        }
    };
    if final_boundary.summary.workspace_state != WorkspaceState::Ready {
        let primary = WorkspaceError::NotReusable {
            path: workspace.canonical_path.clone(),
        };
        return Err(fail_release(
            connection,
            &lease_id,
            &workspace.id,
            primary,
            Some(details_json),
        ));
    }
    if let Err(error) = record_workspace_release(
        connection,
        &lease_id,
        &workspace.id,
        &claim_id,
        Some(details_json),
    ) {
        let primary = WorkspaceError::Database { source: error };
        fail_operation(connection, &lease_id, &primary);
        return Err(primary);
    }
    let released_workspace = find_workspace(connection, &workspace.id).context(DatabaseSnafu)?;
    let released_at =
        released_workspace
            .last_released_at
            .ok_or_else(|| WorkspaceError::Database {
                source: diesel::result::Error::NotFound,
            })?;
    Ok(ReleaseResult {
        workspace_path: released_workspace.canonical_path,
        claim_id,
        released_at,
    })
}

fn validate_release_target(
    connection: &mut SqliteConnection,
    workspace_path: &CanonicalPath,
    claim_id: ClaimId,
) -> Result<WorkspaceRow, WorkspaceError> {
    let workspace = find_workspace_by_path(connection, workspace_path)
        .context(DatabaseSnafu)?
        .ok_or_else(|| WorkspaceError::WorkspaceNotFound {
            path: workspace_path.clone(),
        })?;
    if workspace.management_mode != WorkspaceManagementMode::Automatic {
        return Err(WorkspaceError::NotAutomatic {
            path: workspace.canonical_path,
        });
    }
    let claim_matches = find_workspace_claim(connection, &workspace.id)
        .context(DatabaseSnafu)?
        .is_some_and(|claim| claim.id == claim_id);
    if !claim_matches {
        return Err(WorkspaceError::ClaimNotFound { claim_id });
    }
    Ok(workspace)
}

struct ReleaseAlignment {
    repository: RepoWorktreeRow,
    target_head: String,
    needs_checkout: bool,
}

fn align_release_worktrees(
    connection: &mut SqliteConnection,
    workspace: &WorkspaceRow,
    lease_id: &LeaseId,
) -> Result<(), WorkspaceError> {
    let alignments =
        prepare_worktree_alignments(connection, workspace, |_, source| Ok(source.head.clone()))?;
    execute_worktree_alignments(connection, lease_id, alignments)
}

fn prepare_worktree_alignments<F>(
    connection: &mut SqliteConnection,
    workspace: &WorkspaceRow,
    mut target_head: F,
) -> Result<Vec<ReleaseAlignment>, WorkspaceError>
where
    F: FnMut(&RepoWorktreeRow, &git::RepositoryInfo) -> Result<String, WorkspaceError>,
{
    let repositories =
        crate::storage::list_repo_worktrees(connection, &workspace.id).context(DatabaseSnafu)?;
    let mut alignments = Vec::with_capacity(repositories.len());

    // Validate every worktree before changing any of them. Release must never
    // discard staged, unstaged, or untracked work from the current claimant.
    for repository in repositories {
        let source = git::inspect_repository(&repository.source_path)?;
        if source.common_dir != repository.repository_identity {
            return Err(WorkspaceError::NotReusable {
                path: workspace.canonical_path.clone(),
            });
        }
        let worktree =
            git::find_worktree(&repository.source_path, repository.worktree_path.as_path())?;
        if worktree.prunable.is_some()
            || !repository.worktree_path.as_path().exists()
            || git::inspect_worktree_identity(repository.worktree_path.as_path())?
                != repository.repository_identity
            || !git::is_worktree_clean(repository.worktree_path.as_path())?
        {
            return Err(WorkspaceError::NotReusable {
                path: workspace.canonical_path.clone(),
            });
        }
        let target_head = target_head(&repository, &source)?;
        let needs_checkout = worktree.head.as_deref() != Some(target_head.as_str())
            || !worktree.detached
            || worktree.branch.is_some();
        alignments.push(ReleaseAlignment {
            repository,
            target_head,
            needs_checkout,
        });
    }

    Ok(alignments)
}

fn execute_worktree_alignments(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    alignments: Vec<ReleaseAlignment>,
) -> Result<(), WorkspaceError> {
    for alignment in alignments {
        if !alignment.needs_checkout {
            continue;
        }
        let details = JsonDocument::from_serializable(&serde_json::json!({
            "head": alignment.target_head,
            "source_path": alignment.repository.source_path,
            "worktree_path": alignment.repository.worktree_path,
        }))
        .expect("worktree alignment details should serialize");
        persist_operation_step_intent(
            connection,
            lease_id,
            format!("align {}", alignment.repository.worktree_path),
            details.clone(),
        )
        .context(DatabaseSnafu)?;
        git::checkout_detached_with_heartbeat(
            alignment.repository.worktree_path.as_path(),
            &alignment.target_head,
            || match crate::storage::renew_operation_lease(connection, lease_id) {
                Ok(true) => Ok(()),
                Ok(false) => Err(GitError::Heartbeat {
                    message: "operation lease is no longer owned".to_owned(),
                }),
                Err(error) => Err(GitError::Heartbeat {
                    message: error.to_string(),
                }),
            },
        )?;
        record_worktree_step_result(
            connection,
            &alignment.repository.id,
            lease_id,
            RepoWorktreeState::Attached,
            Some(alignment.target_head.clone()),
            TransitionMetadata::new("worktree_aligned", "trees").with_details(details),
        )
        .context(DatabaseSnafu)?;
    }
    Ok(())
}

fn release_details(workspace_path: &CanonicalPath, claim_id: ClaimId) -> JsonDocument {
    JsonDocument::from_serializable(&serde_json::json!({
        "workspace_path": workspace_path,
        "claim_id": claim_id,
    }))
    .expect("release details should serialize")
}

fn fail_release(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    workspace_id: &WorkspaceId,
    primary: WorkspaceError,
    details_json: Option<JsonDocument>,
) -> WorkspaceError {
    let error_json = error_document(&primary);
    match record_workspace_release_rejection(
        connection,
        lease_id,
        workspace_id,
        details_json,
        error_json,
    ) {
        Ok(()) => primary,
        Err(error) => WorkspaceError::Rollback {
            primary: Box::new(primary),
            rollback: Box::new(WorkspaceError::Database { source: error }),
        },
    }
}

pub fn provision_automatic(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
) -> Result<AutomaticClaimResult, WorkspaceError> {
    provision_automatic_new(connection, plan)
}

fn provision_automatic_new(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
) -> Result<AutomaticClaimResult, WorkspaceError> {
    if plan.repositories.is_empty() {
        return Err(WorkspaceError::Validation {
            source: ValidationError::NoRepositories,
        });
    }
    for repository in &plan.repositories {
        if plan
            .workspace_root
            .as_path()
            .starts_with(repository.source_path.as_path())
        {
            return Err(WorkspaceError::Validation {
                source: ValidationError::WorkspaceInsideRepository {
                    workspace: plan.workspace_root.as_path().to_owned(),
                    repository: repository.source_path.as_path().to_owned(),
                },
            });
        }
    }
    fs::create_dir_all(plan.workspace_root.as_path()).map_err(|source| WorkspaceError::Io {
        path: plan.workspace_root.as_path().to_owned(),
        source,
    })?;
    let mut normalized_plan = plan.clone();
    normalized_plan.workspace_root =
        validation::resolve_workspace_path(plan.workspace_root.as_path())?;

    let repository_set = resolve_repository_set(connection, &normalized_plan)?;
    let pool = crate::storage::ensure_workspace_pool(connection, &repository_set)
        .context(DatabaseSnafu)?;

    let (workspace_id, workspace_path) =
        next_generated_workspace(connection, normalized_plan.workspace_root.as_path())?;
    let creation_plan = automatic_creation_plan(&normalized_plan, workspace_path.clone())?;
    let claim = WorkspaceClaim::new(workspace_id);
    let intent_json = JsonDocument::from_serializable(&serde_json::json!({
        "allocation": normalized_plan,
        "workspace_id": workspace_id,
        "workspace_path": workspace_path,
        "claim_id": claim.id,
    }))?;
    let context = initialize_creation_with_mode(
        connection,
        creation_plan,
        WorkspaceManagementMode::Automatic,
        Some(pool.id),
        Some(&claim),
        intent_json,
    )?;

    if let Err(error) = reconciliation::reconcile_workspace_with_lease(
        connection,
        &context.workspace_id,
        &context.operation_id,
        &context.lease_id,
    ) {
        let primary = WorkspaceError::Reconciliation { source: error };
        return fail_creation(connection, &context, &[], None, Some(&claim), primary)
            .map(|_| unreachable!("automatic provisioning rollback always fails the operation"));
    }
    execute_creation_with_claim(connection, &context, Some(&claim))?;
    let summary = match reconciliation::reconcile_workspace_with_lease(
        connection,
        &context.workspace_id,
        &context.operation_id,
        &context.lease_id,
    ) {
        Ok(summary) => summary,
        Err(error) => {
            let primary = WorkspaceError::Reconciliation { source: error };
            let completed = context.repositories.iter().collect::<Vec<_>>();
            return fail_creation(
                connection,
                &context,
                &completed,
                None,
                Some(&claim),
                primary,
            )
            .map(|_| unreachable!("automatic provisioning rollback always fails the operation"));
        }
    };
    if summary.workspace_state != WorkspaceState::Ready {
        let primary = WorkspaceError::NotReusable {
            path: context.plan.workspace_path.clone(),
        };
        let completed = context.repositories.iter().collect::<Vec<_>>();
        return fail_creation(
            connection,
            &context,
            &completed,
            None,
            Some(&claim),
            primary,
        )
        .map(|_| unreachable!("automatic provisioning rollback always fails the operation"));
    }

    let details_json = acquire_details(&claim, pool.id);
    if let Err(error) = finalize_automatic_creation(
        connection,
        &context.workspace_id,
        &context.lease_id,
        &claim,
        Some(details_json),
    ) {
        let primary = WorkspaceError::Database { source: error };
        let completed = context.repositories.iter().collect::<Vec<_>>();
        return fail_creation(
            connection,
            &context,
            &completed,
            None,
            Some(&claim),
            primary,
        )
        .map(|_| unreachable!("automatic provisioning rollback always fails the operation"));
    }

    Ok(AutomaticClaimResult {
        workspace_path: context.plan.workspace_path.clone(),
        pool_id: pool.id,
        claim_id: claim.id,
    })
}

fn next_generated_workspace(
    connection: &mut SqliteConnection,
    workspace_root: &std::path::Path,
) -> Result<(WorkspaceId, CanonicalPath), WorkspaceError> {
    for _ in 0..8 {
        let workspace_id = WorkspaceId::new();
        let workspace_path =
            crate::paths::generated_workspace_path_below(workspace_root, &workspace_id);
        if workspace_path.exists() {
            continue;
        }
        let workspace_path = CanonicalPath::from_absolute(workspace_path).map_err(|source| {
            WorkspaceError::Validation {
                source: ValidationError::Canonicalize { source },
            }
        })?;
        if find_workspace_by_path(connection, &workspace_path)
            .context(DatabaseSnafu)?
            .is_none()
        {
            return Ok((workspace_id, workspace_path));
        }
    }
    Err(WorkspaceError::GeneratedPathUnavailable {
        path: workspace_root.to_owned(),
    })
}

fn automatic_creation_plan(
    plan: &AutomaticAllocationPlan,
    workspace_path: CanonicalPath,
) -> Result<CreationPlan, WorkspaceError> {
    let repositories = plan
        .repositories
        .iter()
        .map(|repository| repository.source_path.as_path().to_owned())
        .collect::<Vec<_>>();
    let input = validation::validate_create(workspace_path.as_path(), &repositories)?;
    let worktrees = naming::plan_worktrees(&input)?;
    let repositories = worktrees
        .into_iter()
        .zip(&plan.repositories)
        .map(|(worktree, repository)| RepositoryPlan {
            source_path: repository.source_path.clone(),
            repository_identity: repository.repository_identity.clone(),
            worktree_path: worktree.worktree_path,
            head: repository.head.clone(),
        })
        .collect();

    Ok(CreationPlan {
        workspace_path: input.workspace_path,
        repositories,
    })
}

pub fn prepare_create(request: &CreateRequest) -> Result<CreationPlan, WorkspaceError> {
    let input = validation::validate_create(&request.workspace_path, &request.repositories)?;
    let worktrees = naming::plan_worktrees(&input)?;
    let repositories = worktrees
        .into_iter()
        .map(repository_plan)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CreationPlan {
        workspace_path: input.workspace_path,
        repositories,
    })
}

fn repository_plan(plan: WorktreePlan) -> Result<RepositoryPlan, WorkspaceError> {
    let info = git::inspect_upstream_repository(&plan.repository)?;
    Ok(RepositoryPlan {
        source_path: info.root,
        repository_identity: info.common_dir,
        worktree_path: plan.worktree_path,
        head: info.head,
    })
}

pub fn initialize_creation(
    connection: &mut SqliteConnection,
    plan: CreationPlan,
) -> Result<CreationContext, WorkspaceError> {
    let intent_json = JsonDocument::from_serializable(&plan)?;
    initialize_creation_with_mode(
        connection,
        plan,
        WorkspaceManagementMode::Manual,
        None,
        None,
        intent_json,
    )
}

fn initialize_creation_with_mode(
    connection: &mut SqliteConnection,
    plan: CreationPlan,
    management_mode: WorkspaceManagementMode,
    pool_id: Option<PoolId>,
    claim: Option<&WorkspaceClaim>,
    intent_json: JsonDocument,
) -> Result<CreationContext, WorkspaceError> {
    if find_workspace_by_path(connection, &plan.workspace_path)
        .context(DatabaseSnafu)?
        .is_some()
    {
        return Err(WorkspaceError::AlreadyManaged {
            path: plan.workspace_path,
        });
    }

    let workspace_id = claim.map(|claim| claim.workspace_id).unwrap_or_default();
    let operation_intent = OperationIntent::new(
        workspace_id,
        "create",
        Timestamp::after_seconds(300),
        "prepare worktrees",
        intent_json,
    );
    let repositories = plan
        .repositories
        .iter()
        .map(|repository| {
            let origin = ensure_origin_repository(
                connection,
                &repository.repository_identity,
                &repository.source_path,
            )
            .context(DatabaseSnafu)?;
            if let Some(pool_id) = pool_id {
                insert_workspace_pool_repositories(
                    connection,
                    &[NewWorkspacePoolRepository {
                        pool_id,
                        repository_id: origin.id,
                    }],
                )
                .context(DatabaseSnafu)?;
            }
            Ok(TrackedRepository {
                id: RepoWorktreeId::new(),
                origin_repository_id: origin.id,
                plan: repository.clone(),
            })
        })
        .collect::<Result<Vec<_>, WorkspaceError>>()?;
    let now = Timestamp::now();

    with_retrying_short_transaction(connection, |connection| {
        insert_managed_workspace(
            connection,
            &NewManagedWorkspace {
                id: workspace_id,
                canonical_path: plan.workspace_path.clone(),
                state: WorkspaceState::Creating,
                created_at: now.clone(),
                updated_at: now.clone(),
                last_reconciled_at: None,
                management_mode,
                pool_id,
                last_released_at: None,
                reclaimed_at: None,
            },
        )?;
        for repository in &repositories {
            insert_repo_worktree(
                connection,
                &NewRepoWorktree {
                    id: repository.id,
                    workspace_id,
                    origin_repository_id: repository.origin_repository_id,
                    worktree_path: CanonicalPath::from_absolute(&repository.plan.worktree_path)
                        .map_err(|error| {
                            diesel::result::Error::QueryBuilderError(Box::new(error))
                        })?,
                    state: RepoWorktreeState::Pending,
                    last_head: Some(repository.plan.head.clone()),
                    last_observed_at: now.clone(),
                },
            )?;
        }
        persist_operation_intent(connection, &operation_intent)?;
        if let Some(claim) = claim {
            insert_workspace_claim(connection, &crate::storage::NewWorkspaceClaim::from(claim))?;
        }

        append_event(
            connection,
            &EventDraft {
                operation_id: operation_intent.id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace_id.to_string(),
                event_type: "workspace_created".to_owned(),
                source: "trees".to_owned(),
                occurred_at: now.clone(),
                previous_state: None,
                current_state: Some(WorkspaceState::Creating.to_string()),
                details_json: None,
                error_json: None,
            },
        )?;
        for repository in &repositories {
            append_event(
                connection,
                &EventDraft {
                    operation_id: operation_intent.id,
                    entity_type: "repo_worktree".to_owned(),
                    entity_id: repository.id.to_string(),
                    event_type: "worktree_planned".to_owned(),
                    source: "trees".to_owned(),
                    occurred_at: now.clone(),
                    previous_state: None,
                    current_state: Some(RepoWorktreeState::Pending.to_string()),
                    details_json: None,
                    error_json: None,
                },
            )?;
        }
        Ok::<(), diesel::result::Error>(())
    })
    .context(DatabaseSnafu)?;

    Ok(CreationContext {
        workspace_id,
        operation_id: operation_intent.id,
        lease_id: operation_intent.lease_id,
        plan,
        repositories,
    })
}

pub fn execute_creation(
    connection: &mut SqliteConnection,
    context: &CreationContext,
) -> Result<(), WorkspaceError> {
    execute_creation_with_claim(connection, context, None)
}

fn execute_creation_with_claim(
    connection: &mut SqliteConnection,
    context: &CreationContext,
    claim: Option<&WorkspaceClaim>,
) -> Result<(), WorkspaceError> {
    // Git and filesystem steps can be slow; their intent and result writes are
    // deliberately split into short transactions around each external step.
    if !workspace_is_worktree_root(&context.plan) {
        if let Err(source) = fs::create_dir(&context.plan.workspace_path) {
            let primary = WorkspaceError::Io {
                path: context.plan.workspace_path.clone().into_path_buf(),
                source,
            };
            return fail_creation(connection, context, &[], None, claim, primary);
        }
    }

    let mut completed = Vec::new();
    for repository in &context.repositories {
        if let Err(primary) = execute_repository_step(connection, context, repository) {
            return fail_creation(
                connection,
                context,
                &completed,
                Some(repository),
                claim,
                primary,
            );
        }
        completed.push(repository);
    }

    Ok(())
}

fn workspace_is_worktree_root(plan: &CreationPlan) -> bool {
    plan.repositories.len() == 1
        && plan.repositories[0].worktree_path == plan.workspace_path.as_path()
}

#[derive(Debug, Clone, Serialize)]
pub struct CreationResult {
    pub workspace_path: CanonicalPath,
    pub worktree_paths: Vec<PathBuf>,
}

pub fn create(request: CreateRequest) -> Result<CreationResult, WorkspaceError> {
    let workspace_path = validation::resolve_workspace_path(&request.workspace_path)?;
    let mut connection = crate::database::open_default().context(DatabaseOpenSnafu)?;
    reconcile_before_creation(&mut connection, &workspace_path)?;
    let plan = prepare_create(&request)?;
    create_with_connection(&mut connection, plan)
}

pub fn create_with_connection(
    connection: &mut SqliteConnection,
    plan: CreationPlan,
) -> Result<CreationResult, WorkspaceError> {
    reconcile_before_creation(connection, &plan.workspace_path)?;
    let context = initialize_creation(connection, plan)?;
    reconciliation::reconcile_workspace_with_lease(
        connection,
        &context.workspace_id,
        &context.operation_id,
        &context.lease_id,
    )
    .context(ReconciliationSnafu)?;
    execute_creation(connection, &context)?;
    reconciliation::reconcile_workspace_with_lease(
        connection,
        &context.workspace_id,
        &context.operation_id,
        &context.lease_id,
    )
    .context(ReconciliationSnafu)?;
    finalize_persisted_creation(connection, &context.workspace_id, &context.lease_id)
        .context(DatabaseSnafu)?;
    Ok(CreationResult {
        workspace_path: context.plan.workspace_path,
        worktree_paths: context
            .repositories
            .into_iter()
            .map(|repository| repository.plan.worktree_path)
            .collect(),
    })
}

fn reconcile_before_creation(
    connection: &mut SqliteConnection,
    workspace_path: &CanonicalPath,
) -> Result<(), WorkspaceError> {
    let Some(workspace) =
        find_workspace_by_path(connection, workspace_path).context(DatabaseSnafu)?
    else {
        return Ok(());
    };

    match reconciliation::recover_expired_operation(connection, &workspace.id)
        .context(ReconciliationSnafu)?
    {
        reconciliation::RecoveryOutcome::LeaseActive => Err(WorkspaceError::OperationActive {
            workspace_id: workspace.id,
        }),
        reconciliation::RecoveryOutcome::NoRunningOperation
        | reconciliation::RecoveryOutcome::Succeeded
        | reconciliation::RecoveryOutcome::RolledBack
        | reconciliation::RecoveryOutcome::Failed => Ok(()),
    }
}

fn execute_repository_step(
    connection: &mut SqliteConnection,
    context: &CreationContext,
    repository: &TrackedRepository,
) -> Result<(), WorkspaceError> {
    let intent_json = JsonDocument::from_serializable(&repository.plan)?;
    persist_operation_step_intent(
        connection,
        &context.lease_id,
        format!("attach {}", repository.plan.source_path),
        intent_json,
    )
    .context(DatabaseSnafu)?;
    // The subprocess and lease-renewal callback run outside SQLite
    // transactions; each renewal is an independent short operation update.
    let lease_id = context.lease_id;
    git::add_detached_worktree_at_with_heartbeat(
        &repository.plan.source_path,
        &repository.plan.worktree_path,
        &repository.plan.head,
        || match crate::storage::renew_operation_lease(connection, &lease_id) {
            Ok(true) => Ok(()),
            Ok(false) => Err(GitError::Heartbeat {
                message: "operation lease is no longer owned".to_owned(),
            }),
            Err(error) => Err(GitError::Heartbeat {
                message: error.to_string(),
            }),
        },
    )?;
    let worktree =
        git::find_worktree(&repository.plan.source_path, &repository.plan.worktree_path)?;
    record_worktree_step_result(
        connection,
        &repository.id,
        &context.lease_id,
        RepoWorktreeState::Attached,
        worktree.head,
        TransitionMetadata::new("worktree_attached", "trees")
            .with_pending_step("worktree attached")
            .with_details(JsonDocument::from_serializable(&repository.plan)?),
    )
    .context(DatabaseSnafu)?;
    reconciliation::reconcile_workspace_with_lease(
        connection,
        &context.workspace_id,
        &context.operation_id,
        &context.lease_id,
    )
    .context(ReconciliationSnafu)?;
    Ok(())
}

fn fail_creation(
    connection: &mut SqliteConnection,
    context: &CreationContext,
    completed: &[&TrackedRepository],
    failed: Option<&TrackedRepository>,
    claim: Option<&WorkspaceClaim>,
    primary: WorkspaceError,
) -> Result<(), WorkspaceError> {
    match rollback_creation(connection, context, completed, failed, claim, &primary) {
        Ok(()) => Err(primary),
        Err(rollback) => Err(WorkspaceError::Rollback {
            primary: Box::new(primary),
            rollback: Box::new(rollback),
        }),
    }
}

fn rollback_creation(
    connection: &mut SqliteConnection,
    context: &CreationContext,
    completed: &[&TrackedRepository],
    failed: Option<&TrackedRepository>,
    claim: Option<&WorkspaceClaim>,
    primary: &WorkspaceError,
) -> Result<(), WorkspaceError> {
    let error_json = error_document(primary);
    let mut errors = rollback_repositories(connection, context, completed, failed, &error_json);
    rollback_workspace_directory(connection, context, &mut errors);
    rollback_claim(connection, context, claim, &mut errors);
    finish_rollback(connection, context, error_json, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(WorkspaceError::RollbackFailure { errors })
    }
}

fn rollback_repositories(
    connection: &mut SqliteConnection,
    context: &CreationContext,
    completed: &[&TrackedRepository],
    failed: Option<&TrackedRepository>,
    error_json: &JsonDocument,
) -> Vec<String> {
    let failed_id = failed.map(|repository| repository.id);
    let repositories = failed
        .into_iter()
        .chain(completed.iter().rev().copied())
        .collect::<Vec<_>>();
    let mut errors = Vec::new();
    for repository in repositories {
        errors.extend(rollback_repository(
            connection,
            context,
            repository,
            failed_id == Some(repository.id),
            error_json,
        ));
    }
    errors
}

fn rollback_repository(
    connection: &mut SqliteConnection,
    context: &CreationContext,
    repository: &TrackedRepository,
    is_failed_repository: bool,
    error_json: &JsonDocument,
) -> Vec<String> {
    let mut errors = Vec::new();
    match git::find_worktree(&repository.plan.source_path, &repository.plan.worktree_path) {
        Ok(worktree) if worktree.detached && worktree.branch.is_none() => {
            if let Err(error) = git::remove_worktree_with_heartbeat(
                &repository.plan.source_path,
                &repository.plan.worktree_path,
                || match crate::storage::renew_operation_lease(connection, &context.lease_id) {
                    Ok(true) => Ok(()),
                    Ok(false) => Err(GitError::Heartbeat {
                        message: "operation lease is no longer owned".to_owned(),
                    }),
                    Err(error) => Err(GitError::Heartbeat {
                        message: error.to_string(),
                    }),
                },
            ) {
                errors.push(error.to_string());
            }
        }
        Ok(worktree) => errors.push(format!(
            "refusing to remove branch-attached worktree {} ({:?})",
            worktree.path, worktree.branch
        )),
        Err(GitError::WorktreeNotFound { .. }) => {}
        Err(_error) if is_failed_repository => {}
        Err(error) => errors.push(error.to_string()),
    }
    if let Err(error) = record_repo_worktree_transition(
        connection,
        &repository.id,
        &context.operation_id,
        RepoWorktreeState::Failed,
        None,
        TransitionMetadata::new("worktree_rollback", "trees").with_error(error_json.clone()),
    ) {
        errors.push(error.to_string());
    }
    errors
}

fn rollback_workspace_directory(
    connection: &mut SqliteConnection,
    context: &CreationContext,
    errors: &mut Vec<String>,
) {
    if !context.plan.workspace_path.as_path().exists() {
        return;
    }
    match crate::storage::renew_operation_lease(connection, &context.lease_id) {
        Ok(true) => {
            if let Err(error) = fs::remove_dir(&context.plan.workspace_path) {
                errors.push(error.to_string());
            }
        }
        Ok(false) => errors.push("operation lease is no longer owned".to_owned()),
        Err(error) => errors.push(error.to_string()),
    }
}

fn rollback_claim(
    connection: &mut SqliteConnection,
    context: &CreationContext,
    claim: Option<&WorkspaceClaim>,
    errors: &mut Vec<String>,
) {
    let Some(claim) = claim else {
        return;
    };
    match release_workspace_claim(connection, &context.workspace_id, &claim.id) {
        Ok(true) => {}
        Ok(false) => errors.push("automatic workspace claim was not found".to_owned()),
        Err(error) => errors.push(error.to_string()),
    }
}

fn finish_rollback(
    connection: &mut SqliteConnection,
    context: &CreationContext,
    error_json: JsonDocument,
    errors: &mut Vec<String>,
) {
    let operation_state = if errors.is_empty() {
        OperationState::RolledBack
    } else {
        OperationState::Failed
    };
    let operation_event = if operation_state == OperationState::RolledBack {
        "operation_rolled_back"
    } else {
        "operation_rollback_failed"
    };
    if let Err(error) = record_operation_transition(
        connection,
        &context.lease_id,
        operation_state,
        TransitionMetadata::new(operation_event, "trees")
            .with_pending_step("rollback complete")
            .with_error(error_json.clone()),
    ) {
        errors.push(error.to_string());
    }
    if let Err(error) = record_workspace_transition(
        connection,
        &context.workspace_id,
        &context.operation_id,
        WorkspaceState::Failed,
        TransitionMetadata::new("workspace_creation_failed", "trees").with_error(error_json),
    ) {
        errors.push(error.to_string());
    }
}

fn error_document(error: &WorkspaceError) -> JsonDocument {
    JsonDocument::from_serializable(&serde_json::json!({
        "error": error.to_string(),
    }))
    .expect("JSON error document should serialize")
}

#[derive(Debug, Snafu)]
pub enum WorkspaceError {
    #[snafu(transparent)]
    Validation { source: ValidationError },
    #[snafu(transparent)]
    Naming { source: NamingError },
    #[snafu(transparent)]
    Git { source: GitError },
    #[snafu(display("database operation failed: {source}"))]
    Database { source: diesel::result::Error },
    #[snafu(display("failed to open lifecycle database: {source}"))]
    DatabaseOpen {
        source: crate::database::DatabaseError,
    },
    #[snafu(display("failed to resolve workspace root: {source}"))]
    Path { source: crate::paths::PathError },
    #[snafu(display("reconciliation failed: {source}"))]
    Reconciliation { source: ReconciliationError },
    #[snafu(display("workspace has an active operation: {workspace_id}"))]
    OperationActive { workspace_id: WorkspaceId },
    #[snafu(display("workspace is not managed by the automatic workspace pool: {path}"))]
    NotAutomatic { path: CanonicalPath },
    #[snafu(display("automatic workspace is not reusable: {path}"))]
    NotReusable { path: CanonicalPath },
    #[snafu(display("workspace has an active claim: {workspace_id}"))]
    ClaimActive { workspace_id: WorkspaceId },
    #[snafu(display(
        "could not allocate a generated workspace path below {}",
        path.display()
    ))]
    GeneratedPathUnavailable { path: PathBuf },
    #[snafu(display("workspace claim was not found: {claim_id}"))]
    ClaimNotFound { claim_id: ClaimId },
    #[snafu(display("workspace has no active claim: {path}"))]
    WorkspaceUnclaimed { path: CanonicalPath },
    #[snafu(display("repository set does not match workspace pool: {workspace_id}"))]
    RepositorySetMismatch { workspace_id: WorkspaceId },
    #[snafu(display("managed workspace was not found: {path}"))]
    WorkspaceNotFound { path: CanonicalPath },
    #[snafu(transparent)]
    Json {
        source: crate::domain::JsonDocumentError,
    },
    #[snafu(display("workspace is already managed: {path}"))]
    AlreadyManaged { path: CanonicalPath },
    #[snafu(display("workspace creation failed: {primary}; rollback failed: {rollback}"))]
    Rollback {
        #[snafu(source)]
        primary: Box<WorkspaceError>,
        rollback: Box<WorkspaceError>,
    },
    #[snafu(display("workspace rollback failed: {}", errors.join("; ")))]
    RollbackFailure { errors: Vec<String> },
    #[snafu(display(
        "workspace filesystem operation failed for {}: {source}",
        path.display()
    ))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use diesel::prelude::*;

    use super::*;
    use crate::storage::NewWorkspace;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("trees-workspace-{}", uuid::Uuid::now_v7()))
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

    #[test]
    fn prepares_a_multi_repository_creation_plan() {
        let root = test_root();
        let first = root.join("first");
        let second = root.join("second");
        repository(&first);
        repository(&second);

        let plan = prepare_create(&CreateRequest {
            workspace_path: root.join("workspace"),
            repositories: vec![first, second],
        })
        .expect("creation plan should be prepared");

        assert_eq!(plan.repositories.len(), 2);
        assert!(plan
            .repositories
            .iter()
            .all(|repository| !repository.head.is_empty()));
        assert!(plan.repositories.iter().all(|repository| {
            repository
                .worktree_path
                .starts_with(plan.workspace_path.as_path())
        }));
        let database_path = root.join("state.sqlite");
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");
        let context =
            initialize_creation(&mut connection, plan).expect("creation should initialize");
        execute_creation(&mut connection, &context).expect("creation should execute");
        finalize_persisted_creation(&mut connection, &context.workspace_id, &context.lease_id)
            .expect("creation should finalize");
        assert!(context.plan.workspace_path.as_path().exists());
        assert!(context
            .repositories
            .iter()
            .all(|repository| repository.plan.worktree_path.exists()));
        let workspace =
            crate::storage::find_workspace_by_path(&mut connection, &context.plan.workspace_path)
                .unwrap()
                .unwrap();
        assert_eq!(workspace.state, WorkspaceState::Ready);
        assert_eq!(
            crate::storage::list_repo_worktrees(&mut connection, &context.workspace_id)
                .unwrap()
                .iter()
                .filter(|worktree| worktree.state == RepoWorktreeState::Attached)
                .count(),
            2
        );
        assert_eq!(
            crate::storage::find_operation(&mut connection, &context.operation_id)
                .unwrap()
                .id,
            context.operation_id
        );
        assert_eq!(
            crate::storage::operation_state(&mut connection, &context.operation_id).unwrap(),
            Some(OperationState::Succeeded)
        );
        assert_eq!(
            crate::storage::list_events_for_operation(&mut connection, &context.operation_id)
                .unwrap()
                .len(),
            13
        );
        for repository in &context.repositories {
            crate::git::remove_worktree(
                &repository.plan.source_path,
                &repository.plan.worktree_path,
            )
            .expect("created worktree should be removable");
        }
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn prepares_an_automatic_allocation_plan_without_a_workspace_path() {
        let root = test_root();
        let first = root.join("first");
        let second = root.join("second");
        repository(&first);
        repository(&second);

        let plan = prepare_automatic(&AutomaticCreateRequest {
            repositories: vec![second.clone(), first.clone()],
        })
        .expect("automatic allocation plan should be prepared");
        let expected_root = CanonicalPath::from_absolute(
            crate::paths::managed_workspace_directory().expect("workspace root should resolve"),
        )
        .expect("workspace root should be absolute");

        assert_eq!(plan.workspace_root, expected_root);
        assert_eq!(plan.repositories.len(), 2);
        assert!(plan
            .repositories
            .iter()
            .all(|repository| !repository.head.is_empty()));
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn selects_the_oldest_idle_automatic_candidate_for_a_pool_id() {
        let root = test_root();
        let source = root.join("source");
        repository(&source);
        let plan = prepare_automatic(&AutomaticCreateRequest {
            repositories: vec![source.clone()],
        })
        .expect("automatic allocation plan should be prepared");
        let database_path = root.join("state.sqlite");
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");
        let repository_set = resolve_repository_set(&mut connection, &plan)
            .expect("repository set should be resolved");
        let pool = crate::storage::ensure_workspace_pool(&mut connection, &repository_set)
            .expect("workspace pool should be available");
        let now = Timestamp::parse("2026-01-01T00:00:00Z").unwrap();
        let older = WorkspaceId::new();
        let newer = WorkspaceId::new();
        for (id, name, released_at) in [
            (older, "older", "2026-01-01T00:00:00Z"),
            (newer, "newer", "2026-02-01T00:00:00Z"),
        ] {
            let workspace_path = CanonicalPath::from_absolute(root.join(name))
                .expect("workspace path should be absolute");
            crate::storage::insert_workspace(
                &mut connection,
                &NewWorkspace {
                    id,
                    canonical_path: workspace_path,
                    state: WorkspaceState::Ready,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    last_reconciled_at: None,
                },
            )
            .expect("candidate should be inserted");
            diesel::update(crate::schema::workspaces::table.find(&id))
                .set((
                    crate::schema::workspaces::management_mode
                        .eq(crate::domain::WorkspaceManagementMode::Automatic),
                    crate::schema::workspaces::pool_id.eq(Some(pool.id)),
                    crate::schema::workspaces::last_released_at
                        .eq(Some(Timestamp::parse(released_at).unwrap())),
                ))
                .execute(&mut connection)
                .expect("candidate metadata should be updated");
        }

        let candidate = list_idle_automatic_candidates(&mut connection, &plan)
            .expect("candidate lookup should succeed")
            .into_iter()
            .next()
            .expect("an idle candidate should exist");
        assert_eq!(candidate.id, older);

        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn allocates_an_existing_automatic_pool_slot() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();

        let result = allocate_automatic_workspace(&mut connection, &plan)
            .expect("automatic allocation should succeed");
        assert_eq!(result.workspace_path, candidate.canonical_path);
        assert_eq!(
            result.pool_id,
            candidate.pool_id.expect("candidate pool should exist")
        );
        assert!(
            crate::storage::find_workspace_claim(&mut connection, &candidate.id)
                .expect("claim lookup should succeed")
                .is_some()
        );

        crate::storage::release_workspace_claim(&mut connection, &candidate.id, &result.claim_id)
            .expect("allocated claim should be releasable");
        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    fn automatic_candidate_fixture() -> (
        PathBuf,
        PathBuf,
        SqliteConnection,
        AutomaticAllocationPlan,
        crate::storage::WorkspaceRow,
        PathBuf,
    ) {
        let (root, database_path, connection, plan, candidate, mut worktree_paths) =
            automatic_candidate_fixture_with_sources(&["source"]);
        let worktree_path = worktree_paths
            .pop()
            .expect("candidate should have a worktree");
        (
            root,
            database_path,
            connection,
            plan,
            candidate,
            worktree_path,
        )
    }

    fn automatic_candidate_fixture_with_sources(
        source_names: &[&str],
    ) -> (
        PathBuf,
        PathBuf,
        SqliteConnection,
        AutomaticAllocationPlan,
        crate::storage::WorkspaceRow,
        Vec<PathBuf>,
    ) {
        let root = test_root();
        let sources = source_names
            .iter()
            .map(|name| root.join(name))
            .collect::<Vec<_>>();
        for source in &sources {
            repository(source);
        }
        let database_path = root.join("state.sqlite");
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");
        let created = create_with_connection(
            &mut connection,
            prepare_create(&CreateRequest {
                workspace_path: root.join("workspace"),
                repositories: sources.clone(),
            })
            .expect("manual creation plan should be prepared"),
        )
        .expect("workspace should be created");
        let workspace =
            crate::storage::find_workspace_by_path(&mut connection, &created.workspace_path)
                .expect("workspace lookup should succeed")
                .expect("workspace should exist");
        let mut plan = prepare_automatic(&AutomaticCreateRequest {
            repositories: sources,
        })
        .expect("automatic allocation plan should be prepared");
        plan.workspace_root =
            CanonicalPath::from_absolute(root.clone()).expect("workspace root should be absolute");
        let repository_set = resolve_repository_set(&mut connection, &plan)
            .expect("repository set should be resolved");
        let pool = crate::storage::ensure_workspace_pool(&mut connection, &repository_set)
            .expect("workspace pool should be available");
        diesel::update(crate::schema::workspaces::table.find(workspace.id))
            .set((
                crate::schema::workspaces::management_mode
                    .eq(crate::domain::WorkspaceManagementMode::Automatic),
                crate::schema::workspaces::pool_id.eq(Some(pool.id)),
            ))
            .execute(&mut connection)
            .expect("workspace metadata should be updated");
        let candidate = list_idle_automatic_candidates(&mut connection, &plan)
            .expect("candidate lookup should succeed")
            .into_iter()
            .next()
            .expect("automatic candidate should exist");
        let worktree_paths = crate::storage::list_repo_worktrees(&mut connection, &candidate.id)
            .expect("worktree lookup should succeed")
            .into_iter()
            .map(|repository| repository.worktree_path.into_path_buf())
            .collect();
        (
            root,
            database_path,
            connection,
            plan,
            candidate,
            worktree_paths,
        )
    }

    #[test]
    fn acquires_an_idle_automatic_candidate_and_records_its_claim() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let result = acquire_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("automatic acquire should succeed");

        assert_eq!(result.workspace_path, candidate.canonical_path);
        assert_eq!(
            result.pool_id,
            candidate.pool_id.expect("candidate pool should exist")
        );
        let claim = crate::storage::find_workspace_claim(&mut connection, &candidate.id)
            .expect("claim lookup should succeed")
            .expect("acquire claim should exist");
        assert_eq!(claim.id, result.claim_id);
        assert_eq!(
            crate::storage::find_workspace(&mut connection, &candidate.id)
                .expect("workspace lookup should succeed")
                .state,
            WorkspaceState::Ready
        );
        let acquire_event = crate::schema::lifecycle_events::table
            .filter(crate::schema::lifecycle_events::entity_id.eq(candidate.id.to_string()))
            .filter(crate::schema::lifecycle_events::event_type.eq("workspace_claimed"))
            .select(crate::storage::EventRow::as_select())
            .first(&mut connection)
            .expect("acquire event should exist");
        assert_eq!(
            crate::storage::operation_state(&mut connection, &acquire_event.operation_id)
                .expect("acquire operation state should exist"),
            Some(OperationState::Succeeded)
        );

        crate::storage::release_workspace_claim(&mut connection, &candidate.id, &result.claim_id)
            .expect("acquire claim should be releasable");
        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn recovers_an_expired_operation_before_automatic_allocation() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let stale_intent = OperationIntent::new(
            candidate.id,
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

        let result = allocate_automatic_workspace(&mut connection, &plan)
            .expect("automatic allocation should recover the stale operation");
        assert_eq!(result.workspace_path, candidate.canonical_path);
        assert_eq!(
            crate::storage::operation_state(&mut connection, &stale_operation.id)
                .expect("stale operation state should be queryable"),
            Some(OperationState::Succeeded)
        );

        crate::storage::release_workspace_claim(&mut connection, &candidate.id, &result.claim_id)
            .expect("allocated claim should be releasable");
        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn protects_a_claimed_workspace_from_reclamation() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let acquire = acquire_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("initial automatic acquire should succeed");

        assert!(matches!(
            acquire_automatic_candidate(&mut connection, &plan, &candidate),
            Err(WorkspaceError::ClaimActive { workspace_id }) if workspace_id == candidate.id
        ));
        assert_eq!(
            crate::storage::find_workspace_claim(&mut connection, &candidate.id)
                .expect("claim lookup should succeed")
                .expect("original claim should remain active")
                .id,
            acquire.claim_id
        );

        crate::storage::release_workspace_claim(&mut connection, &candidate.id, &acquire.claim_id)
            .expect("original claim should be releasable");
        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn releases_a_reusable_automatic_workspace_and_updates_its_idle_timestamp() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let acquire = acquire_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("automatic acquire should succeed");

        let result = release_automatic_workspace(
            &mut connection,
            &candidate.canonical_path,
            acquire.claim_id,
        )
        .expect("automatic release should succeed");
        assert_eq!(result.workspace_path, candidate.canonical_path);
        assert_eq!(result.claim_id, acquire.claim_id);
        assert!(
            crate::storage::find_workspace_claim(&mut connection, &candidate.id)
                .expect("claim lookup should succeed")
                .is_none()
        );
        let workspace = crate::storage::find_workspace(&mut connection, &candidate.id)
            .expect("workspace lookup should succeed");
        assert_eq!(workspace.state, WorkspaceState::Ready);
        assert_eq!(workspace.last_released_at, Some(result.released_at));
        assert_eq!(
            crate::storage::list_repo_worktrees(&mut connection, &candidate.id)
                .expect("worktree lookup should succeed")[0]
                .state,
            RepoWorktreeState::Attached
        );
        let release_event = crate::schema::lifecycle_events::table
            .filter(crate::schema::lifecycle_events::entity_id.eq(candidate.id.to_string()))
            .filter(crate::schema::lifecycle_events::event_type.eq("workspace_released"))
            .select(crate::storage::EventRow::as_select())
            .first(&mut connection)
            .expect("release event should exist");
        let operation =
            crate::storage::find_operation(&mut connection, &release_event.operation_id)
                .expect("release operation should exist");
        assert_eq!(operation.kind, "release");
        assert_eq!(
            crate::storage::operation_state(&mut connection, &operation.id)
                .expect("release operation state should exist"),
            Some(OperationState::Succeeded)
        );
        assert!(worktree_path.exists());

        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn release_aligns_a_clean_worktree_to_the_source_repository_head() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let acquire = acquire_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("automatic acquire should succeed");
        run_git(&worktree_path, &["checkout", "-q", "-b", "feature"]);

        let source_path = plan.repositories[0].source_path.as_path();
        fs::write(source_path.join("README"), "updated\n")
            .expect("source repository should be updated");
        run_git(source_path, &["commit", "-qam", "update"]);
        let source_head = git::inspect_repository(&plan.repositories[0].source_path)
            .expect("source repository should be inspectable")
            .head;

        release_automatic_workspace(&mut connection, &candidate.canonical_path, acquire.claim_id)
            .expect("clean changed worktree should be aligned and released");

        let worktree = git::find_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("worktree should remain registered");
        assert!(worktree.detached);
        assert!(worktree.branch.is_none());
        assert_eq!(worktree.head.as_deref(), Some(source_head.as_str()));
        assert!(git::is_worktree_clean(&worktree_path).expect("worktree status should succeed"));
        let repository = crate::storage::list_repo_worktrees(&mut connection, &candidate.id)
            .expect("worktree lookup should succeed")
            .into_iter()
            .next()
            .expect("workspace should have a worktree");
        assert_eq!(repository.state, RepoWorktreeState::Attached);
        assert_eq!(repository.last_head.as_deref(), Some(source_head.as_str()));
        assert!(
            crate::storage::find_workspace_claim(&mut connection, &candidate.id)
                .expect("claim lookup should succeed")
                .is_none()
        );

        git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn releases_by_workspace_path_current_directory_or_claim_id() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();

        let path_claim = acquire_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("path-targeted acquire should succeed");
        let path_result = release_automatic_workspace_by_target(
            &mut connection,
            ReleaseTarget::WorkspacePath(candidate.canonical_path.clone()),
        )
        .expect("path-targeted release should succeed");
        assert_eq!(path_result.claim_id, path_claim.claim_id);

        let id_claim = acquire_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("claim-targeted acquire should succeed");
        let id_result = release_automatic_workspace_by_target(
            &mut connection,
            ReleaseTarget::ClaimId(id_claim.claim_id),
        )
        .expect("claim-targeted release should succeed");
        assert_eq!(id_result.claim_id, id_claim.claim_id);

        let cwd_claim = acquire_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("current-directory-targeted acquire should succeed");
        let nested_directory = worktree_path.join("nested").join("directory");
        fs::create_dir_all(&nested_directory).expect("nested directory should be created");
        let nested_directory = CanonicalPath::resolve(&nested_directory)
            .expect("nested directory should be canonicalized");
        assert!(matches!(
            release_automatic_workspace_by_target(
                &mut connection,
                ReleaseTarget::WorkspacePath(nested_directory.clone()),
            ),
            Err(WorkspaceError::WorkspaceNotFound { path }) if path == nested_directory
        ));
        let cwd_result = release_automatic_workspace_by_target(
            &mut connection,
            ReleaseTarget::CurrentDirectory(nested_directory),
        )
        .expect("current-directory-targeted release should succeed");
        assert_eq!(cwd_result.claim_id, cwd_claim.claim_id);

        assert!(matches!(
            release_automatic_workspace_by_target(
                &mut connection,
                ReleaseTarget::WorkspacePath(candidate.canonical_path.clone()),
            ),
            Err(WorkspaceError::WorkspaceUnclaimed { path }) if path == candidate.canonical_path
        ));
        assert!(matches!(
            release_automatic_workspace_by_target(
                &mut connection,
                ReleaseTarget::ClaimId(cwd_claim.claim_id),
            ),
            Err(WorkspaceError::ClaimNotFound { claim_id }) if claim_id == cwd_claim.claim_id
        ));

        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn concurrent_release_exits_when_the_workspace_operation_is_busy() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let acquire = acquire_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("automatic acquire should succeed");
        let intent = OperationIntent::new(
            candidate.id,
            "release",
            Timestamp::after_seconds(300),
            "hold release admission",
            JsonDocument::parse(r#"{"target":"test"}"#).unwrap(),
        );
        let lease_id = intent.lease_id;
        let operation = begin_operation(&mut connection, &intent)
            .expect("first release operation should acquire admission");

        let error = release_automatic_workspace_by_target(
            &mut connection,
            ReleaseTarget::WorkspacePath(candidate.canonical_path.clone()),
        )
        .expect_err("second release should exit busy");
        assert!(matches!(
            error,
            WorkspaceError::OperationActive { workspace_id } if workspace_id == candidate.id
        ));
        assert_eq!(
            crate::storage::operation_state(&mut connection, &operation.id)
                .expect("operation state should be queryable"),
            Some(OperationState::Running)
        );
        assert_eq!(
            crate::storage::find_workspace_claim(&mut connection, &candidate.id)
                .expect("claim lookup should succeed")
                .expect("claim should remain active")
                .id,
            acquire.claim_id
        );

        crate::storage::record_operation_transition(
            &mut connection,
            &lease_id,
            OperationState::Failed,
            TransitionMetadata::new("operation_failed", "test"),
        )
        .expect("test operation should finish");
        crate::storage::release_workspace_claim(&mut connection, &candidate.id, &acquire.claim_id)
            .expect("claim should be releasable for cleanup");
        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn rejects_dirty_release_and_retains_the_active_claim() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let acquire = acquire_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("automatic acquire should succeed");
        fs::write(worktree_path.join("local-change"), "dirty\n")
            .expect("worktree should become dirty");

        let error = release_automatic_workspace(
            &mut connection,
            &candidate.canonical_path,
            acquire.claim_id,
        )
        .expect_err("dirty release should be rejected");
        assert!(matches!(error, WorkspaceError::NotReusable { path: _ }));
        assert_eq!(
            crate::storage::find_workspace_claim(&mut connection, &candidate.id)
                .expect("claim lookup should succeed")
                .expect("active claim should remain")
                .id,
            acquire.claim_id
        );
        assert_eq!(
            crate::storage::find_workspace(&mut connection, &candidate.id)
                .expect("workspace lookup should succeed")
                .state,
            WorkspaceState::Degraded
        );
        assert!(worktree_path.join("local-change").exists());
        let rejected_event = crate::schema::lifecycle_events::table
            .filter(crate::schema::lifecycle_events::entity_id.eq(candidate.id.to_string()))
            .filter(crate::schema::lifecycle_events::event_type.eq("workspace_release_rejected"))
            .select(crate::storage::EventRow::as_select())
            .first(&mut connection)
            .expect("release rejection event should exist");
        assert_eq!(
            crate::storage::operation_state(&mut connection, &rejected_event.operation_id)
                .expect("release operation state should exist"),
            Some(OperationState::Failed)
        );

        crate::storage::release_workspace_claim(&mut connection, &candidate.id, &acquire.claim_id)
            .expect("active claim should be releasable for test cleanup");
        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("dirty test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn dirty_release_does_not_align_any_clean_worktree() {
        let (root, database_path, mut connection, plan, candidate, worktree_paths) =
            automatic_candidate_fixture_with_sources(&["first", "second"]);
        let acquire = acquire_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("automatic acquire should succeed");
        let first_worktree = worktree_paths
            .iter()
            .find(|path| path.ends_with("first"))
            .expect("first worktree should exist");
        let second_worktree = worktree_paths
            .iter()
            .find(|path| path.ends_with("second"))
            .expect("second worktree should exist");
        let first_source = plan
            .repositories
            .iter()
            .find(|repository| repository.source_path.as_path().ends_with("first"))
            .expect("first source should exist");
        let first_head = git::find_worktree(&first_source.source_path, first_worktree)
            .expect("first worktree should be registered")
            .head
            .expect("first worktree should have a head");
        fs::write(
            first_source.source_path.as_path().join("README"),
            "updated\n",
        )
        .expect("first source should be updated");
        run_git(
            first_source.source_path.as_path(),
            &["commit", "-qam", "update"],
        );
        fs::write(second_worktree.join("local-change"), "dirty\n")
            .expect("second worktree should become dirty");

        let error = release_automatic_workspace(
            &mut connection,
            &candidate.canonical_path,
            acquire.claim_id,
        )
        .expect_err("dirty release should be rejected before alignment");

        assert!(matches!(error, WorkspaceError::NotReusable { path: _ }));
        let first_after = git::find_worktree(&first_source.source_path, first_worktree)
            .expect("first worktree should remain registered");
        assert_eq!(first_after.head.as_deref(), Some(first_head.as_str()));
        assert!(first_after.detached);
        assert!(second_worktree.join("local-change").exists());
        assert!(
            crate::storage::find_workspace_claim(&mut connection, &candidate.id)
                .expect("claim lookup should succeed")
                .is_some()
        );

        crate::storage::release_workspace_claim(&mut connection, &candidate.id, &acquire.claim_id)
            .expect("active claim should be releasable for cleanup");
        for worktree_path in worktree_paths {
            let repository = plan
                .repositories
                .iter()
                .find(|repository| {
                    repository.source_path.as_path().file_name() == worktree_path.file_name()
                })
                .expect("worktree source should exist");
            git::remove_worktree(&repository.source_path, &worktree_path)
                .expect("test worktree should be removable");
        }
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn rejects_release_with_a_wrong_claim_identifier() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let acquire = acquire_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("automatic acquire should succeed");
        let wrong_claim_id = ClaimId::new();

        assert!(matches!(
            release_automatic_workspace(&mut connection, &candidate.canonical_path, wrong_claim_id),
            Err(WorkspaceError::ClaimNotFound { claim_id }) if claim_id == wrong_claim_id
        ));
        assert_eq!(
            crate::storage::find_workspace_claim(&mut connection, &candidate.id)
                .expect("claim lookup should succeed")
                .expect("original claim should remain active")
                .id,
            acquire.claim_id
        );

        crate::storage::release_workspace_claim(&mut connection, &candidate.id, &acquire.claim_id)
            .expect("original claim should be releasable");
        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn provisions_an_automatic_workspace_below_the_managed_root() {
        let root = test_root();
        let source = root.join("source");
        repository(&source);
        let workspace_root = root.join("managed");
        let mut plan = prepare_automatic(&AutomaticCreateRequest {
            repositories: vec![source.clone()],
        })
        .expect("automatic allocation plan should be prepared");
        plan.workspace_root = CanonicalPath::from_absolute(workspace_root.clone())
            .expect("managed root should be absolute");
        let database_path = root.join("state.sqlite");
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");

        let result = provision_automatic(&mut connection, &plan)
            .expect("automatic provisioning should succeed");
        let canonical_workspace_root =
            CanonicalPath::resolve(&workspace_root).expect("managed root should resolve");
        assert!(
            result
                .workspace_path
                .as_path()
                .starts_with(canonical_workspace_root.as_path()),
            "workspace path {} should be below {}",
            result.workspace_path,
            canonical_workspace_root
        );
        assert!(result
            .workspace_path
            .as_path()
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("ws-")));
        assert!(result.workspace_path.as_path().is_dir());

        let workspace =
            crate::storage::find_workspace_by_path(&mut connection, &result.workspace_path)
                .expect("workspace lookup should succeed")
                .expect("provisioned workspace should be persisted");
        assert_eq!(
            workspace.management_mode,
            WorkspaceManagementMode::Automatic
        );
        assert_eq!(workspace.pool_id, Some(result.pool_id));
        assert_eq!(workspace.state, WorkspaceState::Ready);
        let repositories = crate::storage::list_repo_worktrees(&mut connection, &workspace.id)
            .expect("worktree lookup should succeed");
        assert_eq!(repositories.len(), 1);
        assert_eq!(repositories[0].state, RepoWorktreeState::Attached);
        assert!(repositories[0].worktree_path.as_path().is_dir());

        let claim = crate::storage::find_workspace_claim(&mut connection, &workspace.id)
            .expect("claim lookup should succeed")
            .expect("provisioned workspace should be acquired");
        assert_eq!(claim.id, result.claim_id);
        let operation_id = crate::schema::operations::table
            .filter(crate::schema::operations::workspace_id.eq(workspace.id))
            .select(crate::schema::operations::id)
            .first::<crate::domain::OperationId>(&mut connection)
            .expect("provisioning operation should exist");
        assert_eq!(
            crate::storage::operation_state(&mut connection, &operation_id)
                .expect("provisioning operation state should exist"),
            Some(OperationState::Succeeded)
        );
        assert!(
            crate::storage::list_events_for_operation(&mut connection, &operation_id)
                .expect("provisioning events should be readable")
                .iter()
                .any(|event| event.event_type == "workspace_claimed")
        );

        crate::storage::release_workspace_claim(&mut connection, &workspace.id, &result.claim_id)
            .expect("provisioning claim should be releasable");
        crate::git::remove_worktree(
            &CanonicalPath::resolve(&source).expect("source repository should resolve"),
            repositories[0].worktree_path.as_path(),
        )
        .expect("provisioned worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn rolls_back_automatic_provisioning_after_a_repository_identity_change() {
        let root = test_root();
        let first = root.join("first");
        let second = root.join("second");
        repository(&first);
        repository(&second);
        let workspace_root = root.join("managed");
        let mut plan = prepare_automatic(&AutomaticCreateRequest {
            repositories: vec![first.clone(), second.clone()],
        })
        .expect("automatic allocation plan should be prepared");
        plan.workspace_root =
            CanonicalPath::from_absolute(workspace_root).expect("managed root should be absolute");
        plan.repositories[1].repository_identity =
            CanonicalPath::from_absolute(root.join("unexpected-repository-identity"))
                .expect("fake repository identity should be absolute");
        let database_path = root.join("state.sqlite");
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");

        let error = provision_automatic(&mut connection, &plan)
            .expect_err("changed repository identity should roll back provisioning");
        let workspace_path = match &error {
            WorkspaceError::NotReusable { path } => path.clone().into_path_buf(),
            other => panic!("unexpected provisioning error: {other}"),
        };
        assert!(!workspace_path.exists());
        let workspace = crate::storage::find_workspace_by_path(
            &mut connection,
            &CanonicalPath::from_absolute(workspace_path.clone())
                .expect("workspace path should be absolute"),
        )
        .expect("workspace lookup should succeed")
        .expect("failed workspace should remain persisted");
        assert_eq!(
            workspace.management_mode,
            WorkspaceManagementMode::Automatic
        );
        assert_eq!(workspace.state, WorkspaceState::Failed);
        assert!(
            crate::storage::find_workspace_claim(&mut connection, &workspace.id)
                .expect("claim lookup should succeed")
                .is_none()
        );
        assert!(
            crate::storage::list_repo_worktrees(&mut connection, &workspace.id)
                .expect("worktree lookup should succeed")
                .iter()
                .all(|worktree| worktree.state == RepoWorktreeState::Failed)
        );
        let operation_id = crate::schema::operations::table
            .filter(crate::schema::operations::workspace_id.eq(workspace.id))
            .select(crate::schema::operations::id)
            .first::<crate::domain::OperationId>(&mut connection)
            .expect("provisioning operation should exist");
        assert_eq!(
            crate::storage::operation_state(&mut connection, &operation_id)
                .expect("provisioning operation state should exist"),
            Some(OperationState::RolledBack)
        );
        assert_eq!(
            crate::git::list_worktrees(&CanonicalPath::resolve(&first).unwrap())
                .expect("first repository worktrees should be readable")
                .len(),
            1
        );
        assert_eq!(
            crate::git::list_worktrees(&CanonicalPath::resolve(&second).unwrap())
                .expect("second repository worktrees should be readable")
                .len(),
            1
        );

        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn rolls_back_worktrees_when_a_later_repository_fails() {
        let root = test_root();
        let first = root.join("first");
        let second = root.join("second");
        repository(&first);
        repository(&second);
        let second_git = second.join(".git");
        let plan = prepare_create(&CreateRequest {
            workspace_path: root.join("workspace"),
            repositories: vec![first, second],
        })
        .expect("creation plan should be prepared");
        let database_path = root.join("state.sqlite");
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");
        let context =
            initialize_creation(&mut connection, plan).expect("creation should initialize");
        fs::remove_dir_all(second_git).expect("second repository metadata should be removed");

        assert!(execute_creation(&mut connection, &context).is_err());
        assert!(!context.plan.workspace_path.as_path().exists());
        assert!(context
            .repositories
            .iter()
            .all(|repository| !repository.plan.worktree_path.exists()));
        assert_eq!(
            crate::storage::find_workspace_by_path(&mut connection, &context.plan.workspace_path)
                .unwrap()
                .unwrap()
                .state,
            WorkspaceState::Failed
        );
        assert_eq!(
            crate::storage::operation_state(&mut connection, &context.operation_id).unwrap(),
            Some(OperationState::RolledBack)
        );
        assert_eq!(
            crate::storage::list_repo_worktrees(&mut connection, &context.workspace_id)
                .unwrap()
                .iter()
                .filter(|worktree| worktree.state == RepoWorktreeState::Failed)
                .count(),
            2
        );
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn rejects_creation_when_an_existing_operation_is_active() {
        let root = test_root();
        let source = root.join("source");
        repository(&source);
        let plan = prepare_create(&CreateRequest {
            workspace_path: root.join("workspace"),
            repositories: vec![source],
        })
        .expect("creation plan should be prepared");
        let database_path = root.join("state.sqlite");
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");
        let context = initialize_creation(&mut connection, plan)
            .expect("initial operation should be persisted");

        let error = create_with_connection(&mut connection, context.plan.clone())
            .expect_err("active operation should reject a competing creation");
        assert!(matches!(
            error,
            WorkspaceError::OperationActive { workspace_id } if workspace_id == context.workspace_id
        ));
        assert_eq!(
            crate::storage::operation_state(&mut connection, &context.operation_id).unwrap(),
            Some(OperationState::Running)
        );
        assert!(!context.plan.workspace_path.as_path().exists());

        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn recovers_an_expired_operation_before_rejecting_a_duplicate_creation() {
        let root = test_root();
        let source = root.join("source");
        repository(&source);
        let plan = prepare_create(&CreateRequest {
            workspace_path: root.join("workspace"),
            repositories: vec![source],
        })
        .expect("creation plan should be prepared");
        let database_path = root.join("state.sqlite");
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");
        let context = initialize_creation(&mut connection, plan)
            .expect("initial operation should be persisted");
        execute_creation(&mut connection, &context).expect("Git steps should complete");
        let lease = crate::storage::find_operation_lease(&mut connection, &context.operation_id)
            .expect("operation lease should be queryable")
            .expect("operation lease should exist");
        diesel::update(crate::schema::operation_leases::table.find(lease.id))
            .set(crate::schema::operation_leases::lease_expires_at.eq(Timestamp::now()))
            .execute(&mut connection)
            .expect("operation lease should expire");

        let error = create_with_connection(&mut connection, context.plan.clone())
            .expect_err("recovered workspace should remain managed");
        assert!(matches!(error, WorkspaceError::AlreadyManaged { path: _ }));
        assert_eq!(
            crate::storage::operation_state(&mut connection, &context.operation_id).unwrap(),
            Some(OperationState::Succeeded)
        );
        assert!(
            crate::storage::list_events_for_operation(&mut connection, &context.operation_id)
                .unwrap()
                .iter()
                .any(|event| event.event_type == "operation_recovered")
        );

        for repository in &context.repositories {
            crate::git::remove_worktree(
                &repository.plan.source_path,
                &repository.plan.worktree_path,
            )
            .expect("recovered worktree should be removable");
        }
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn formats_workspace_errors_and_sources() {
        let path = CanonicalPath::resolve(".").expect("workspace path should resolve");
        let primary = WorkspaceError::AlreadyManaged { path: path.clone() };
        let errors = [
            WorkspaceError::Validation {
                source: ValidationError::NoRepositories,
            },
            WorkspaceError::Naming {
                source: NamingError::MissingRepositoryName {
                    repository: path.clone(),
                },
            },
            WorkspaceError::Git {
                source: GitError::WorktreeNotFound {
                    path: path.as_path().to_owned(),
                },
            },
            WorkspaceError::Database {
                source: diesel::result::Error::NotFound,
            },
            WorkspaceError::DatabaseOpen {
                source: crate::database::DatabaseError::Path {
                    source: crate::paths::PathError::HomeDirectoryUnavailable,
                },
            },
            WorkspaceError::OperationActive {
                workspace_id: WorkspaceId::new(),
            },
            WorkspaceError::NotAutomatic { path: path.clone() },
            WorkspaceError::NotReusable { path: path.clone() },
            WorkspaceError::ClaimActive {
                workspace_id: WorkspaceId::new(),
            },
            WorkspaceError::GeneratedPathUnavailable {
                path: path.as_path().to_owned(),
            },
            WorkspaceError::ClaimNotFound {
                claim_id: ClaimId::new(),
            },
            WorkspaceError::WorkspaceUnclaimed { path: path.clone() },
            WorkspaceError::RepositorySetMismatch {
                workspace_id: WorkspaceId::new(),
            },
            WorkspaceError::WorkspaceNotFound { path: path.clone() },
            WorkspaceError::Json {
                source: JsonDocument::parse("not json").unwrap_err(),
            },
            primary,
            WorkspaceError::Rollback {
                primary: Box::new(WorkspaceError::AlreadyManaged { path: path.clone() }),
                rollback: Box::new(WorkspaceError::RollbackFailure {
                    errors: vec!["rollback error".to_owned()],
                }),
            },
            WorkspaceError::RollbackFailure {
                errors: vec!["rollback error".to_owned()],
            },
            WorkspaceError::Io {
                path: path.into_path_buf(),
                source: std::io::Error::other("io error"),
            },
        ];

        for error in errors {
            assert!(!error.to_string().is_empty());
            let _ = std::error::Error::source(&error);
        }
    }
}
