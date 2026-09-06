use std::fmt;
use std::fs;
use std::path::PathBuf;

use diesel::sqlite::SqliteConnection;
use serde::Serialize;

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
    find_workspace_claim, find_workspace_pool, insert_managed_workspace, insert_repo_worktree,
    insert_workspace_claim, insert_workspace_pool_repositories, persist_operation_intent,
    persist_operation_step_intent, record_operation_transition, record_repo_worktree_transition,
    record_workspace_acquire, record_workspace_acquire_failure, record_workspace_release,
    record_workspace_release_rejection, record_workspace_transition, record_worktree_step_result,
    release_workspace_claim, with_short_transaction, EventDraft, NewManagedWorkspace,
    NewRepoWorktree, NewWorkspacePoolRepository, OperationIntent, OperationIntentError,
    TransitionMetadata,
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
        let info = git::inspect_repository(&source_path)?;
        plans.push(AutomaticRepositoryPlan {
            source_path,
            repository_identity: info.common_dir,
            head: info.head,
        });
    }
    let workspace_root = CanonicalPath::from_absolute(
        crate::paths::managed_workspace_directory().map_err(WorkspaceError::Path)?,
    )
    .map_err(|error| WorkspaceError::Validation(ValidationError::Canonicalize(error)))?;

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
            .map_err(WorkspaceError::Database)
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
    let Some(pool) =
        find_workspace_pool(connection, &repository_set).map_err(WorkspaceError::Database)?
    else {
        return Ok(Vec::new());
    };
    let candidates = crate::storage::list_automatic_workspace_candidates(connection, &pool.id)
        .map_err(WorkspaceError::Database)?;
    let mut idle_candidates = Vec::new();
    for workspace in candidates {
        if crate::storage::find_running_operation(connection, &workspace.id)
            .map_err(WorkspaceError::Database)?
            .is_some()
        {
            continue;
        }
        if find_workspace_claim(connection, &workspace.id)
            .map_err(WorkspaceError::Database)?
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
    matches!(
        error,
        WorkspaceError::OperationActive(_)
            | WorkspaceError::NotAutomatic(_)
            | WorkspaceError::NotReusable(_)
            | WorkspaceError::ClaimActive(_)
            | WorkspaceError::Database(diesel::result::Error::NotFound)
            | WorkspaceError::Database(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                _,
            ))
    )
}

pub fn acquire_automatic_candidate(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
    candidate: &crate::storage::WorkspaceRow,
) -> Result<AutomaticClaimResult, WorkspaceError> {
    acquire_automatic_candidate_with_post_acquire_hook(connection, plan, candidate, || {})
}

