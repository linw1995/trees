use std::fmt;
use std::fs;
use std::path::PathBuf;

use diesel::sqlite::SqliteConnection;
use serde::Serialize;

use crate::domain::{
    CanonicalPath, CheckoutId, JsonDocument, OperationState, RepoWorktreeId, RepoWorktreeState,
    Timestamp, WorkspaceId, WorkspaceManagementMetadata, WorkspaceManagementMode, WorkspaceState,
};
use crate::git::{self, GitError};
use crate::lease::WorkspaceLease;
use crate::naming::{self, NamingError, WorktreePlan};
use crate::reconciliation::{self, ReconciliationError};
use crate::storage::{
    append_event, begin_operation, finalize_automatic_creation,
    finalize_creation as finalize_persisted_creation, find_workspace, find_workspace_by_path,
    find_workspace_lease_by_id, insert_managed_workspace, insert_repo_worktree,
    insert_workspace_lease, persist_operation_intent, persist_operation_step_intent,
    record_operation_transition, record_repo_worktree_transition, record_workspace_checkout,
    record_workspace_checkout_failure, record_workspace_transition, record_worktree_step_result,
    release_workspace_lease, with_short_transaction, EventDraft, NewManagedWorkspace,
    NewRepoWorktree, OperationIntent, OperationIntentError, TransitionMetadata,
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
    pub checkout_id: Option<CheckoutId>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct AutomaticAllocationPlan {
    pub workspace_root: CanonicalPath,
    pub pool_key: crate::pool::RepositorySetKey,
    pub repositories: Vec<AutomaticRepositoryPlan>,
    pub checkout_id: Option<CheckoutId>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct AutomaticRepositoryPlan {
    pub source_path: CanonicalPath,
    pub repository_identity: CanonicalPath,
    pub head: String,
}

#[derive(Debug, Clone)]
pub struct AutomaticCheckoutResult {
    pub workspace_path: CanonicalPath,
    pub pool_key: crate::pool::RepositorySetKey,
    pub checkout_id: CheckoutId,
    pub lease_expires_at: Timestamp,
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
    pub plan: RepositoryPlan,
}

#[derive(Debug, Clone)]
pub struct CreationContext {
    pub workspace_id: WorkspaceId,
    pub operation_id: crate::domain::OperationId,
    owner_id: String,
    pub plan: CreationPlan,
    pub repositories: Vec<TrackedRepository>,
}

pub fn prepare_automatic(
    request: &AutomaticCreateRequest,
) -> Result<AutomaticAllocationPlan, WorkspaceError> {
    let repositories = validation::validate_repositories(&request.repositories)?;
    let mut plans = Vec::with_capacity(repositories.len());
    let mut identities = Vec::with_capacity(repositories.len());
    for source_path in repositories {
        let info = git::inspect_repository(&source_path)?;
        identities.push(info.common_dir.clone());
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
        pool_key: crate::pool::RepositorySetKey::from_repositories(&identities),
        repositories: plans,
        checkout_id: request.checkout_id,
    })
}

pub fn find_idle_automatic_candidate(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
) -> Result<Option<crate::storage::WorkspaceRow>, WorkspaceError> {
    let candidates = crate::storage::list_automatic_workspace_candidates(
        connection,
        &plan.workspace_root,
        plan.pool_key.as_str(),
    )
    .map_err(WorkspaceError::Database)?;
    let mut idle_candidates = Vec::new();
    for workspace in candidates {
        if crate::storage::find_running_operation(connection, &workspace.id)
            .map_err(WorkspaceError::Database)?
            .is_some()
        {
            continue;
        }
        if crate::storage::find_workspace_lease(connection, &workspace.id)
            .map_err(WorkspaceError::Database)?
            .is_some()
        {
            continue;
        }
        idle_candidates.push(workspace);
    }
    let mut candidates = idle_candidates;
    candidates.sort_by(|left, right| {
        crate::pool::compare_candidates(
            &crate::pool::PoolCandidate {
                id: left.id,
                last_checked_in_at: left.last_checked_in_at.clone(),
                created_at: left.created_at.clone(),
            },
            &crate::pool::PoolCandidate {
                id: right.id,
                last_checked_in_at: right.last_checked_in_at.clone(),
                created_at: right.created_at.clone(),
            },
        )
    });
    Ok(candidates.into_iter().next())
}

pub fn checkout_automatic_candidate(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
    candidate: &crate::storage::WorkspaceRow,
) -> Result<AutomaticCheckoutResult, WorkspaceError> {
    checkout_automatic_candidate_with_post_checkout_hook(connection, plan, candidate, || {})
}

fn checkout_automatic_candidate_with_post_checkout_hook<F>(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
    candidate: &crate::storage::WorkspaceRow,
    post_checkout: F,
) -> Result<AutomaticCheckoutResult, WorkspaceError>
where
    F: FnOnce(),
{
    let owner_id = format!("process:{}", std::process::id());
    let intent_json = JsonDocument::from_serializable(plan).map_err(WorkspaceError::Json)?;
    let intent = OperationIntent::new(
        candidate.id,
        "checkout",
        owner_id,
        Timestamp::after_seconds(300),
        "acquire workspace lease",
        intent_json,
    );
    let operation = begin_operation(connection, &intent).map_err(map_operation_error)?;
    let boundary = match reconciliation::reconcile_workspace_for_access(
        connection,
        &candidate.id,
        &operation.id,
    ) {
        Ok(boundary) => boundary,
        Err(error) => {
            let primary = WorkspaceError::Reconciliation(error);
            fail_operation(connection, &operation.id, &primary);
            return Err(primary);
        }
    };
    if boundary.workspace.management_mode != crate::domain::WorkspaceManagementMode::Automatic {
        let primary = WorkspaceError::NotAutomatic(candidate.canonical_path.clone());
        fail_operation(connection, &operation.id, &primary);
        return Err(primary);
    }
    if boundary.summary.workspace_state != WorkspaceState::Ready {
        let primary = WorkspaceError::NotReusable(candidate.canonical_path.clone());
        fail_operation(connection, &operation.id, &primary);
        return Err(primary);
    }
    if boundary.lease.is_some() {
        let primary = WorkspaceError::LeaseActive(candidate.id);
        fail_operation(connection, &operation.id, &primary);
        return Err(primary);
    }

    let lease = WorkspaceLease::new(candidate.id, format!("process:{}", std::process::id()));
    let details_json = checkout_details(plan, &lease);
    if let Err(error) = record_workspace_checkout(
        connection,
        &operation.id,
        &lease,
        Some(details_json.clone()),
    ) {
        let primary = WorkspaceError::Database(error);
        return Err(fail_checkout(
            connection,
            &operation.id,
            &candidate.id,
            &lease.id,
            primary,
            Some(details_json),
        ));
    }

    post_checkout();

    let final_boundary = match reconciliation::reconcile_workspace_for_access(
        connection,
        &candidate.id,
        &operation.id,
    ) {
        Ok(boundary) => boundary,
        Err(error) => {
            let primary = WorkspaceError::Reconciliation(error);
            return Err(fail_checkout(
                connection,
                &operation.id,
                &candidate.id,
                &lease.id,
                primary,
                Some(details_json),
            ));
        }
    };
    if final_boundary.summary.workspace_state != WorkspaceState::Ready
        || final_boundary.lease.as_ref().map(|value| value.id) != Some(lease.id)
    {
        let primary = WorkspaceError::NotReusable(candidate.canonical_path.clone());
        return Err(fail_checkout(
            connection,
            &operation.id,
            &candidate.id,
            &lease.id,
            primary,
            Some(details_json),
        ));
    }

    Ok(AutomaticCheckoutResult {
        workspace_path: candidate.canonical_path.clone(),
        pool_key: plan.pool_key.clone(),
        checkout_id: lease.id,
        lease_expires_at: lease.lease_expires_at,
    })
}

fn checkout_details(plan: &AutomaticAllocationPlan, lease: &WorkspaceLease) -> JsonDocument {
    JsonDocument::from_serializable(&serde_json::json!({
        "pool_key": plan.pool_key,
        "workspace_root": plan.workspace_root,
        "lease": lease,
    }))
    .expect("checkout details should serialize")
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
    error: &WorkspaceError,
) {
    let _ = record_operation_transition(
        connection,
        operation_id,
        OperationState::Failed,
        "checkout failed",
        None,
        TransitionMetadata::new("operation_failed", "trees").with_error(error_document(error)),
    );
}

fn fail_checkout(
    connection: &mut SqliteConnection,
    operation_id: &crate::domain::OperationId,
    workspace_id: &WorkspaceId,
    checkout_id: &CheckoutId,
    primary: WorkspaceError,
    details_json: Option<JsonDocument>,
) -> WorkspaceError {
    let error_json = error_document(&primary);
    match record_workspace_checkout_failure(
        connection,
        operation_id,
        workspace_id,
        checkout_id,
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

pub fn renew_automatic(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
) -> Result<AutomaticCheckoutResult, WorkspaceError> {
    let checkout_id = plan
        .checkout_id
        .ok_or(WorkspaceError::RenewalRequiresCheckoutId)?;
    let lease_row = match find_workspace_lease_by_id(connection, &checkout_id) {
        Ok(lease) => lease,
        Err(diesel::result::Error::NotFound) => {
            return Err(WorkspaceError::CheckoutNotFound(checkout_id));
        }
        Err(error) => return Err(WorkspaceError::Database(error)),
    };
    let workspace =
        find_workspace(connection, &lease_row.workspace_id).map_err(WorkspaceError::Database)?;
    if workspace.management_mode != WorkspaceManagementMode::Automatic {
        return Err(WorkspaceError::NotAutomatic(workspace.canonical_path));
    }
    if workspace.pool_key.as_deref() != Some(plan.pool_key.as_str()) {
        return Err(WorkspaceError::RepositorySetMismatch(workspace.id));
    }
    if lease_row.lease_expires_at.has_expired() {
        return Err(WorkspaceError::LeaseExpired(checkout_id));
    }

    let intent_json = JsonDocument::from_serializable(plan).map_err(WorkspaceError::Json)?;
    let intent = OperationIntent::new(
        workspace.id,
        "checkout_renew",
        format!("process:{}", std::process::id()),
        Timestamp::after_seconds(300),
        "renew checkout lease",
        intent_json,
    );
    let operation = begin_operation(connection, &intent).map_err(map_operation_error)?;
    let boundary = match reconciliation::reconcile_workspace_for_access(
        connection,
        &workspace.id,
        &operation.id,
    ) {
        Ok(boundary) => boundary,
        Err(error) => {
            let primary = WorkspaceError::Reconciliation(error);
            fail_operation(connection, &operation.id, &primary);
            return Err(primary);
        }
    };
    if boundary.workspace.management_mode != WorkspaceManagementMode::Automatic {
        let primary = WorkspaceError::NotAutomatic(boundary.workspace.canonical_path);
        fail_operation(connection, &operation.id, &primary);
        return Err(primary);
    }
    if boundary.workspace.pool_key.as_deref() != Some(plan.pool_key.as_str()) {
        let primary = WorkspaceError::RepositorySetMismatch(boundary.workspace.id);
        fail_operation(connection, &operation.id, &primary);
        return Err(primary);
    }
    let Some(active_lease) = boundary.lease else {
        let primary = WorkspaceError::CheckoutNotFound(checkout_id);
        fail_operation(connection, &operation.id, &primary);
        return Err(primary);
    };
    if active_lease.id != checkout_id {
        let primary = WorkspaceError::CheckoutNotFound(checkout_id);
        fail_operation(connection, &operation.id, &primary);
        return Err(primary);
    }
    if active_lease.lease_expires_at.has_expired() {
        let primary = WorkspaceError::LeaseExpired(checkout_id);
        fail_operation(connection, &operation.id, &primary);
        return Err(primary);
    }

    let mut lease = WorkspaceLease::from(active_lease);
    lease.renew();
    let details_json = checkout_details(plan, &lease);
    if let Err(error) = crate::storage::record_workspace_lease_renewal(
        connection,
        &operation.id,
        &lease,
        Some(details_json),
    ) {
        let primary = WorkspaceError::Database(error);
        fail_operation(connection, &operation.id, &primary);
        return Err(primary);
    }

    Ok(AutomaticCheckoutResult {
        workspace_path: boundary.workspace.canonical_path,
        pool_key: plan.pool_key.clone(),
        checkout_id: lease.id,
        lease_expires_at: lease.lease_expires_at,
    })
}

pub fn provision_automatic(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
) -> Result<AutomaticCheckoutResult, WorkspaceError> {
    let Some(checkout_id) = plan.checkout_id else {
        return provision_automatic_new(connection, plan);
    };
    Err(WorkspaceError::ProvisioningHasCheckoutId(checkout_id))
}

fn provision_automatic_new(
    connection: &mut SqliteConnection,
    plan: &AutomaticAllocationPlan,
) -> Result<AutomaticCheckoutResult, WorkspaceError> {
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

    let (workspace_id, workspace_path) =
        next_generated_workspace(connection, normalized_plan.workspace_root.as_path())?;
    let creation_plan = automatic_creation_plan(&normalized_plan, workspace_path.clone())?;
    let lease = WorkspaceLease::new(workspace_id, format!("process:{}", std::process::id()));
    let management = WorkspaceManagementMetadata {
        mode: WorkspaceManagementMode::Automatic,
        pool_key: Some(normalized_plan.pool_key.as_str().to_owned()),
        workspace_root: Some(normalized_plan.workspace_root.clone()),
        last_checked_in_at: None,
        reclaimed_at: None,
    };
    let intent_json = JsonDocument::from_serializable(&serde_json::json!({
        "allocation": normalized_plan,
        "workspace_id": workspace_id,
        "workspace_path": workspace_path,
        "checkout_id": lease.id,
    }))
    .map_err(WorkspaceError::Json)?;
    let context = initialize_creation_with_metadata(
        connection,
        creation_plan,
        management,
        Some(&lease),
        intent_json,
    )?;

    if let Err(error) = reconciliation::reconcile_workspace(
        connection,
        &context.workspace_id,
        &context.operation_id,
    ) {
        let primary = WorkspaceError::Reconciliation(error);
        return fail_creation(connection, &context, &[], None, Some(&lease), primary)
            .map(|_| unreachable!("automatic provisioning rollback always fails the operation"));
    }
    execute_creation_with_lease(connection, &context, Some(&lease))?;
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
                Some(&lease),
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
            Some(&lease),
            primary,
        )
        .map(|_| unreachable!("automatic provisioning rollback always fails the operation"));
    }

    let details_json = checkout_details(&normalized_plan, &lease);
    if let Err(error) = finalize_automatic_creation(
        connection,
        &context.workspace_id,
        &context.operation_id,
        &lease,
        Some(details_json),
    ) {
        let primary = WorkspaceError::Database(error);
        let completed = context.repositories.iter().collect::<Vec<_>>();
        return fail_creation(
            connection,
            &context,
            &completed,
            None,
            Some(&lease),
            primary,
        )
        .map(|_| unreachable!("automatic provisioning rollback always fails the operation"));
    }

    Ok(AutomaticCheckoutResult {
        workspace_path: context.plan.workspace_path.clone(),
        pool_key: normalized_plan.pool_key,
        checkout_id: lease.id,
        lease_expires_at: lease.lease_expires_at,
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
    initialize_creation_with_metadata(
        connection,
        plan,
        WorkspaceManagementMetadata {
            mode: WorkspaceManagementMode::Manual,
            pool_key: None,
            workspace_root: None,
            last_checked_in_at: None,
            reclaimed_at: None,
        },
        None,
        intent_json,
    )
}

fn initialize_creation_with_metadata(
    connection: &mut SqliteConnection,
    plan: CreationPlan,
    management: WorkspaceManagementMetadata,
    lease: Option<&WorkspaceLease>,
    intent_json: JsonDocument,
) -> Result<CreationContext, WorkspaceError> {
    if find_workspace_by_path(connection, &plan.workspace_path)
        .map_err(WorkspaceError::Database)?
        .is_some()
    {
        return Err(WorkspaceError::AlreadyManaged(plan.workspace_path));
    }

    let workspace_id = lease
        .map(|lease| lease.workspace_id)
        .unwrap_or_else(WorkspaceId::new);
    let owner_id = format!("process:{}", std::process::id());
    let operation_intent = OperationIntent::new(
        workspace_id,
        "create",
        owner_id,
        Timestamp::after_seconds(300),
        "prepare worktrees",
        intent_json,
    );
    let repositories = plan
        .repositories
        .iter()
        .map(|repository| TrackedRepository {
            id: RepoWorktreeId::new(),
            plan: repository.clone(),
        })
        .collect::<Vec<_>>();
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
                management_mode: management.mode,
                pool_key: management.pool_key.clone(),
                workspace_root: management.workspace_root.clone(),
                last_checked_in_at: management.last_checked_in_at.clone(),
                reclaimed_at: management.reclaimed_at.clone(),
            },
        )?;
        for repository in &repositories {
            insert_repo_worktree(
                connection,
                &NewRepoWorktree {
                    id: repository.id,
                    workspace_id,
                    repository_identity: repository.plan.repository_identity.clone(),
                    source_path: repository.plan.source_path.clone(),
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
        if let Some(lease) = lease {
            insert_workspace_lease(connection, &crate::storage::NewWorkspaceLease::from(lease))?;
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
        append_event(
            connection,
            &EventDraft {
                operation_id: operation_intent.id,
                entity_type: "operation".to_owned(),
                entity_id: operation_intent.id.to_string(),
                event_type: "operation_started".to_owned(),
                source: "trees".to_owned(),
                occurred_at: now,
                previous_state: None,
                current_state: Some("running".to_owned()),
                details_json: None,
                error_json: None,
            },
        )?;
        Ok::<(), diesel::result::Error>(())
    })
    .map_err(WorkspaceError::Database)?;

    Ok(CreationContext {
        workspace_id,
        operation_id: operation_intent.id,
        owner_id: operation_intent.owner_id,
        plan,
        repositories,
    })
}

pub fn execute_creation(
    connection: &mut SqliteConnection,
    context: &CreationContext,
) -> Result<(), WorkspaceError> {
    execute_creation_with_lease(connection, context, None)
}

fn execute_creation_with_lease(
    connection: &mut SqliteConnection,
    context: &CreationContext,
    lease: Option<&WorkspaceLease>,
) -> Result<(), WorkspaceError> {
    if let Err(source) = fs::create_dir(&context.plan.workspace_path) {
        let primary = WorkspaceError::Io {
            path: context.plan.workspace_path.clone().into_path_buf(),
            source,
        };
        return fail_creation(connection, context, &[], None, lease, primary);
    }

    let mut completed = Vec::new();
    for repository in &context.repositories {
        if let Err(primary) = execute_repository_step(connection, context, repository) {
            return fail_creation(
                connection,
                context,
                &completed,
                Some(repository),
                lease,
                primary,
            );
        }
        completed.push(repository);
    }

    Ok(())
}

#[derive(Debug, Clone)]
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
    finalize_persisted_creation(connection, &context.workspace_id, &context.operation_id)
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
        format!("attach {}", repository.plan.source_path),
        intent_json,
    )
    .map_err(WorkspaceError::Database)?;
    let operation_id = context.operation_id;
    let owner_id = context.owner_id.clone();
    git::add_detached_worktree_with_heartbeat(
        &repository.plan.source_path,
        &repository.plan.worktree_path,
        || match crate::storage::renew_operation_lease(connection, &operation_id, &owner_id) {
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
        RepoWorktreeState::Attached,
        worktree.head,
        "worktree attached",
        TransitionMetadata::new("worktree_attached", "trees").with_details(
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
    lease: Option<&WorkspaceLease>,
    primary: WorkspaceError,
) -> Result<(), WorkspaceError> {
    match rollback_creation(connection, context, completed, failed, lease, &primary) {
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
    lease: Option<&WorkspaceLease>,
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

    if let Some(lease) = lease {
        match release_workspace_lease(connection, &context.workspace_id, &lease.id) {
            Ok(true) => {}
            Ok(false) => errors.push("automatic checkout lease was not found".to_owned()),
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
        operation_state,
        "rollback complete",
        None,
        TransitionMetadata::new(operation_event, "trees").with_error(error_json.clone()),
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
    LeaseActive(WorkspaceId),
    ProvisioningHasCheckoutId(CheckoutId),
    GeneratedPathUnavailable(PathBuf),
    RenewalRequiresCheckoutId,
    CheckoutNotFound(CheckoutId),
    LeaseExpired(CheckoutId),
    RepositorySetMismatch(WorkspaceId),
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
            Self::LeaseActive(workspace_id) => {
                write!(
                    formatter,
                    "workspace has an active checkout lease: {workspace_id}"
                )
            }
            Self::ProvisioningHasCheckoutId(checkout_id) => write!(
                formatter,
                "automatic provisioning cannot reuse checkout identifier: {checkout_id}"
            ),
            Self::GeneratedPathUnavailable(path) => write!(
                formatter,
                "could not allocate a generated workspace path below {}",
                path.display()
            ),
            Self::RenewalRequiresCheckoutId => {
                formatter.write_str("automatic lease renewal requires a checkout identifier")
            }
            Self::CheckoutNotFound(checkout_id) => {
                write!(formatter, "checkout lease was not found: {checkout_id}")
            }
            Self::LeaseExpired(checkout_id) => {
                write!(formatter, "checkout lease has expired: {checkout_id}")
            }
            Self::RepositorySetMismatch(workspace_id) => write!(
                formatter,
                "checkout repositories do not match workspace pool: {workspace_id}"
            ),
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
            Self::LeaseActive(_) => None,
            Self::ProvisioningHasCheckoutId(_) => None,
            Self::GeneratedPathUnavailable(_) => None,
            Self::RenewalRequiresCheckoutId => None,
            Self::CheckoutNotFound(_) => None,
            Self::LeaseExpired(_) => None,
            Self::RepositorySetMismatch(_) => None,
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
            crate::storage::find_operation(&mut connection, &context.operation_id)
                .unwrap()
                .state,
            OperationState::Succeeded
        );
        assert_eq!(
            crate::storage::list_events_for_operation(&mut connection, &context.operation_id)
                .unwrap()
                .len(),
            11
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
            checkout_id: None,
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
        assert_eq!(
            plan.checkout_id, None,
            "a new allocation should not carry a renewal identifier"
        );

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn selects_the_oldest_idle_automatic_candidate_for_a_pool_key() {
        let root = test_root();
        let source = root.join("source");
        repository(&source);
        let plan = prepare_automatic(&AutomaticCreateRequest {
            repositories: vec![source.clone()],
            checkout_id: None,
        })
        .expect("automatic allocation plan should be prepared");
        let database_path = root.join("state.sqlite");
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");
        let now = Timestamp::parse("2026-01-01T00:00:00Z").unwrap();
        let older = WorkspaceId::new();
        let newer = WorkspaceId::new();
        for (id, name, checked_in_at) in [
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
                    crate::schema::workspaces::pool_key.eq(Some(plan.pool_key.as_str())),
                    crate::schema::workspaces::workspace_root.eq(Some(plan.workspace_root.clone())),
                    crate::schema::workspaces::last_checked_in_at
                        .eq(Some(Timestamp::parse(checked_in_at).unwrap())),
                ))
                .execute(&mut connection)
                .expect("candidate metadata should be updated");
        }

        let candidate = find_idle_automatic_candidate(&mut connection, &plan)
            .expect("candidate lookup should succeed")
            .expect("an idle candidate should exist");
        assert_eq!(candidate.id, older);

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
            checkout_id: None,
        })
        .expect("automatic allocation plan should be prepared");
        plan.workspace_root =
            CanonicalPath::from_absolute(root.clone()).expect("workspace root should be absolute");
        diesel::update(crate::schema::workspaces::table.find(workspace.id))
            .set((
                crate::schema::workspaces::management_mode
                    .eq(crate::domain::WorkspaceManagementMode::Automatic),
                crate::schema::workspaces::pool_key.eq(Some(plan.pool_key.as_str())),
                crate::schema::workspaces::workspace_root.eq(Some(plan.workspace_root.clone())),
            ))
            .execute(&mut connection)
            .expect("workspace metadata should be updated");
        let candidate = find_idle_automatic_candidate(&mut connection, &plan)
            .expect("candidate lookup should succeed")
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
    fn checks_out_an_idle_automatic_candidate_and_records_its_lease() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let result = checkout_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("automatic checkout should succeed");

        assert_eq!(result.workspace_path, candidate.canonical_path);
        assert_eq!(result.pool_key, plan.pool_key);
        let lease = crate::storage::find_workspace_lease(&mut connection, &candidate.id)
            .expect("lease lookup should succeed")
            .expect("checkout lease should exist");
        assert_eq!(lease.id, result.checkout_id);
        assert_eq!(lease.lease_expires_at, result.lease_expires_at);
        assert_eq!(
            crate::storage::find_workspace(&mut connection, &candidate.id)
                .expect("workspace lookup should succeed")
                .state,
            WorkspaceState::Ready
        );
        let checkout_event = crate::schema::lifecycle_events::table
            .filter(crate::schema::lifecycle_events::entity_id.eq(candidate.id.to_string()))
            .filter(crate::schema::lifecycle_events::event_type.eq("workspace_checked_out"))
            .select(crate::storage::EventRow::as_select())
            .first(&mut connection)
            .expect("checkout event should exist");
        assert_eq!(
            crate::storage::find_operation(&mut connection, &checkout_event.operation_id)
                .expect("checkout operation should exist")
                .state,
            OperationState::Succeeded
        );

        crate::storage::release_workspace_lease(
            &mut connection,
            &candidate.id,
            &result.checkout_id,
        )
        .expect("checkout lease should be releasable");
        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn releases_a_checkout_lease_when_final_reconciliation_fails() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let dirty_path = worktree_path.join("local-change");
        let error = checkout_automatic_candidate_with_post_checkout_hook(
            &mut connection,
            &plan,
            &candidate,
            || {
                fs::write(&dirty_path, "dirty\n").expect("test worktree should become dirty");
            },
        )
        .expect_err("post-check dirty state should reject checkout");

        assert!(matches!(error, WorkspaceError::NotReusable(_)));
        assert!(
            crate::storage::find_workspace_lease(&mut connection, &candidate.id)
                .expect("lease lookup should succeed")
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
        let checkout_event = crate::schema::lifecycle_events::table
            .filter(crate::schema::lifecycle_events::entity_id.eq(candidate.id.to_string()))
            .filter(crate::schema::lifecycle_events::event_type.eq("workspace_checkout_failed"))
            .select(crate::storage::EventRow::as_select())
            .first(&mut connection)
            .expect("checkout failure event should exist");
        assert_eq!(
            crate::storage::find_operation(&mut connection, &checkout_event.operation_id)
                .expect("checkout operation should exist")
                .state,
            OperationState::Failed
        );

        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("dirty test worktree should be removable");
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
            checkout_id: None,
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
        assert_eq!(workspace.pool_key.as_deref(), Some(plan.pool_key.as_str()));
        assert_eq!(workspace.workspace_root, Some(canonical_workspace_root));
        assert_eq!(workspace.state, WorkspaceState::Ready);
        let repositories = crate::storage::list_repo_worktrees(&mut connection, &workspace.id)
            .expect("worktree lookup should succeed");
        assert_eq!(repositories.len(), 1);
        assert_eq!(repositories[0].state, RepoWorktreeState::Attached);
        assert!(repositories[0].worktree_path.as_path().is_dir());

        let lease = crate::storage::find_workspace_lease(&mut connection, &workspace.id)
            .expect("lease lookup should succeed")
            .expect("provisioned workspace should be checked out");
        assert_eq!(lease.id, result.checkout_id);
        let operation = crate::schema::operations::table
            .filter(crate::schema::operations::workspace_id.eq(workspace.id))
            .select(crate::storage::OperationRow::as_select())
            .first(&mut connection)
            .expect("provisioning operation should exist");
        assert_eq!(operation.state, OperationState::Succeeded);
        assert!(
            crate::storage::list_events_for_operation(&mut connection, &operation.id)
                .expect("provisioning events should be readable")
                .iter()
                .any(|event| event.event_type == "workspace_checked_out")
        );

        crate::storage::release_workspace_lease(
            &mut connection,
            &workspace.id,
            &result.checkout_id,
        )
        .expect("provisioning lease should be releasable");
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
            checkout_id: None,
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
            crate::storage::find_workspace_lease(&mut connection, &workspace.id)
                .expect("lease lookup should succeed")
                .is_none()
        );
        assert!(
            crate::storage::list_repo_worktrees(&mut connection, &workspace.id)
                .expect("worktree lookup should succeed")
                .iter()
                .all(|worktree| worktree.state == RepoWorktreeState::Failed)
        );
        let operation = crate::schema::operations::table
            .filter(crate::schema::operations::workspace_id.eq(workspace.id))
            .select(crate::storage::OperationRow::as_select())
            .first(&mut connection)
            .expect("provisioning operation should exist");
        assert_eq!(operation.state, OperationState::RolledBack);
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
    fn renews_an_automatic_checkout_with_the_same_identifier() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let checkout = checkout_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("automatic checkout should succeed");
        let original_expiry = crate::storage::find_workspace_lease(&mut connection, &candidate.id)
            .expect("lease lookup should succeed")
            .expect("checkout lease should exist")
            .lease_expires_at;
        let mut renewal_plan = plan.clone();
        renewal_plan.checkout_id = Some(checkout.checkout_id);

        let renewed = renew_automatic(&mut connection, &renewal_plan)
            .expect("automatic checkout renewal should succeed");
        assert_eq!(renewed.workspace_path, candidate.canonical_path);
        assert_eq!(renewed.pool_key, plan.pool_key);
        assert_eq!(renewed.checkout_id, checkout.checkout_id);
        assert!(renewed.lease_expires_at > original_expiry);
        let lease =
            crate::storage::find_workspace_lease_by_id(&mut connection, &checkout.checkout_id)
                .expect("renewed lease should exist");
        assert_eq!(lease.lease_expires_at, renewed.lease_expires_at);
        let renewal_event = crate::schema::lifecycle_events::table
            .filter(crate::schema::lifecycle_events::entity_id.eq(candidate.id.to_string()))
            .filter(crate::schema::lifecycle_events::event_type.eq("workspace_checkout_renewed"))
            .select(crate::storage::EventRow::as_select())
            .first(&mut connection)
            .expect("renewal event should exist");
        let operation =
            crate::storage::find_operation(&mut connection, &renewal_event.operation_id)
                .expect("renewal operation should exist");
        assert_eq!(operation.kind, "checkout_renew");
        assert_eq!(operation.state, OperationState::Succeeded);

        crate::storage::release_workspace_lease(
            &mut connection,
            &candidate.id,
            &checkout.checkout_id,
        )
        .expect("renewed lease should be releasable");
        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn rejects_checkout_renewal_for_a_different_repository_set() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let checkout = checkout_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("automatic checkout should succeed");
        let other_source = root.join("other-source");
        repository(&other_source);
        let mut renewal_plan = prepare_automatic(&AutomaticCreateRequest {
            repositories: vec![other_source],
            checkout_id: Some(checkout.checkout_id),
        })
        .expect("different repository set should be inspectable");
        renewal_plan.workspace_root = plan.workspace_root.clone();

        assert!(matches!(
            renew_automatic(&mut connection, &renewal_plan),
            Err(WorkspaceError::RepositorySetMismatch(workspace_id)) if workspace_id == candidate.id
        ));
        assert_eq!(
            crate::storage::find_workspace_lease(&mut connection, &candidate.id)
                .expect("lease lookup should succeed")
                .expect("original lease should remain active")
                .id,
            checkout.checkout_id
        );

        crate::storage::release_workspace_lease(
            &mut connection,
            &candidate.id,
            &checkout.checkout_id,
        )
        .expect("original lease should be releasable");
        crate::git::remove_worktree(&plan.repositories[0].source_path, &worktree_path)
            .expect("test worktree should be removable");
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn rejects_renewal_of_an_expired_checkout_without_releasing_it() {
        let (root, database_path, mut connection, plan, candidate, worktree_path) =
            automatic_candidate_fixture();
        let checkout = checkout_automatic_candidate(&mut connection, &plan, &candidate)
            .expect("automatic checkout should succeed");
        diesel::update(crate::schema::workspace_leases::table.find(checkout.checkout_id))
            .set(crate::schema::workspace_leases::lease_expires_at.eq(Timestamp::now()))
            .execute(&mut connection)
            .expect("lease should be expired");
        let mut renewal_plan = plan;
        renewal_plan.checkout_id = Some(checkout.checkout_id);

        assert!(matches!(
            renew_automatic(&mut connection, &renewal_plan),
            Err(WorkspaceError::LeaseExpired(checkout_id)) if checkout_id == checkout.checkout_id
        ));
        assert!(
            crate::storage::find_workspace_lease(&mut connection, &candidate.id)
                .expect("lease lookup should succeed")
                .is_some()
        );

        crate::storage::release_workspace_lease(
            &mut connection,
            &candidate.id,
            &checkout.checkout_id,
        )
        .expect("expired lease should be releasable for test cleanup");
        crate::git::remove_worktree(&renewal_plan.repositories[0].source_path, &worktree_path)
            .expect("test worktree should be removable");
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
            crate::storage::find_operation(&mut connection, &context.operation_id)
                .unwrap()
                .state,
            OperationState::RolledBack
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
            crate::storage::find_operation(&mut connection, &context.operation_id)
                .unwrap()
                .state,
            OperationState::Running
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
        diesel::update(crate::schema::operations::table.find(&context.operation_id))
            .set(crate::schema::operations::lease_expires_at.eq(Timestamp::now()))
            .execute(&mut connection)
            .expect("operation lease should expire");

        let error = create_with_connection(&mut connection, context.plan.clone())
            .expect_err("recovered workspace should remain managed");
        assert!(matches!(error, WorkspaceError::AlreadyManaged(_)));
        assert_eq!(
            crate::storage::find_operation(&mut connection, &context.operation_id)
                .unwrap()
                .state,
            OperationState::Succeeded
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
            WorkspaceError::LeaseActive(WorkspaceId::new()),
            WorkspaceError::ProvisioningHasCheckoutId(CheckoutId::new()),
            WorkspaceError::GeneratedPathUnavailable(path.as_path().to_owned()),
            WorkspaceError::RenewalRequiresCheckoutId,
            WorkspaceError::CheckoutNotFound(CheckoutId::new()),
            WorkspaceError::LeaseExpired(CheckoutId::new()),
            WorkspaceError::RepositorySetMismatch(WorkspaceId::new()),
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