fn acquire_automatic_candidate_with_post_acquire_hook<F>(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
    candidate: &crate::storage::WorkspaceRow,
    post_acquire: F,
) -> Result<AutomaticClaimResult, WorkspaceError>
where
    F: FnOnce(),
{
    let intent_json = JsonDocument::from_serializable(plan).map_err(WorkspaceError::Json)?;
    let intent = OperationIntent::new(
        candidate.id,
        "acquire",
        Timestamp::after_seconds(300),
        "acquire workspace claim",
        intent_json,
    );
    let lease_id = intent.lease_id;
    let operation = begin_operation(connection, &intent).map_err(map_operation_error)?;
    let boundary = match reconciliation::reconcile_workspace_for_access(
        connection,
        &candidate.id,
        &operation.id,
    ) {
        Ok(boundary) => boundary,
        Err(error) => {
            let primary = WorkspaceError::Reconciliation(error);
            fail_operation(connection, &operation.id, &lease_id, &primary);
            return Err(primary);
        }
    };
    if boundary.workspace.management_mode != crate::domain::WorkspaceManagementMode::Automatic {
        let primary = WorkspaceError::NotAutomatic(candidate.canonical_path.clone());
        fail_operation(connection, &operation.id, &lease_id, &primary);
        return Err(primary);
    }
    let Some(pool_id) = boundary.workspace.pool_id else {
        let primary = WorkspaceError::RepositorySetMismatch(candidate.id);
        fail_operation(connection, &operation.id, &lease_id, &primary);
        return Err(primary);
    };
    if boundary.claim.is_some() {
        let primary = WorkspaceError::ClaimActive(candidate.id);
        fail_operation(connection, &operation.id, &lease_id, &primary);
        return Err(primary);
    }
    if boundary.summary.workspace_state != WorkspaceState::Ready {
        let primary = WorkspaceError::NotReusable(candidate.canonical_path.clone());
        fail_operation(connection, &operation.id, &lease_id, &primary);
        return Err(primary);
    }

    let claim = WorkspaceClaim::new(candidate.id);
    let details_json = acquire_details(&claim, pool_id);
    let record_result = record_workspace_acquire(
        connection,
        &operation.id,
        &claim,
        Some(details_json.clone()),
    );
    if let Err(error) = record_result {
        let primary = WorkspaceError::Database(error);
        fail_operation(connection, &operation.id, &lease_id, &primary);
        return Err(primary);
    }

    post_acquire();

    let final_boundary = match reconciliation::reconcile_workspace_for_access(
        connection,
        &candidate.id,
        &operation.id,
    ) {
        Ok(boundary) => boundary,
        Err(error) => {
            let primary = WorkspaceError::Reconciliation(error);
            return Err(fail_acquire(
                connection,
                &operation.id,
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
        let primary = WorkspaceError::NotReusable(candidate.canonical_path.clone());
        return Err(fail_acquire(
            connection,
            &operation.id,
            &lease_id,
            &candidate.id,
            &claim.id,
            primary,
            Some(details_json),
        ));
    }

    if let Err(error) = record_operation_transition(
        connection,
        &operation.id,
        &lease_id,
        OperationState::Succeeded,
        TransitionMetadata::new("operation_succeeded", "trees")
            .with_pending_step("acquire complete")
            .with_details(details_json.clone()),
    ) {
        let primary = WorkspaceError::Database(error);
        fail_operation(connection, &operation.id, &lease_id, &primary);
        return Err(primary);
    }

    Ok(AutomaticClaimResult {
        workspace_path: candidate.canonical_path.clone(),
        pool_id,
        claim_id: claim.id,
    })
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
        OperationIntentError::WorkspaceBusy(workspace_id) => {
            WorkspaceError::OperationActive(workspace_id)
        }
        OperationIntentError::Database(error) => WorkspaceError::Database(error),
    }
}

fn fail_operation(
    connection: &mut SqliteConnection,
    operation_id: &crate::domain::OperationId,
    lease_id: &LeaseId,
    error: &WorkspaceError,
) {
    let _ = record_operation_transition(
        connection,
        operation_id,
        lease_id,
        OperationState::Failed,
        TransitionMetadata::new("operation_failed", "trees")
            .with_pending_step("operation failed")
            .with_error(error_document(error)),
    );
}

fn fail_acquire(
    connection: &mut SqliteConnection,
    operation_id: &crate::domain::OperationId,
    lease_id: &LeaseId,
    workspace_id: &WorkspaceId,
    claim_id: &ClaimId,
    primary: WorkspaceError,
    details_json: Option<JsonDocument>,
) -> WorkspaceError {
    let error_json = error_document(&primary);
    match record_workspace_acquire_failure(
        connection,
        operation_id,
        lease_id,
        workspace_id,
        claim_id,
        details_json,
        error_json,
    ) {
        Ok(()) => primary,
        Err(error) => WorkspaceError::Rollback {
            primary: Box::new(primary),
            rollback: Box::new(WorkspaceError::Database(error)),
        },
    }
}

pub fn release_automatic_workspace(
    connection: &mut SqliteConnection,
    workspace_path: &CanonicalPath,
    claim_id: ClaimId,
) -> Result<ReleaseResult, WorkspaceError> {
    let workspace = find_workspace_by_path(connection, workspace_path)
        .map_err(WorkspaceError::Database)?
        .ok_or_else(|| WorkspaceError::WorkspaceNotFound(workspace_path.clone()))?;
    if workspace.management_mode != WorkspaceManagementMode::Automatic {
        return Err(WorkspaceError::NotAutomatic(workspace.canonical_path));
    }
    let claim_matches = find_workspace_claim(connection, &workspace.id)
        .map_err(WorkspaceError::Database)?
        .is_some_and(|claim| claim.id == claim_id);
    if !claim_matches {
        return Err(WorkspaceError::ClaimNotFound(claim_id));
    }

    let intent_json = JsonDocument::from_serializable(&serde_json::json!({
        "workspace_path": workspace_path,
        "claim_id": claim_id,
    }))
    .map_err(WorkspaceError::Json)?;
    let intent = OperationIntent::new(
        workspace.id,
        "release",
        Timestamp::after_seconds(300),
        "release workspace claim",
        intent_json,
    );
    let lease_id = intent.lease_id;
    let operation = begin_operation(connection, &intent).map_err(map_operation_error)?;
    let boundary = match reconciliation::reconcile_workspace_for_access(
        connection,
        &workspace.id,
        &operation.id,
    ) {
        Ok(boundary) => boundary,
        Err(error) => {
            let primary = WorkspaceError::Reconciliation(error);
            fail_operation(connection, &operation.id, &lease_id, &primary);
            return Err(primary);
        }
    };
    if boundary.workspace.management_mode != WorkspaceManagementMode::Automatic {
        let primary = WorkspaceError::NotAutomatic(boundary.workspace.canonical_path);
        fail_operation(connection, &operation.id, &lease_id, &primary);
        return Err(primary);
    }
    let Some(active_claim) = boundary.claim.as_ref() else {
        let primary = WorkspaceError::ClaimNotFound(claim_id);
        fail_operation(connection, &operation.id, &lease_id, &primary);
        return Err(primary);
    };
    if active_claim.id != claim_id {
        let primary = WorkspaceError::ClaimNotFound(claim_id);
        fail_operation(connection, &operation.id, &lease_id, &primary);
        return Err(primary);
    }
    // Do not release the claim until live reconciliation proves the slot is
    // safe to reuse; a rejected release intentionally keeps the claim.
    let details_json = release_details(workspace_path, claim_id);
    if boundary.summary.workspace_state != WorkspaceState::Ready {
        let primary = WorkspaceError::NotReusable(workspace.canonical_path.clone());
        return Err(fail_release(
            connection,
            &operation.id,
            &lease_id,
            &workspace.id,
            primary,
            Some(details_json),
        ));
    }
    if let Err(error) = record_workspace_release(
        connection,
        &operation.id,
        &lease_id,
        &workspace.id,
        &claim_id,
        Some(details_json),
    ) {
        let primary = WorkspaceError::Database(error);
        fail_operation(connection, &operation.id, &lease_id, &primary);
        return Err(primary);
    }
    let released_workspace =
        find_workspace(connection, &workspace.id).map_err(WorkspaceError::Database)?;
    let released_at = released_workspace
        .last_released_at
        .ok_or_else(|| WorkspaceError::Database(diesel::result::Error::NotFound))?;
    Ok(ReleaseResult {
        workspace_path: released_workspace.canonical_path,
        claim_id,
        released_at,
    })
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
    operation_id: &crate::domain::OperationId,
    lease_id: &LeaseId,
    workspace_id: &WorkspaceId,
    primary: WorkspaceError,
    details_json: Option<JsonDocument>,
) -> WorkspaceError {
    let error_json = error_document(&primary);
    match record_workspace_release_rejection(
        connection,
        operation_id,
        lease_id,
        workspace_id,
        details_json,
        error_json,
    ) {
        Ok(()) => primary,
        Err(error) => WorkspaceError::Rollback {
            primary: Box::new(primary),
            rollback: Box::new(WorkspaceError::Database(error)),
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
        return Err(WorkspaceError::Validation(ValidationError::NoRepositories));
    }
    for repository in &plan.repositories {
        if plan
            .workspace_root
            .as_path()
            .starts_with(repository.source_path.as_path())
        {
            return Err(WorkspaceError::Validation(
                ValidationError::WorkspaceInsideRepository {
                    workspace: plan.workspace_root.as_path().to_owned(),
                    repository: repository.source_path.as_path().to_owned(),
                },
            ));
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
        .map_err(WorkspaceError::Database)?;

    let (workspace_id, workspace_path) =
        next_generated_workspace(connection, normalized_plan.workspace_root.as_path())?;
    let creation_plan = automatic_creation_plan(&normalized_plan, workspace_path.clone())?;
    let claim = WorkspaceClaim::new(workspace_id);
    let intent_json = JsonDocument::from_serializable(&serde_json::json!({
        "allocation": normalized_plan,
        "workspace_id": workspace_id,
        "workspace_path": workspace_path,
        "claim_id": claim.id,
    }))
    .map_err(WorkspaceError::Json)?;
    let context = initialize_creation_with_mode(
        connection,
        creation_plan,
        WorkspaceManagementMode::Automatic,
        Some(pool.id),
        Some(&claim),
        intent_json,
    )?;

    if let Err(error) = reconciliation::reconcile_workspace(
        connection,
        &context.workspace_id,
        &context.operation_id,
    ) {
        let primary = WorkspaceError::Reconciliation(error);
        return fail_creation(connection, &context, &[], None, Some(&claim), primary)
            .map(|_| unreachable!("automatic provisioning rollback always fails the operation"));
    }
    execute_creation_with_claim(connection, &context, Some(&claim))?;
    let summary = match reconciliation::reconcile_workspace(
        connection,
        &context.workspace_id,
        &context.operation_id,
    ) {
        Ok(summary) => summary,
        Err(error) => {
            let primary = WorkspaceError::Reconciliation(error);
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
        let primary = WorkspaceError::NotReusable(context.plan.workspace_path.clone());
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
        &context.operation_id,
        &context.lease_id,
        &claim,
        Some(details_json),
    ) {
        let primary = WorkspaceError::Database(error);
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
        let workspace_path = CanonicalPath::from_absolute(workspace_path)
            .map_err(|error| WorkspaceError::Validation(ValidationError::Canonicalize(error)))?;
        if find_workspace_by_path(connection, &workspace_path)
            .map_err(WorkspaceError::Database)?
            .is_none()
        {
            return Ok((workspace_id, workspace_path));
        }
    }
    Err(WorkspaceError::GeneratedPathUnavailable(
        workspace_root.to_owned(),
    ))
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
    let info = git::inspect_repository(&plan.repository)?;
    Ok(RepositoryPlan {
        source_path: plan.repository,
        repository_identity: info.common_dir,
        worktree_path: plan.worktree_path,
        head: info.head,
    })
}

pub fn initialize_creation(
    connection: &mut SqliteConnection,
    plan: CreationPlan,
) -> Result<CreationContext, WorkspaceError> {
    let intent_json = JsonDocument::from_serializable(&plan).map_err(WorkspaceError::Json)?;
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
        .map_err(WorkspaceError::Database)?
        .is_some()
    {
        return Err(WorkspaceError::AlreadyManaged(plan.workspace_path));
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
            .map_err(WorkspaceError::Database)?;
            if let Some(pool_id) = pool_id {
                insert_workspace_pool_repositories(
                    connection,
                    &[NewWorkspacePoolRepository {
                        pool_id,
                        repository_id: origin.id,
                    }],
                )
                .map_err(WorkspaceError::Database)?;
            }
            Ok(TrackedRepository {
                id: RepoWorktreeId::new(),
                origin_repository_id: origin.id,
                plan: repository.clone(),
            })
        })
        .collect::<Result<Vec<_>, WorkspaceError>>()?;
    let now = Timestamp::now();

    with_short_transaction(connection, |connection| {
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
    .map_err(WorkspaceError::Database)?;

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
    if let Err(source) = fs::create_dir(&context.plan.workspace_path) {
        let primary = WorkspaceError::Io {
            path: context.plan.workspace_path.clone().into_path_buf(),
            source,
        };
        return fail_creation(connection, context, &[], None, claim, primary);
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

#[derive(Debug, Clone, Serialize)]
pub struct CreationResult {
    pub workspace_path: CanonicalPath,
    pub worktree_paths: Vec<PathBuf>,
}

pub fn create(request: CreateRequest) -> Result<CreationResult, WorkspaceError> {
    let workspace_path = validation::resolve_workspace_path(&request.workspace_path)?;
    let mut connection = crate::database::open_default().map_err(WorkspaceError::DatabaseOpen)?;
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
    reconciliation::reconcile_workspace(connection, &context.workspace_id, &context.operation_id)
        .map_err(WorkspaceError::Reconciliation)?;
    execute_creation(connection, &context)?;
    reconciliation::reconcile_workspace(connection, &context.workspace_id, &context.operation_id)
        .map_err(WorkspaceError::Reconciliation)?;
    finalize_persisted_creation(
        connection,
        &context.workspace_id,
        &context.operation_id,
        &context.lease_id,
    )
    .map_err(WorkspaceError::Database)?;
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
        find_workspace_by_path(connection, workspace_path).map_err(WorkspaceError::Database)?
    else {
        return Ok(());
    };

    match reconciliation::recover_expired_operation(connection, &workspace.id)
        .map_err(WorkspaceError::Reconciliation)?
    {
        reconciliation::RecoveryOutcome::LeaseActive => {
            Err(WorkspaceError::OperationActive(workspace.id))
        }
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
    let intent_json =
        JsonDocument::from_serializable(&repository.plan).map_err(WorkspaceError::Json)?;
    persist_operation_step_intent(
        connection,
        &context.operation_id,
        &context.lease_id,
        format!("attach {}", repository.plan.source_path),
        intent_json,
    )
    .map_err(WorkspaceError::Database)?;
    // The subprocess and lease-renewal callback run outside SQLite
    // transactions; each renewal is an independent short operation update.
    let lease_id = context.lease_id;
    git::add_detached_worktree_with_heartbeat(
        &repository.plan.source_path,
        &repository.plan.worktree_path,
        || match crate::storage::renew_operation_lease(connection, &lease_id) {
            Ok(true) => Ok(()),
            Ok(false) => Err(GitError::Heartbeat(
                "operation lease is no longer owned".to_owned(),
            )),
            Err(error) => Err(GitError::Heartbeat(error.to_string())),
        },
    )?;
    let worktree =
        git::find_worktree(&repository.plan.source_path, &repository.plan.worktree_path)?;
    record_worktree_step_result(
        connection,
        &repository.id,
        &context.operation_id,
        &context.lease_id,
        RepoWorktreeState::Attached,
        worktree.head,
        TransitionMetadata::new("worktree_attached", "trees")
            .with_pending_step("worktree attached")
            .with_details(
                JsonDocument::from_serializable(&repository.plan).map_err(WorkspaceError::Json)?,
            ),
    )
    .map_err(WorkspaceError::Database)?;
    reconciliation::reconcile_workspace(connection, &context.workspace_id, &context.operation_id)
        .map_err(WorkspaceError::Reconciliation)?;
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
    let mut errors = Vec::new();
    let failed_id = failed.map(|repository| repository.id);
    let repositories = failed
        .into_iter()
        .chain(completed.iter().rev().copied())
        .collect::<Vec<_>>();

    for repository in repositories {
        match git::find_worktree(&repository.plan.source_path, &repository.plan.worktree_path) {
            Ok(_) => {
                if let Err(error) = git::remove_worktree(
                    &repository.plan.source_path,
                    &repository.plan.worktree_path,
                ) {
                    errors.push(error.to_string());
                }
            }
            Err(GitError::WorktreeNotFound(_)) => {}
            Err(_error) if failed_id == Some(repository.id) => {}
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
    }

    if context.plan.workspace_path.as_path().exists() {
        if let Err(error) = fs::remove_dir(&context.plan.workspace_path) {
            errors.push(error.to_string());
        }
    }

    if let Some(claim) = claim {
        match release_workspace_claim(connection, &context.workspace_id, &claim.id) {
            Ok(true) => {}
            Ok(false) => errors.push("automatic workspace claim was not found".to_owned()),
            Err(error) => errors.push(error.to_string()),
        }
    }

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
        &context.operation_id,
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

    if errors.is_empty() {
        Ok(())
    } else {
        Err(WorkspaceError::RollbackFailure { errors })
    }
}

fn error_document(error: &WorkspaceError) -> JsonDocument {
    JsonDocument::from_serializable(&serde_json::json!({
        "error": error.to_string(),
    }))
    .expect("JSON error document should serialize")
}

#[derive(Debug)]
pub enum WorkspaceError {
    Validation(ValidationError),
    Naming(NamingError),
    Git(GitError),
    Database(diesel::result::Error),
    DatabaseOpen(crate::database::DatabaseError),
    Path(crate::paths::PathError),
    Reconciliation(ReconciliationError),
    OperationActive(WorkspaceId),
    NotAutomatic(CanonicalPath),
    NotReusable(CanonicalPath),
    ClaimActive(WorkspaceId),
    GeneratedPathUnavailable(PathBuf),
    ClaimNotFound(ClaimId),
    RepositorySetMismatch(WorkspaceId),
    WorkspaceNotFound(CanonicalPath),
    Json(crate::domain::JsonDocumentError),
    AlreadyManaged(CanonicalPath),
    Rollback {
        primary: Box<WorkspaceError>,
        rollback: Box<WorkspaceError>,
    },
    RollbackFailure {
        errors: Vec<String>,
    },
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(formatter),
            Self::Naming(error) => error.fmt(formatter),
            Self::Git(error) => error.fmt(formatter),
            Self::Database(error) => write!(formatter, "database operation failed: {error}"),
            Self::DatabaseOpen(error) => {
                write!(formatter, "failed to open lifecycle database: {error}")
            }
            Self::Path(error) => write!(formatter, "failed to resolve workspace root: {error}"),
            Self::Reconciliation(error) => write!(formatter, "reconciliation failed: {error}"),
            Self::OperationActive(workspace_id) => {
                write!(
                    formatter,
                    "workspace has an active operation: {workspace_id}"
                )
            }
            Self::NotAutomatic(path) => write!(
                formatter,
                "workspace is not managed by the automatic workspace pool: {path}"
            ),
            Self::NotReusable(path) => {
                write!(formatter, "automatic workspace is not reusable: {path}")
            }
            Self::ClaimActive(workspace_id) => {
                write!(formatter, "workspace has an active claim: {workspace_id}")
            }
            Self::GeneratedPathUnavailable(path) => write!(
                formatter,
                "could not allocate a generated workspace path below {}",
                path.display()
            ),
            Self::ClaimNotFound(claim_id) => {
                write!(formatter, "workspace claim was not found: {claim_id}")
            }
            Self::RepositorySetMismatch(workspace_id) => write!(
                formatter,
                "repository set does not match workspace pool: {workspace_id}"
            ),
            Self::WorkspaceNotFound(path) => {
                write!(formatter, "managed workspace was not found: {path}")
            }
            Self::Json(error) => error.fmt(formatter),
            Self::AlreadyManaged(path) => write!(formatter, "workspace is already managed: {path}"),
            Self::Rollback { primary, rollback } => {
                write!(
                    formatter,
                    "workspace creation failed: {primary}; rollback failed: {rollback}"
                )
            }
            Self::RollbackFailure { errors } => {
                write!(
                    formatter,
                    "workspace rollback failed: {}",
                    errors.join("; ")
                )
            }
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "workspace filesystem operation failed for {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for WorkspaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(error) => Some(error),
            Self::Naming(error) => Some(error),
            Self::Git(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::DatabaseOpen(error) => Some(error),
            Self::Path(error) => Some(error),
            Self::Reconciliation(error) => Some(error),
            Self::OperationActive(_) => None,
            Self::NotAutomatic(_) => None,
            Self::NotReusable(_) => None,
            Self::ClaimActive(_) => None,
            Self::GeneratedPathUnavailable(_) => None,
            Self::ClaimNotFound(_) => None,
            Self::RepositorySetMismatch(_) => None,
            Self::WorkspaceNotFound(_) => None,
            Self::Json(error) => Some(error),
            Self::AlreadyManaged(_) => None,
            Self::Rollback { primary, .. } => Some(primary),
            Self::RollbackFailure { .. } => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

impl From<ValidationError> for WorkspaceError {
    fn from(error: ValidationError) -> Self {
        Self::Validation(error)
    }
}

impl From<NamingError> for WorkspaceError {
    fn from(error: NamingError) -> Self {
        Self::Naming(error)
    }
}

impl From<GitError> for WorkspaceError {
    fn from(error: GitError) -> Self {
        Self::Git(error)
    }
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
        finalize_persisted_creation(
            &mut connection,
            &context.workspace_id,
            &context.operation_id,
            &context.lease_id,
        )
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
        let root = test_root();
        let source = root.join("source");
        repository(&source);
        let database_path = root.join("state.sqlite");
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");
        let created = create_with_connection(
            &mut connection,
            prepare_create(&CreateRequest {
                workspace_path: root.join("workspace"),
                repositories: vec![source.clone()],
            })
            .expect("manual creation plan should be prepared"),
        )
        .expect("workspace should be created");
        let workspace =
            crate::storage::find_workspace_by_path(&mut connection, &created.workspace_path)
                .expect("workspace lookup should succeed")
                .expect("workspace should exist");
        let mut plan = prepare_automatic(&AutomaticCreateRequest {
            repositories: vec![source],
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
        let worktree_path = crate::storage::list_repo_worktrees(&mut connection, &candidate.id)
            .expect("worktree lookup should succeed")
            .into_iter()
            .next()
            .expect("candidate should have a worktree")
            .worktree_path
            .into_path_buf();
        (
            root,
            database_path,
            connection,
            plan,
            candidate,
            worktree_path,
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
    fn removes_a_claim_when_final_reconciliation_fails() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let dirty_path = worktree_path.join("local-change");
        let error = acquire_automatic_candidate_with_post_acquire_hook(
            &mut connection,
            &plan,
            &candidate,
            || {
                fs::write(&dirty_path, "dirty\n").expect("test worktree should become dirty");
            },
        )
        .expect_err("post-check dirty state should reject acquire");

        assert!(matches!(error, WorkspaceError::NotReusable(_)));
        assert!(
            crate::storage::find_workspace_claim(&mut connection, &candidate.id)
                .expect("claim lookup should succeed")
                .is_none()
        );
        assert_eq!(
            crate::storage::find_workspace(&mut connection, &candidate.id)
                .expect("workspace lookup should succeed")
                .state,
            WorkspaceState::Degraded
        );
        assert_eq!(
            crate::storage::list_repo_worktrees(&mut connection, &candidate.id)
                .expect("worktree lookup should succeed")[0]
                .state,
            RepoWorktreeState::Dirty
        );
        let acquire_event = crate::schema::lifecycle_events::table
            .filter(crate::schema::lifecycle_events::entity_id.eq(candidate.id.to_string()))
            .filter(crate::schema::lifecycle_events::event_type.eq("workspace_acquire_failed"))
            .select(crate::storage::EventRow::as_select())
            .first(&mut connection)
            .expect("acquire failure event should exist");
        assert_eq!(
            crate::storage::operation_state(&mut connection, &acquire_event.operation_id)
                .expect("acquire operation state should exist"),
            Some(OperationState::Failed)
        );

        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("dirty test worktree should be removable");
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
            Err(WorkspaceError::ClaimActive(workspace_id)) if workspace_id == candidate.id
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
        assert!(matches!(error, WorkspaceError::NotReusable(_)));
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
    fn rejects_release_with_a_wrong_claim_identifier() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let acquire = acquire_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("automatic acquire should succeed");
        let wrong_claim_id = ClaimId::new();

        assert!(matches!(
            release_automatic_workspace(&mut connection, &candidate.canonical_path, wrong_claim_id),
            Err(WorkspaceError::ClaimNotFound(claim_id)) if claim_id == wrong_claim_id
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
            WorkspaceError::NotReusable(path) => path.clone().into_path_buf(),
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
            WorkspaceError::OperationActive(workspace_id) if workspace_id == context.workspace_id
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
        assert!(matches!(error, WorkspaceError::AlreadyManaged(_)));
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
        let primary = WorkspaceError::AlreadyManaged(path.clone());
        let errors = [
            WorkspaceError::Validation(ValidationError::NoRepositories),
            WorkspaceError::Naming(NamingError::MissingRepositoryName(path.clone())),
            WorkspaceError::Git(GitError::WorktreeNotFound(path.as_path().to_owned())),
            WorkspaceError::Database(diesel::result::Error::NotFound),
            WorkspaceError::DatabaseOpen(crate::database::DatabaseError::Path(
                crate::paths::PathError::HomeDirectoryUnavailable,
            )),
            WorkspaceError::OperationActive(WorkspaceId::new()),
            WorkspaceError::NotAutomatic(path.clone()),
            WorkspaceError::NotReusable(path.clone()),
            WorkspaceError::ClaimActive(WorkspaceId::new()),
            WorkspaceError::GeneratedPathUnavailable(path.as_path().to_owned()),
            WorkspaceError::ClaimNotFound(ClaimId::new()),
            WorkspaceError::RepositorySetMismatch(WorkspaceId::new()),
            WorkspaceError::WorkspaceNotFound(path.clone()),
            WorkspaceError::Json(JsonDocument::parse("not json").unwrap_err()),
            primary,
            WorkspaceError::Rollback {
                primary: Box::new(WorkspaceError::AlreadyManaged(path.clone())),
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
