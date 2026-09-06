use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error, QueryResult};
use diesel::sqlite::SqliteConnection;
use std::fmt;

use crate::claim::WorkspaceClaim;
use crate::domain::{
    CanonicalPath, ClaimId, JsonDocument, LeaseId, OperationId, OperationState, OriginRepositoryId,
    PoolId, RepoWorktreeId, RepoWorktreeState, Timestamp, WorkspaceId, WorkspaceManagementMode,
    WorkspaceState,
};
use crate::pool::RepositorySetKey;
use crate::schema::{
    lifecycle_events, operation_leases, operations, origin_repositories, repo_worktrees,
    workspace_claims, workspace_pool_repositories, workspace_pools, workspaces,
};

use super::models::{
    EventRow, NewEvent, NewManagedWorkspace, NewOperation, NewOperationLease, NewOriginRepository,
    NewRepoWorktree, NewWorkspace, NewWorkspaceClaim, NewWorkspacePool, NewWorkspacePoolRepository,
    OperationIntent, OperationLeaseRow, OperationRow, OriginRepositoryRow, RepoWorktreeRow,
    WorkspaceClaimRow, WorkspacePoolRepositoryRow, WorkspacePoolRow, WorkspaceRow,
};
use super::transaction::with_short_transaction;

#[derive(Debug, Clone)]
pub struct TransitionMetadata {
    pub event_type: String,
    pub source: String,
    pub pending_step: Option<String>,
    pub details_json: Option<JsonDocument>,
    pub error_json: Option<JsonDocument>,
}

impl TransitionMetadata {
    pub fn new(event_type: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            source: source.into(),
            pending_step: None,
            details_json: None,
            error_json: None,
        }
    }

    pub fn with_pending_step(mut self, pending_step: impl Into<String>) -> Self {
        self.pending_step = Some(pending_step.into());
        self
    }

    pub fn with_details(mut self, details_json: JsonDocument) -> Self {
        self.details_json = Some(details_json);
        self
    }

    pub fn with_error(mut self, error_json: JsonDocument) -> Self {
        self.error_json = Some(error_json);
        self
    }
}

#[derive(Debug, Clone)]
pub struct EventDraft {
    pub operation_id: OperationId,
    pub entity_type: String,
    pub entity_id: String,
    pub event_type: String,
    pub source: String,
    pub occurred_at: Timestamp,
    pub previous_state: Option<String>,
    pub current_state: Option<String>,
    pub details_json: Option<JsonDocument>,
    pub error_json: Option<JsonDocument>,
}

pub fn insert_workspace(
    connection: &mut SqliteConnection,
    value: &NewWorkspace,
) -> QueryResult<WorkspaceRow> {
    diesel::insert_into(workspaces::table)
        .values(value)
        .execute(connection)?;
    workspaces::table
        .find(&value.id)
        .select(WorkspaceRow::as_select())
        .first(connection)
}

pub fn insert_managed_workspace(
    connection: &mut SqliteConnection,
    value: &NewManagedWorkspace,
) -> QueryResult<WorkspaceRow> {
    diesel::insert_into(workspaces::table)
        .values(value)
        .execute(connection)?;
    workspaces::table
        .find(&value.id)
        .select(WorkspaceRow::as_select())
        .first(connection)
}

pub fn find_workspace_by_path(
    connection: &mut SqliteConnection,
    path: &CanonicalPath,
) -> QueryResult<Option<WorkspaceRow>> {
    workspaces::table
        .filter(workspaces::canonical_path.eq(path))
        .select(WorkspaceRow::as_select())
        .first(connection)
        .optional()
}

pub fn find_workspace(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
) -> QueryResult<WorkspaceRow> {
    workspaces::table
        .find(workspace_id)
        .select(WorkspaceRow::as_select())
        .first(connection)
}

pub fn find_origin_repository_by_identity(
    connection: &mut SqliteConnection,
    repository_identity: &CanonicalPath,
) -> QueryResult<Option<OriginRepositoryRow>> {
    origin_repositories::table
        .filter(origin_repositories::repository_identity.eq(repository_identity))
        .select(OriginRepositoryRow::as_select())
        .first(connection)
        .optional()
}

pub fn insert_origin_repository(
    connection: &mut SqliteConnection,
    value: &NewOriginRepository,
) -> QueryResult<OriginRepositoryRow> {
    diesel::insert_into(origin_repositories::table)
        .values(value)
        .execute(connection)?;
    origin_repositories::table
        .find(&value.id)
        .select(OriginRepositoryRow::as_select())
        .first(connection)
}

pub fn ensure_origin_repository(
    connection: &mut SqliteConnection,
    repository_identity: &CanonicalPath,
    source_path: &CanonicalPath,
) -> QueryResult<OriginRepositoryRow> {
    if let Some(repository) = find_origin_repository_by_identity(connection, repository_identity)? {
        if repository.source_path != *source_path {
            diesel::update(origin_repositories::table.find(repository.id))
                .set(origin_repositories::source_path.eq(source_path))
                .execute(connection)?;
            return origin_repositories::table
                .find(repository.id)
                .select(OriginRepositoryRow::as_select())
                .first(connection);
        }
        return Ok(repository);
    }

    let value = NewOriginRepository {
        id: OriginRepositoryId::new(),
        repository_identity: repository_identity.clone(),
        source_path: source_path.clone(),
    };
    match insert_origin_repository(connection, &value) {
        Ok(repository) => Ok(repository),
        Err(Error::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
            find_origin_repository_by_identity(connection, repository_identity)?
                .ok_or(Error::NotFound)
        }
        Err(error) => Err(error),
    }
}

pub fn find_workspace_pool(
    connection: &mut SqliteConnection,
    repository_set: &RepositorySetKey,
) -> QueryResult<Option<WorkspacePoolRow>> {
    workspace_pools::table
        .filter(workspace_pools::hash_key.eq(repository_set.hash_key()))
        .filter(workspace_pools::repository_ids.eq(repository_set.repository_ids()))
        .select(WorkspacePoolRow::as_select())
        .first(connection)
        .optional()
}

pub fn find_workspace_pool_by_id(
    connection: &mut SqliteConnection,
    pool_id: &PoolId,
) -> QueryResult<WorkspacePoolRow> {
    workspace_pools::table
        .find(pool_id)
        .select(WorkspacePoolRow::as_select())
        .first(connection)
}

pub fn insert_workspace_pool(
    connection: &mut SqliteConnection,
    value: &NewWorkspacePool,
) -> QueryResult<WorkspacePoolRow> {
    diesel::insert_into(workspace_pools::table)
        .values(value)
        .execute(connection)?;
    workspace_pools::table
        .find(&value.id)
        .select(WorkspacePoolRow::as_select())
        .first(connection)
}

pub fn ensure_workspace_pool(
    connection: &mut SqliteConnection,
    repository_set: &RepositorySetKey,
) -> QueryResult<WorkspacePoolRow> {
    if let Some(pool) = find_workspace_pool(connection, repository_set)? {
        return Ok(pool);
    }

    // Serialize only the lookup-and-create window because repository_ids is a
    // collision-check payload rather than a database uniqueness constraint.
    connection.immediate_transaction(|connection| {
        if let Some(pool) = find_workspace_pool(connection, repository_set)? {
            return Ok(pool);
        }

        insert_workspace_pool(
            connection,
            &NewWorkspacePool {
                id: PoolId::new(),
                hash_key: repository_set.hash_key().to_owned(),
                repository_ids: repository_set.repository_ids().to_owned(),
            },
        )
    })
}

pub fn insert_workspace_pool_repositories(
    connection: &mut SqliteConnection,
    values: &[NewWorkspacePoolRepository],
) -> QueryResult<usize> {
    if values.is_empty() {
        return Ok(0);
    }
    let mut inserted = 0;
    for value in values {
        inserted += diesel::insert_into(workspace_pool_repositories::table)
            .values(value)
            .on_conflict_do_nothing()
            .execute(connection)?;
    }
    Ok(inserted)
}

pub fn list_workspace_pool_repositories(
    connection: &mut SqliteConnection,
    pool_id: &PoolId,
) -> QueryResult<Vec<WorkspacePoolRepositoryRow>> {
    workspace_pool_repositories::table
        .filter(workspace_pool_repositories::pool_id.eq(pool_id))
        .order(workspace_pool_repositories::repository_id.asc())
        .select(WorkspacePoolRepositoryRow::as_select())
        .load(connection)
}

pub fn list_automatic_workspace_candidates(
    connection: &mut SqliteConnection,
    pool_id: &PoolId,
) -> QueryResult<Vec<WorkspaceRow>> {
    workspaces::table
        .filter(workspaces::management_mode.eq(WorkspaceManagementMode::Automatic))
        .filter(workspaces::pool_id.eq(Some(pool_id)))
        .filter(workspaces::state.eq(WorkspaceState::Ready))
        .order(workspaces::id.asc())
        .select(WorkspaceRow::as_select())
        .load(connection)
}

pub fn list_automatic_workspaces(
    connection: &mut SqliteConnection,
) -> QueryResult<Vec<WorkspaceRow>> {
    workspaces::table
        .filter(workspaces::management_mode.eq(WorkspaceManagementMode::Automatic))
        .order(workspaces::id.asc())
        .select(WorkspaceRow::as_select())
        .load(connection)
}

pub fn insert_workspace_claim(
    connection: &mut SqliteConnection,
    value: &NewWorkspaceClaim,
) -> QueryResult<WorkspaceClaimRow> {
    diesel::insert_into(workspace_claims::table)
        .values(value)
        .execute(connection)?;
    workspace_claims::table
        .find(&value.id)
        .select(WorkspaceClaimRow::as_select())
        .first(connection)
}

pub fn find_workspace_claim(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
) -> QueryResult<Option<WorkspaceClaimRow>> {
    workspace_claims::table
        .filter(workspace_claims::workspace_id.eq(workspace_id))
        .select(WorkspaceClaimRow::as_select())
        .first(connection)
        .optional()
}

pub fn find_workspace_claim_by_id(
    connection: &mut SqliteConnection,
    claim_id: &ClaimId,
) -> QueryResult<WorkspaceClaimRow> {
    workspace_claims::table
        .find(claim_id)
        .select(WorkspaceClaimRow::as_select())
        .first(connection)
}

pub fn release_workspace_claim(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    claim_id: &ClaimId,
) -> QueryResult<bool> {
    // Claim absence is the idle signal; release history is stored separately.
    let deleted = diesel::delete(
        workspace_claims::table
            .filter(workspace_claims::workspace_id.eq(workspace_id))
            .filter(workspace_claims::id.eq(claim_id)),
    )
    .execute(connection)?;
    Ok(deleted == 1)
}

pub fn record_workspace_acquire(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    claim: &WorkspaceClaim,
    details_json: Option<JsonDocument>,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        let operation_id = operation_id_for_lease(connection, lease_id)?;
        insert_workspace_claim(connection, &NewWorkspaceClaim::from(claim))?;
        let workspace = workspaces::table
            .find(&claim.workspace_id)
            .select(WorkspaceRow::as_select())
            .first(connection)?;
        append_event(
            connection,
            &workspace_access_event(&operation_id, &workspace, "workspace_claimed", details_json),
        )?;
        Ok(())
    })
}

pub fn record_workspace_acquire_failure(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    workspace_id: &WorkspaceId,
    claim_id: &ClaimId,
    details_json: Option<JsonDocument>,
    error_json: JsonDocument,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        let operation_id = operation_id_for_lease(connection, lease_id)?;
        let workspace = workspaces::table
            .find(workspace_id)
            .select(WorkspaceRow::as_select())
            .first(connection)?;
        diesel::delete(
            workspace_claims::table
                .filter(workspace_claims::workspace_id.eq(workspace_id))
                .filter(workspace_claims::id.eq(claim_id)),
        )
        .execute(connection)?;
        finish_operation_in_transaction(
            connection,
            lease_id,
            OperationState::Failed,
            TransitionMetadata::new("operation_failed", "trees")
                .with_pending_step("acquire failed")
                .with_details(details_json.clone().unwrap_or_else(empty_json))
                .with_error(error_json.clone()),
        )?;
        append_event(
            connection,
            &EventDraft {
                operation_id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace.id.to_string(),
                event_type: "workspace_acquire_failed".to_owned(),
                source: "trees".to_owned(),
                occurred_at: Timestamp::now(),
                previous_state: Some(workspace.state.to_string()),
                current_state: Some(workspace.state.to_string()),
                details_json,
                error_json: Some(error_json),
            },
        )?;
        Ok(())
    })
}

pub fn record_workspace_release(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    workspace_id: &WorkspaceId,
    claim_id: &ClaimId,
    details_json: Option<JsonDocument>,
) -> QueryResult<()> {
    // Keep claim release, idle timestamp, operation completion, and the event
    // append atomic so no reader observes a reusable workspace without history.
    with_short_transaction(connection, |connection| {
        let operation_id = operation_id_for_lease(connection, lease_id)?;
        let workspace = workspaces::table
            .find(workspace_id)
            .select(WorkspaceRow::as_select())
            .first(connection)?;
        if !release_workspace_claim(connection, workspace_id, claim_id)? {
            return Err(Error::NotFound);
        }
        let occurred_at = Timestamp::now();
        diesel::update(workspaces::table.find(workspace_id))
            .set((
                workspaces::last_released_at.eq(&occurred_at),
                workspaces::updated_at.eq(&occurred_at),
            ))
            .execute(connection)?;
        finish_operation_in_transaction(
            connection,
            lease_id,
            OperationState::Succeeded,
            TransitionMetadata::new("operation_succeeded", "trees")
                .with_pending_step("release complete")
                .with_details(details_json.clone().unwrap_or_else(empty_json)),
        )?;
        append_event(
            connection,
            &workspace_access_event(
                &operation_id,
                &workspace,
                "workspace_released",
                details_json,
            ),
        )?;
        Ok(())
    })
}

pub fn record_workspace_release_rejection(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    workspace_id: &WorkspaceId,
    details_json: Option<JsonDocument>,
    error_json: JsonDocument,
) -> QueryResult<()> {
    // Rejection leaves the claim intact while its holder repairs the workspace.
    with_short_transaction(connection, |connection| {
        let operation_id = operation_id_for_lease(connection, lease_id)?;
        let workspace = workspaces::table
            .find(workspace_id)
            .select(WorkspaceRow::as_select())
            .first(connection)?;
        finish_operation_in_transaction(
            connection,
            lease_id,
            OperationState::Failed,
            TransitionMetadata::new("operation_failed", "trees")
                .with_pending_step("release rejected")
                .with_details(details_json.clone().unwrap_or_else(empty_json))
                .with_error(error_json.clone()),
        )?;
        append_event(
            connection,
            &EventDraft {
                operation_id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace.id.to_string(),
                event_type: "workspace_release_rejected".to_owned(),
                source: "trees".to_owned(),
                occurred_at: Timestamp::now(),
                previous_state: Some(workspace.state.to_string()),
                current_state: Some(workspace.state.to_string()),
                details_json,
                error_json: Some(error_json),
            },
        )?;
        Ok(())
    })
}

pub fn record_workspace_reclaimed(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    workspace_id: &WorkspaceId,
    details_json: Option<JsonDocument>,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        let operation_id = operation_id_for_lease(connection, lease_id)?;
        let workspace = workspaces::table
            .find(workspace_id)
            .select(WorkspaceRow::as_select())
            .first(connection)?;
        let repositories = list_repo_worktrees(connection, workspace_id)?;
        let occurred_at = Timestamp::now();
        for repository in &repositories {
            diesel::update(repo_worktrees::table.find(&repository.id))
                .set((
                    repo_worktrees::state.eq(RepoWorktreeState::Reclaimed),
                    repo_worktrees::last_observed_at.eq(&occurred_at),
                ))
                .execute(connection)?;
            append_event(
                connection,
                &EventDraft {
                    operation_id,
                    entity_type: "repo_worktree".to_owned(),
                    entity_id: repository.id.to_string(),
                    event_type: "worktree_reclaimed".to_owned(),
                    source: "trees".to_owned(),
                    occurred_at: occurred_at.clone(),
                    previous_state: Some(repository.state.to_string()),
                    current_state: Some(RepoWorktreeState::Reclaimed.to_string()),
                    details_json: details_json.clone(),
                    error_json: None,
                },
            )?;
        }
        diesel::update(workspaces::table.find(workspace_id))
            .set((
                workspaces::state.eq(WorkspaceState::Reclaimed),
                workspaces::updated_at.eq(&occurred_at),
                workspaces::last_reconciled_at.eq(&occurred_at),
                workspaces::reclaimed_at.eq(&occurred_at),
            ))
            .execute(connection)?;
        finish_operation_in_transaction(
            connection,
            lease_id,
            OperationState::Succeeded,
            TransitionMetadata::new("operation_succeeded", "trees")
                .with_pending_step("garbage collection complete")
                .with_details(details_json.clone().unwrap_or_else(empty_json)),
        )?;
        append_event(
            connection,
            &EventDraft {
                operation_id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace.id.to_string(),
                event_type: "workspace_reclaimed".to_owned(),
                source: "trees".to_owned(),
                occurred_at,
                previous_state: Some(workspace.state.to_string()),
                current_state: Some(WorkspaceState::Reclaimed.to_string()),
                details_json,
                error_json: None,
            },
        )?;
        Ok(())
    })
}

pub fn record_workspace_gc_skipped(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    workspace_id: &WorkspaceId,
    details_json: Option<JsonDocument>,
    error_json: JsonDocument,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        let operation_id = operation_id_for_lease(connection, lease_id)?;
        let workspace = workspaces::table
            .find(workspace_id)
            .select(WorkspaceRow::as_select())
            .first(connection)?;
        finish_operation_in_transaction(
            connection,
            lease_id,
            OperationState::Failed,
            TransitionMetadata::new("operation_failed", "gc")
                .with_pending_step("GC skipped workspace")
                .with_details(details_json.clone().unwrap_or_else(empty_json))
                .with_error(error_json.clone()),
        )?;
        append_event(
            connection,
            &EventDraft {
                operation_id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace.id.to_string(),
                event_type: "workspace_gc_skipped".to_owned(),
                source: "gc".to_owned(),
                occurred_at: Timestamp::now(),
                previous_state: Some(workspace.state.to_string()),
                current_state: Some(workspace.state.to_string()),
                details_json,
                error_json: Some(error_json),
            },
        )?;
        Ok(())
    })
}

pub fn record_workspace_gc_failure(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    workspace_id: &WorkspaceId,
    details_json: Option<JsonDocument>,
    error_json: JsonDocument,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        let operation_id = operation_id_for_lease(connection, lease_id)?;
        let workspace = workspaces::table
            .find(workspace_id)
            .select(WorkspaceRow::as_select())
            .first(connection)?;
        let occurred_at = Timestamp::now();
        diesel::update(workspaces::table.find(workspace_id))
            .set((
                workspaces::state.eq(WorkspaceState::Failed),
                workspaces::updated_at.eq(&occurred_at),
                workspaces::last_reconciled_at.eq(&occurred_at),
            ))
            .execute(connection)?;
        finish_operation_in_transaction(
            connection,
            lease_id,
            OperationState::Failed,
            TransitionMetadata::new("operation_failed", "gc")
                .with_pending_step("GC failed")
                .with_details(details_json.clone().unwrap_or_else(empty_json))
                .with_error(error_json.clone()),
        )?;
        append_event(
            connection,
            &EventDraft {
                operation_id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace.id.to_string(),
                event_type: "workspace_gc_failed".to_owned(),
                source: "gc".to_owned(),
                occurred_at,
                previous_state: Some(workspace.state.to_string()),
                current_state: Some(WorkspaceState::Failed.to_string()),
                details_json,
                error_json: Some(error_json),
            },
        )?;
        Ok(())
    })
}

fn workspace_access_event(
    operation_id: &OperationId,
    workspace: &WorkspaceRow,
    event_type: &str,
    details_json: Option<JsonDocument>,
) -> EventDraft {
    EventDraft {
        operation_id: *operation_id,
        entity_type: "workspace".to_owned(),
        entity_id: workspace.id.to_string(),
        event_type: event_type.to_owned(),
        source: "trees".to_owned(),
        occurred_at: Timestamp::now(),
        previous_state: Some(workspace.state.to_string()),
        current_state: Some(workspace.state.to_string()),
        details_json,
        error_json: None,
    }
}

fn empty_json() -> JsonDocument {
    JsonDocument::parse("{}").expect("empty JSON object should parse")
}

pub fn insert_repo_worktree(
    connection: &mut SqliteConnection,
    value: &NewRepoWorktree,
) -> QueryResult<RepoWorktreeRow> {
    diesel::insert_into(repo_worktrees::table)
        .values(value)
        .execute(connection)?;
    repo_worktrees::table
        .inner_join(origin_repositories::table)
        .filter(repo_worktrees::id.eq(&value.id))
        .select((
            repo_worktrees::id,
            repo_worktrees::workspace_id,
            origin_repositories::id,
            origin_repositories::repository_identity,
            origin_repositories::source_path,
            repo_worktrees::worktree_path,
            repo_worktrees::state,
            repo_worktrees::last_head,
            repo_worktrees::last_observed_at,
        ))
        .first(connection)
}

pub fn list_repo_worktrees(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
) -> QueryResult<Vec<RepoWorktreeRow>> {
    repo_worktrees::table
        .inner_join(origin_repositories::table)
        .filter(repo_worktrees::workspace_id.eq(workspace_id))
        .order(repo_worktrees::worktree_path.asc())
        .select((
            repo_worktrees::id,
            repo_worktrees::workspace_id,
            origin_repositories::id,
            origin_repositories::repository_identity,
            origin_repositories::source_path,
            repo_worktrees::worktree_path,
            repo_worktrees::state,
            repo_worktrees::last_head,
            repo_worktrees::last_observed_at,
        ))
        .load(connection)
}

pub fn insert_operation(
    connection: &mut SqliteConnection,
    value: &NewOperation,
) -> QueryResult<OperationRow> {
    diesel::insert_into(operations::table)
        .values(value)
        .execute(connection)?;
    operations::table
        .find(&value.id)
        .select(OperationRow::as_select())
        .first(connection)
}

pub fn insert_operation_lease(
    connection: &mut SqliteConnection,
    value: &NewOperationLease,
) -> QueryResult<OperationLeaseRow> {
    diesel::insert_into(operation_leases::table)
        .values(value)
        .execute(connection)?;
    operation_leases::table
        .find(&value.id)
        .select(OperationLeaseRow::as_select())
        .first(connection)
}

pub fn find_operation_lease(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
) -> QueryResult<Option<OperationLeaseRow>> {
    operation_leases::table
        .filter(operation_leases::operation_id.eq(operation_id))
        .select(OperationLeaseRow::as_select())
        .first(connection)
        .optional()
}

/// Resolves the immutable operation identity owned by the current lease token.
fn operation_id_for_lease(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
) -> QueryResult<OperationId> {
    operation_leases::table
        .find(lease_id)
        .select(operation_leases::operation_id)
        .first(connection)
}

pub fn persist_operation_intent(
    connection: &mut SqliteConnection,
    intent: &OperationIntent,
) -> QueryResult<OperationRow> {
    let operation = insert_operation(
        connection,
        &NewOperation {
            id: intent.id,
            workspace_id: intent.workspace_id,
            kind: intent.kind.clone(),
            started_at: intent.started_at.clone(),
            intent_json: intent.intent_json.clone(),
        },
    )?;
    insert_operation_lease(
        connection,
        &NewOperationLease {
            id: intent.lease_id,
            operation_id: intent.id,
            workspace_id: intent.workspace_id,
            lease_expires_at: intent.lease_expires_at.clone(),
        },
    )?;
    append_operation_event(
        connection,
        intent.id,
        None,
        OperationState::Running,
        TransitionMetadata::new("operation_started", "trees")
            .with_pending_step(intent.pending_step.clone()),
    )?;
    Ok(operation)
}

pub fn begin_operation(
    connection: &mut SqliteConnection,
    intent: &OperationIntent,
) -> Result<OperationRow, OperationIntentError> {
    with_short_transaction(connection, |connection| {
        persist_operation_intent(connection, intent)
    })
    .map_err(|error| match error {
        Error::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
            OperationIntentError::WorkspaceBusy(intent.workspace_id)
        }
        error => OperationIntentError::Database(error),
    })
}

#[derive(Debug)]
pub enum OperationIntentError {
    WorkspaceBusy(WorkspaceId),
    Database(diesel::result::Error),
}

impl fmt::Display for OperationIntentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkspaceBusy(workspace_id) => {
                write!(
                    formatter,
                    "workspace already has a running operation: {workspace_id}"
                )
            }
            Self::Database(error) => {
                write!(formatter, "failed to persist operation intent: {error}")
            }
        }
    }
}

impl std::error::Error for OperationIntentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::WorkspaceBusy(_) => None,
            Self::Database(error) => Some(error),
        }
    }
}

pub fn find_operation(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
) -> QueryResult<OperationRow> {
    operations::table
        .find(operation_id)
        .select(OperationRow::as_select())
        .first(connection)
}

pub fn find_latest_operation_event(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
) -> QueryResult<Option<EventRow>> {
    lifecycle_events::table
        .filter(lifecycle_events::operation_id.eq(operation_id))
        .filter(lifecycle_events::entity_type.eq("operation"))
        .order((
            lifecycle_events::occurred_at.desc(),
            lifecycle_events::event_id.desc(),
        ))
        .select(EventRow::as_select())
        .first(connection)
        .optional()
}

pub fn operation_state(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
) -> QueryResult<Option<OperationState>> {
    let Some(event) = find_latest_operation_event(connection, operation_id)? else {
        return Ok(None);
    };
    event
        .current_state
        .as_deref()
        .map(|state| {
            state
                .parse()
                .map_err(|error| Error::DeserializationError(Box::new(error)))
        })
        .transpose()
}

pub fn find_running_operation(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
) -> QueryResult<Option<super::models::RunningOperation>> {
    let Some(lease) = operation_leases::table
        .filter(operation_leases::workspace_id.eq(workspace_id))
        .select(OperationLeaseRow::as_select())
        .first(connection)
        .optional()?
    else {
        return Ok(None);
    };
    let operation = find_operation(connection, &lease.operation_id)?;
    if operation_state(connection, &operation.id)?.unwrap_or(OperationState::Running)
        != OperationState::Running
    {
        return Ok(None);
    }
    Ok(Some(super::models::RunningOperation { operation, lease }))
}

pub fn claim_expired_operation(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    lease_expires_at: &Timestamp,
    new_lease_id: &LeaseId,
    new_lease_expires_at: &Timestamp,
) -> QueryResult<bool> {
    let now = Timestamp::now();
    let updated = diesel::update(
        operation_leases::table
            .filter(operation_leases::id.eq(lease_id))
            .filter(operation_leases::lease_expires_at.eq(lease_expires_at))
            .filter(operation_leases::lease_expires_at.le(&now)),
    )
    .set((
        operation_leases::id.eq(new_lease_id),
        operation_leases::lease_expires_at.eq(new_lease_expires_at),
    ))
    .execute(connection)?;

    Ok(updated == 1)
}

pub fn renew_operation_lease(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
) -> QueryResult<bool> {
    // Lease renewals run while external work is in progress, so each update
    // gets its own short transaction and never extends the surrounding operation.
    with_short_transaction(connection, |connection| {
        let lease_expires_at = Timestamp::after_seconds(300);
        let now = Timestamp::now();
        let updated = diesel::update(
            operation_leases::table
                .filter(operation_leases::id.eq(lease_id))
                .filter(operation_leases::lease_expires_at.gt(&now)),
        )
        .set(operation_leases::lease_expires_at.eq(&lease_expires_at))
        .execute(connection)?;

        Ok(updated == 1)
    })
}

pub fn insert_event(connection: &mut SqliteConnection, value: &NewEvent) -> QueryResult<EventRow> {
    diesel::insert_into(lifecycle_events::table)
        .values(value)
        .execute(connection)?;
    lifecycle_events::table
        .find(&value.event_id)
        .select(EventRow::as_select())
        .first(connection)
}

pub fn append_event(
    connection: &mut SqliteConnection,
    draft: &EventDraft,
) -> QueryResult<EventRow> {
    insert_event(
        connection,
        &NewEvent {
            event_id: crate::domain::EventId::new(),
            operation_id: draft.operation_id,
            entity_type: draft.entity_type.clone(),
            entity_id: draft.entity_id.clone(),
            event_type: draft.event_type.clone(),
            source: draft.source.clone(),
            occurred_at: draft.occurred_at.clone(),
            previous_state: draft.previous_state.clone(),
            current_state: draft.current_state.clone(),
            details_json: draft.details_json.clone(),
            error_json: draft.error_json.clone(),
        },
    )
}

fn append_operation_event(
    connection: &mut SqliteConnection,
    operation_id: OperationId,
    previous_state: Option<OperationState>,
    current_state: OperationState,
    metadata: TransitionMetadata,
) -> QueryResult<EventRow> {
    let details_json = JsonDocument::from_serializable(&serde_json::json!({
        "pending_step": metadata.pending_step,
        "details": metadata.details_json,
    }))
    .map_err(|error| Error::SerializationError(Box::new(error)))?;
    append_event(
        connection,
        &EventDraft {
            operation_id,
            entity_type: "operation".to_owned(),
            entity_id: operation_id.to_string(),
            event_type: metadata.event_type,
            source: metadata.source,
            occurred_at: Timestamp::now(),
            previous_state: previous_state.map(|state| state.to_string()),
            current_state: Some(current_state.to_string()),
            details_json: Some(details_json),
            error_json: metadata.error_json,
        },
    )
}

pub fn record_workspace_transition(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    operation_id: &OperationId,
    state: WorkspaceState,
    metadata: TransitionMetadata,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        let previous_state = workspaces::table
            .find(workspace_id)
            .select(workspaces::state)
            .first::<WorkspaceState>(connection)?;
        let occurred_at = Timestamp::now();

        diesel::update(workspaces::table.find(workspace_id))
            .set((
                workspaces::state.eq(state),
                workspaces::updated_at.eq(&occurred_at),
                workspaces::last_reconciled_at.eq(&occurred_at),
            ))
            .execute(connection)?;
        append_event(
            connection,
            &EventDraft {
                operation_id: *operation_id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace_id.to_string(),
                event_type: metadata.event_type.clone(),
                source: metadata.source,
                occurred_at,
                previous_state: Some(previous_state.to_string()),
                current_state: Some(state.to_string()),
                details_json: metadata.details_json,
                error_json: metadata.error_json,
            },
        )?;
        Ok(())
    })
}

pub fn record_repo_worktree_transition(
    connection: &mut SqliteConnection,
    worktree_id: &RepoWorktreeId,
    operation_id: &OperationId,
    state: RepoWorktreeState,
    last_head: Option<String>,
    metadata: TransitionMetadata,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        let previous_state = repo_worktrees::table
            .find(worktree_id)
            .select(repo_worktrees::state)
            .first::<RepoWorktreeState>(connection)?;
        let occurred_at = Timestamp::now();

        diesel::update(repo_worktrees::table.find(worktree_id))
            .set((
                repo_worktrees::state.eq(state),
                repo_worktrees::last_head.eq(last_head),
                repo_worktrees::last_observed_at.eq(&occurred_at),
            ))
            .execute(connection)?;
        append_event(
            connection,
            &EventDraft {
                operation_id: *operation_id,
                entity_type: "repo_worktree".to_owned(),
                entity_id: worktree_id.to_string(),
                event_type: metadata.event_type.clone(),
                source: metadata.source,
                occurred_at,
                previous_state: Some(previous_state.to_string()),
                current_state: Some(state.to_string()),
                details_json: metadata.details_json,
                error_json: metadata.error_json,
            },
        )?;
        Ok(())
    })
}

pub fn record_operation_transition(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    state: OperationState,
    metadata: TransitionMetadata,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        finish_operation_in_transaction(connection, lease_id, state, metadata)
    })
}

fn finish_operation_in_transaction(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    state: OperationState,
    metadata: TransitionMetadata,
) -> QueryResult<()> {
    let operation_id = operation_id_for_lease(connection, lease_id)?;
    let previous_state =
        operation_state(connection, &operation_id)?.unwrap_or(OperationState::Running);
    let updated = diesel::delete(operation_leases::table.filter(operation_leases::id.eq(lease_id)))
        .execute(connection)?;
    if updated != 1 {
        return Err(Error::NotFound);
    }
    append_operation_event(
        connection,
        operation_id,
        Some(previous_state),
        state,
        metadata,
    )?;
    Ok(())
}

pub fn persist_operation_step_intent(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    pending_step: impl Into<String>,
    intent_json: JsonDocument,
) -> QueryResult<()> {
    let pending_step = pending_step.into();
    with_short_transaction(connection, |connection| {
        let operation_id = operation_id_for_lease(connection, lease_id)?;
        let updated =
            diesel::update(operation_leases::table.filter(operation_leases::id.eq(lease_id)))
                .set(operation_leases::lease_expires_at.eq(Timestamp::after_seconds(300)))
                .execute(connection)?;
        if updated != 1 {
            return Err(Error::NotFound);
        }
        let previous_state =
            operation_state(connection, &operation_id)?.unwrap_or(OperationState::Running);
        append_operation_event(
            connection,
            operation_id,
            Some(previous_state),
            OperationState::Running,
            TransitionMetadata::new("operation_step_started", "trees")
                .with_pending_step(pending_step)
                .with_details(intent_json),
        )?;
        Ok(())
    })
}

pub fn record_worktree_step_result(
    connection: &mut SqliteConnection,
    worktree_id: &RepoWorktreeId,
    lease_id: &LeaseId,
    state: RepoWorktreeState,
    last_head: Option<String>,
    metadata: TransitionMetadata,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        let operation_id = operation_id_for_lease(connection, lease_id)?;
        let previous_state = repo_worktrees::table
            .find(worktree_id)
            .select(repo_worktrees::state)
            .first::<RepoWorktreeState>(connection)?;
        let operation_state =
            operation_state(connection, &operation_id)?.unwrap_or(OperationState::Running);
        let lease_expires_at = Timestamp::after_seconds(300);
        let updated =
            diesel::update(operation_leases::table.filter(operation_leases::id.eq(lease_id)))
                .set(operation_leases::lease_expires_at.eq(&lease_expires_at))
                .execute(connection)?;
        if updated != 1 {
            return Err(Error::NotFound);
        }

        let occurred_at = Timestamp::now();
        diesel::update(repo_worktrees::table.find(worktree_id))
            .set((
                repo_worktrees::state.eq(state),
                repo_worktrees::last_head.eq(last_head),
                repo_worktrees::last_observed_at.eq(&occurred_at),
            ))
            .execute(connection)?;
        append_event(
            connection,
            &EventDraft {
                operation_id,
                entity_type: "repo_worktree".to_owned(),
                entity_id: worktree_id.to_string(),
                event_type: metadata.event_type.clone(),
                source: metadata.source.clone(),
                occurred_at: occurred_at.clone(),
                previous_state: Some(previous_state.to_string()),
                current_state: Some(state.to_string()),
                details_json: metadata.details_json.clone(),
                error_json: metadata.error_json.clone(),
            },
        )?;
        let operation_metadata = TransitionMetadata {
            event_type: format!("operation_step_{}", metadata.event_type),
            source: metadata.source.clone(),
            pending_step: metadata.pending_step.clone(),
            details_json: metadata.details_json.clone(),
            error_json: metadata.error_json.clone(),
        };
        append_operation_event(
            connection,
            operation_id,
            Some(operation_state),
            OperationState::Running,
            operation_metadata,
        )?;
        Ok(())
    })
}

pub fn finalize_creation(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    lease_id: &LeaseId,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        let operation_id = operation_id_for_lease(connection, lease_id)?;
        let workspace = workspaces::table
            .find(workspace_id)
            .select(WorkspaceRow::as_select())
            .first(connection)?;
        let occurred_at = Timestamp::now();

        finish_operation_in_transaction(
            connection,
            lease_id,
            OperationState::Succeeded,
            TransitionMetadata::new("operation_succeeded", "trees").with_pending_step("complete"),
        )?;
        diesel::update(workspaces::table.find(workspace_id))
            .set((
                workspaces::state.eq(WorkspaceState::Ready),
                workspaces::updated_at.eq(&occurred_at),
                workspaces::last_reconciled_at.eq(&occurred_at),
            ))
            .execute(connection)?;
        append_event(
            connection,
            &EventDraft {
                operation_id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace.id.to_string(),
                event_type: "workspace_ready".to_owned(),
                source: "trees".to_owned(),
                occurred_at,
                previous_state: Some(workspace.state.to_string()),
                current_state: Some(WorkspaceState::Ready.to_string()),
                details_json: None,
                error_json: None,
            },
        )?;
        Ok(())
    })
}

pub fn finalize_automatic_creation(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    lease_id: &LeaseId,
    claim: &WorkspaceClaim,
    details_json: Option<JsonDocument>,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        let operation_id = operation_id_for_lease(connection, lease_id)?;
        let workspace = workspaces::table
            .find(workspace_id)
            .select(WorkspaceRow::as_select())
            .first(connection)?;
        let active_claim = workspace_claims::table
            .filter(workspace_claims::workspace_id.eq(workspace_id))
            .select(WorkspaceClaimRow::as_select())
            .first(connection)?;
        if active_claim.id != claim.id {
            return Err(Error::NotFound);
        }

        let occurred_at = Timestamp::now();
        finish_operation_in_transaction(
            connection,
            lease_id,
            OperationState::Succeeded,
            TransitionMetadata::new("operation_succeeded", "trees")
                .with_pending_step("complete")
                .with_details(details_json.clone().unwrap_or_else(empty_json)),
        )?;
        diesel::update(workspaces::table.find(workspace_id))
            .set((
                workspaces::state.eq(WorkspaceState::Ready),
                workspaces::updated_at.eq(&occurred_at),
                workspaces::last_reconciled_at.eq(&occurred_at),
            ))
            .execute(connection)?;
        append_event(
            connection,
            &EventDraft {
                operation_id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace.id.to_string(),
                event_type: "workspace_ready".to_owned(),
                source: "trees".to_owned(),
                occurred_at: occurred_at.clone(),
                previous_state: Some(workspace.state.to_string()),
                current_state: Some(WorkspaceState::Ready.to_string()),
                details_json: None,
                error_json: None,
            },
        )?;
        let workspace = workspaces::table
            .find(workspace_id)
            .select(WorkspaceRow::as_select())
            .first(connection)?;
        append_event(
            connection,
            &workspace_access_event(&operation_id, &workspace, "workspace_claimed", details_json),
        )?;
        Ok(())
    })
}

pub fn list_events_for_operation(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
) -> QueryResult<Vec<EventRow>> {
    lifecycle_events::table
        .filter(lifecycle_events::operation_id.eq(operation_id))
        .order((
            lifecycle_events::occurred_at.asc(),
            lifecycle_events::event_id.asc(),
        ))
        .select(EventRow::as_select())
        .load(connection)
}

pub fn update_workspace_observation(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    state: crate::domain::WorkspaceState,
    updated_at: &Timestamp,
    last_reconciled_at: &Timestamp,
) -> QueryResult<usize> {
    diesel::update(workspaces::table.find(workspace_id))
        .set((
            workspaces::state.eq(state),
            workspaces::updated_at.eq(updated_at),
            workspaces::last_reconciled_at.eq(last_reconciled_at),
        ))
        .execute(connection)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::claim::WorkspaceClaim;
    use crate::database;
    use crate::domain::{
        CanonicalPath, ClaimId, EventId, JsonDocument, OperationState, PoolId, RepoWorktreeId,
        RepoWorktreeState, Timestamp, WorkspaceId, WorkspaceState,
    };
    use crate::pool::RepositorySetKey;

    fn origin_id(connection: &mut SqliteConnection, path: &CanonicalPath) -> OriginRepositoryId {
        ensure_origin_repository(connection, path, path)
            .expect("origin repository should be available")
            .id
    }

    #[test]
    fn repositories_round_trip_typed_rows() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let workspace_id = WorkspaceId::new();
        let now = Timestamp::now();
        let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");

        let workspace = insert_workspace(
            &mut connection,
            &NewWorkspace {
                id: workspace_id,
                canonical_path: workspace_path.clone(),
                state: WorkspaceState::Creating,
                created_at: now.clone(),
                updated_at: now.clone(),
                last_reconciled_at: None,
            },
        )
        .expect("workspace should be inserted");
        assert_eq!(workspace.id, workspace_id);
        assert_eq!(
            find_workspace_by_path(&mut connection, &workspace_path)
                .unwrap()
                .unwrap()
                .id,
            workspace_id
        );

        let origin_repository_id = origin_id(&mut connection, &workspace_path);
        let worktree = insert_repo_worktree(
            &mut connection,
            &NewRepoWorktree {
                id: RepoWorktreeId::new(),
                workspace_id,
                origin_repository_id,
                worktree_path: CanonicalPath::resolve("/tmp")
                    .expect("temporary path should resolve"),
                state: RepoWorktreeState::Pending,
                last_head: None,
                last_observed_at: now.clone(),
            },
        )
        .expect("repo worktree should be inserted");
        assert_eq!(
            list_repo_worktrees(&mut connection, &workspace_id)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(worktree.workspace_id, workspace_id);

        let operation = persist_operation_intent(
            &mut connection,
            &OperationIntent::new(
                workspace_id,
                "create",
                now.clone(),
                "attach",
                JsonDocument::parse(r#"{"workspace":"test"}"#).unwrap(),
            ),
        )
        .expect("operation should be inserted");
        let operation_id = operation.id;
        assert_eq!(
            find_running_operation(&mut connection, &workspace_id)
                .unwrap()
                .unwrap()
                .operation
                .id,
            operation_id
        );

        let event = insert_event(
            &mut connection,
            &NewEvent {
                event_id: EventId::new(),
                operation_id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace_id.to_string(),
                event_type: "created".to_owned(),
                source: "trees".to_owned(),
                occurred_at: now,
                previous_state: None,
                current_state: Some("creating".to_owned()),
                details_json: None,
                error_json: None,
            },
        )
        .expect("event should be inserted");
        assert_eq!(
            list_events_for_operation(&mut connection, &operation.id)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(event.operation_id, operation_id);

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn pool_lookup_verifies_repository_ids_without_a_unique_payload_constraint() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let first_id = OriginRepositoryId::new();
        let second_id = OriginRepositoryId::new();
        let first = RepositorySetKey::from_repository_ids(&[first_id]);
        let second = RepositorySetKey::from_repository_ids(&[second_id]);
        let first_pool = insert_workspace_pool(
            &mut connection,
            &NewWorkspacePool {
                id: PoolId::new(),
                hash_key: first.hash_key().to_owned(),
                repository_ids: first.repository_ids().to_owned(),
            },
        )
        .expect("first pool should be inserted");
        let second_pool = insert_workspace_pool(
            &mut connection,
            &NewWorkspacePool {
                id: PoolId::new(),
                hash_key: first.hash_key().to_owned(),
                repository_ids: second.repository_ids().to_owned(),
            },
        )
        .expect("colliding pool should be inserted");

        assert_eq!(
            find_workspace_pool(&mut connection, &first)
                .expect("first pool lookup should succeed")
                .expect("first pool should exist")
                .id,
            first_pool.id
        );
        assert_eq!(
            workspace_pools::table
                .filter(workspace_pools::hash_key.eq(first.hash_key()))
                .filter(workspace_pools::repository_ids.eq(second.repository_ids()))
                .select(WorkspacePoolRow::as_select())
                .first::<WorkspacePoolRow>(&mut connection)
                .expect("second pool lookup should succeed")
                .id,
            second_pool.id
        );
        let duplicate_payload_pool = insert_workspace_pool(
            &mut connection,
            &NewWorkspacePool {
                id: PoolId::new(),
                hash_key: first.hash_key().to_owned(),
                repository_ids: first.repository_ids().to_owned(),
            },
        )
        .expect("duplicate payload should not be rejected by the schema");
        assert_ne!(duplicate_payload_pool.id, first_pool.id);
        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn pool_lookup_is_independent_of_workspace_root() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let repository_set = RepositorySetKey::from_repository_ids(&[OriginRepositoryId::new()]);

        let first_pool = ensure_workspace_pool(&mut connection, &repository_set)
            .expect("first workspace pool should be created");
        let second_pool = ensure_workspace_pool(&mut connection, &repository_set)
            .expect("second workspace pool should be created");

        assert_eq!(first_pool.id, second_pool.id);
        assert_eq!(
            find_workspace_pool(&mut connection, &repository_set)
                .expect("first pool lookup should succeed")
                .expect("first pool should exist")
                .id,
            first_pool.id
        );
        assert_eq!(
            find_workspace_pool(&mut connection, &repository_set)
                .expect("second pool lookup should succeed")
                .expect("second pool should exist")
                .id,
            second_pool.id
        );

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn workspace_claims_round_trip_and_release() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let workspace_id = WorkspaceId::new();
        let now = Timestamp::now();
        let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");

        insert_workspace(
            &mut connection,
            &NewWorkspace {
                id: workspace_id,
                canonical_path: workspace_path,
                state: WorkspaceState::Ready,
                created_at: now.clone(),
                updated_at: now,
                last_reconciled_at: None,
            },
        )
        .expect("workspace should be inserted");

        let claim = WorkspaceClaim::new(workspace_id);
        let row = insert_workspace_claim(&mut connection, &NewWorkspaceClaim::from(&claim))
            .expect("claim should be inserted");
        assert_eq!(row.id, claim.id);
        assert!(matches!(
            insert_workspace_claim(
                &mut connection,
                &NewWorkspaceClaim::from(&WorkspaceClaim::new(workspace_id)),
            ),
            Err(Error::DatabaseError(DatabaseErrorKind::UniqueViolation, _))
        ));
        assert_eq!(
            find_workspace_claim(&mut connection, &workspace_id)
                .unwrap()
                .unwrap()
                .id,
            claim.id
        );
        assert_eq!(
            find_workspace_claim_by_id(&mut connection, &claim.id)
                .unwrap()
                .workspace_id,
            workspace_id
        );

        let wrong_claim_id = ClaimId::new();
        assert!(
            !release_workspace_claim(&mut connection, &workspace_id, &wrong_claim_id,)
                .expect("wrong claim ID should not release a claim")
        );
        assert!(find_workspace_claim(&mut connection, &workspace_id)
            .unwrap()
            .is_some());

        assert!(
            release_workspace_claim(&mut connection, &workspace_id, &claim.id,)
                .expect("claim should release")
        );
        assert!(find_workspace_claim(&mut connection, &workspace_id)
            .unwrap()
            .is_none());

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn acquire_and_release_commit_claim_state_and_events_atomically() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let workspace_id = WorkspaceId::new();
        let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
        let now = Timestamp::now();

        insert_workspace(
            &mut connection,
            &NewWorkspace {
                id: workspace_id,
                canonical_path: workspace_path,
                state: WorkspaceState::Ready,
                created_at: now.clone(),
                updated_at: now.clone(),
                last_reconciled_at: None,
            },
        )
        .expect("workspace should be inserted");

        let acquire_intent = OperationIntent::new(
            workspace_id,
            "acquire",
            Timestamp::after_seconds(300),
            "acquire claim",
            JsonDocument::parse(r#"{"kind":"acquire"}"#).unwrap(),
        );
        let acquire_operation = begin_operation(&mut connection, &acquire_intent)
            .expect("acquire operation should start");
        let claim = WorkspaceClaim::new(workspace_id);
        record_workspace_acquire(
            &mut connection,
            &acquire_intent.lease_id,
            &claim,
            Some(JsonDocument::parse(r#"{"acquire":true}"#).unwrap()),
        )
        .expect("acquire should be recorded");
        assert!(find_workspace_claim(&mut connection, &workspace_id)
            .unwrap()
            .is_some());
        assert_eq!(
            operation_state(&mut connection, &acquire_operation.id).unwrap(),
            Some(OperationState::Running)
        );
        assert_eq!(
            list_events_for_operation(&mut connection, &acquire_operation.id)
                .unwrap()
                .len(),
            2
        );
        record_operation_transition(
            &mut connection,
            &acquire_intent.lease_id,
            OperationState::Succeeded,
            TransitionMetadata::new("operation_succeeded", "trees")
                .with_pending_step("acquire complete"),
        )
        .expect("acquire operation should complete");

        let release_intent = OperationIntent::new(
            workspace_id,
            "release",
            Timestamp::after_seconds(300),
            "release claim",
            JsonDocument::parse(r#"{"kind":"release"}"#).unwrap(),
        );
        let release_operation = begin_operation(&mut connection, &release_intent)
            .expect("release operation should start");
        record_workspace_release(
            &mut connection,
            &release_intent.lease_id,
            &workspace_id,
            &claim.id,
            None,
        )
        .expect("release should be recorded");
        assert!(find_workspace_claim(&mut connection, &workspace_id)
            .unwrap()
            .is_none());
        assert!(find_workspace(&mut connection, &workspace_id)
            .unwrap()
            .last_released_at
            .is_some());
        assert_eq!(
            operation_state(&mut connection, &release_operation.id).unwrap(),
            Some(OperationState::Succeeded)
        );

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn workspace_reclamation_keeps_tombstones_and_prior_events() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let workspace_id = WorkspaceId::new();
        let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
        let worktree_path = CanonicalPath::resolve("/tmp").expect("worktree path should resolve");
        let now = Timestamp::now();

        insert_workspace(
            &mut connection,
            &NewWorkspace {
                id: workspace_id,
                canonical_path: workspace_path.clone(),
                state: WorkspaceState::Ready,
                created_at: now.clone(),
                updated_at: now.clone(),
                last_reconciled_at: None,
            },
        )
        .expect("workspace should be inserted");
        let origin_repository_id = origin_id(&mut connection, &workspace_path);
        let worktree = insert_repo_worktree(
            &mut connection,
            &NewRepoWorktree {
                id: RepoWorktreeId::new(),
                workspace_id,
                origin_repository_id,
                worktree_path,
                state: RepoWorktreeState::Attached,
                last_head: Some("abc123".to_owned()),
                last_observed_at: now.clone(),
            },
        )
        .expect("worktree should be inserted");
        let intent = OperationIntent::new(
            workspace_id,
            "gc",
            Timestamp::after_seconds(300),
            "reclaim workspace",
            JsonDocument::parse(r#"{"kind":"gc"}"#).unwrap(),
        );
        let operation =
            begin_operation(&mut connection, &intent).expect("GC operation should start");
        append_event(
            &mut connection,
            &EventDraft {
                operation_id: operation.id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace_id.to_string(),
                event_type: "workspace_released".to_owned(),
                source: "trees".to_owned(),
                occurred_at: now,
                previous_state: Some(WorkspaceState::Ready.to_string()),
                current_state: Some(WorkspaceState::Ready.to_string()),
                details_json: None,
                error_json: None,
            },
        )
        .expect("prior event should be inserted");

        record_workspace_reclaimed(
            &mut connection,
            &intent.lease_id,
            &workspace_id,
            Some(JsonDocument::parse(r#"{"forced":false}"#).unwrap()),
        )
        .expect("workspace should be reclaimed");

        let workspace = find_workspace(&mut connection, &workspace_id).unwrap();
        assert_eq!(workspace.state, WorkspaceState::Reclaimed);
        assert!(workspace.reclaimed_at.is_some());
        assert_eq!(
            list_repo_worktrees(&mut connection, &workspace_id).unwrap()[0].state,
            RepoWorktreeState::Reclaimed
        );
        let events = list_events_for_operation(&mut connection, &operation.id).unwrap();
        assert!(events
            .iter()
            .any(|event| event.event_type == "workspace_released"));
        assert!(events
            .iter()
            .any(|event| event.event_type == "workspace_reclaimed"));
        assert!(events
            .iter()
            .any(|event| event.entity_id == worktree.id.to_string()));

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn operation_intent_is_persisted_before_any_workflow_step() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let workspace_id = WorkspaceId::new();
        let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
        let now = Timestamp::now();

        insert_workspace(
            &mut connection,
            &NewWorkspace {
                id: workspace_id,
                canonical_path: workspace_path,
                state: WorkspaceState::Creating,
                created_at: now.clone(),
                updated_at: now,
                last_reconciled_at: None,
            },
        )
        .expect("workspace should be inserted");

        let intent = OperationIntent::new(
            workspace_id,
            "create",
            Timestamp::now(),
            "attach repo",
            JsonDocument::parse(r#"{"target":"repo"}"#).unwrap(),
        );
        let operation = persist_operation_intent(&mut connection, &intent)
            .expect("operation intent should be persisted");

        assert_eq!(operation.id, intent.id);
        assert_eq!(
            operation_state(&mut connection, &operation.id).unwrap(),
            Some(OperationState::Running)
        );
        assert_eq!(
            find_operation_lease(&mut connection, &operation.id)
                .unwrap()
                .unwrap()
                .id,
            intent.lease_id
        );

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn running_operations_are_exclusive_per_workspace() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let first_workspace_id = WorkspaceId::new();
        let second_workspace_id = WorkspaceId::new();
        let now = Timestamp::now();
        let workspace_paths = [
            (
                first_workspace_id,
                CanonicalPath::resolve(".").expect("workspace path should resolve"),
            ),
            (
                second_workspace_id,
                CanonicalPath::resolve("/tmp").expect("temporary path should resolve"),
            ),
        ];

        for (workspace_id, workspace_path) in workspace_paths {
            insert_workspace(
                &mut connection,
                &NewWorkspace {
                    id: workspace_id,
                    canonical_path: workspace_path,
                    state: WorkspaceState::Creating,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    last_reconciled_at: None,
                },
            )
            .expect("workspace should be inserted");
        }

        let intent = |workspace_id| {
            OperationIntent::new(
                workspace_id,
                "create",
                Timestamp::now(),
                "attach repo",
                JsonDocument::parse(r#"{"target":"repo"}"#).unwrap(),
            )
        };

        begin_operation(&mut connection, &intent(first_workspace_id))
            .expect("first operation should start");
        assert!(matches!(
            begin_operation(&mut connection, &intent(first_workspace_id)),
            Err(OperationIntentError::WorkspaceBusy(id)) if id == first_workspace_id
        ));
        begin_operation(&mut connection, &intent(second_workspace_id))
            .expect("different workspace should start concurrently");

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn expired_operation_can_be_claimed_only_once() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let workspace_id = WorkspaceId::new();
        let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
        let now = Timestamp::now();
        insert_workspace(
            &mut connection,
            &NewWorkspace {
                id: workspace_id,
                canonical_path: workspace_path,
                state: WorkspaceState::Creating,
                created_at: now.clone(),
                updated_at: now.clone(),
                last_reconciled_at: None,
            },
        )
        .expect("workspace should be inserted");
        let intent = OperationIntent::new(
            workspace_id,
            "create",
            now.clone(),
            "attach",
            JsonDocument::parse(r#"{"target":"repo"}"#).unwrap(),
        );
        let operation = persist_operation_intent(&mut connection, &intent)
            .expect("operation should be inserted");
        let operation_id = operation.id;

        let new_lease = Timestamp::after_seconds(300);
        let new_lease_id = LeaseId::new();
        assert!(claim_expired_operation(
            &mut connection,
            &intent.lease_id,
            &now,
            &new_lease_id,
            &new_lease,
        )
        .expect("expired operation should be claimable"));
        assert!(!claim_expired_operation(
            &mut connection,
            &intent.lease_id,
            &now,
            &LeaseId::new(),
            &Timestamp::after_seconds(300),
        )
        .expect("the same expired operation should not be claimable twice"));

        let lease = find_operation_lease(&mut connection, &operation_id)
            .expect("claimed operation lease should be readable")
            .expect("claimed operation lease should exist");
        assert_eq!(lease.id, new_lease_id);
        assert_eq!(lease.lease_expires_at, new_lease);

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn active_operation_lease_renews_only_for_its_owner() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let workspace_id = WorkspaceId::new();
        let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
        let now = Timestamp::now();
        insert_workspace(
            &mut connection,
            &NewWorkspace {
                id: workspace_id,
                canonical_path: workspace_path,
                state: WorkspaceState::Creating,
                created_at: now.clone(),
                updated_at: now.clone(),
                last_reconciled_at: None,
            },
        )
        .expect("workspace should be inserted");
        let lease_expires_at = Timestamp::after_seconds(300);
        let intent = OperationIntent::new(
            workspace_id,
            "create",
            lease_expires_at.clone(),
            "attach",
            JsonDocument::parse(r#"{"target":"repo"}"#).unwrap(),
        );
        let operation = persist_operation_intent(&mut connection, &intent)
            .expect("operation should be inserted");
        let operation_id = operation.id;

        assert!(renew_operation_lease(&mut connection, &intent.lease_id)
            .expect("owner should renew the lease"));
        assert!(!renew_operation_lease(&mut connection, &LeaseId::new())
            .expect("a different owner should not renew the lease"));
        let lease = find_operation_lease(&mut connection, &operation_id)
            .expect("renewed operation lease should be readable")
            .expect("renewed operation lease should exist");
        assert!(lease.lease_expires_at >= lease_expires_at);

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn transitions_commit_snapshots_and_events_together() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let workspace_id = WorkspaceId::new();
        let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
        let now = Timestamp::now();

        insert_workspace(
            &mut connection,
            &NewWorkspace {
                id: workspace_id,
                canonical_path: workspace_path.clone(),
                state: WorkspaceState::Creating,
                created_at: now.clone(),
                updated_at: now.clone(),
                last_reconciled_at: None,
            },
        )
        .expect("workspace should be inserted");
        let operation = begin_operation(
            &mut connection,
            &OperationIntent::new(
                workspace_id,
                "create",
                Timestamp::now(),
                "attach repo",
                JsonDocument::parse(r#"{"target":"repo"}"#).unwrap(),
            ),
        )
        .expect("operation should start");
        let lease_id = find_operation_lease(&mut connection, &operation.id)
            .unwrap()
            .unwrap()
            .id;
        let origin_repository_id = origin_id(&mut connection, &workspace_path);
        let worktree = insert_repo_worktree(
            &mut connection,
            &NewRepoWorktree {
                id: RepoWorktreeId::new(),
                workspace_id,
                origin_repository_id,
                worktree_path: CanonicalPath::resolve("/tmp")
                    .expect("temporary path should resolve"),
                state: RepoWorktreeState::Pending,
                last_head: None,
                last_observed_at: now,
            },
        )
        .expect("worktree should be inserted");

        record_workspace_transition(
            &mut connection,
            &workspace_id,
            &operation.id,
            WorkspaceState::Ready,
            TransitionMetadata::new("workspace_ready", "trees")
                .with_details(JsonDocument::parse(r#"{"repositories":1}"#).unwrap()),
        )
        .expect("workspace transition should be recorded");
        record_repo_worktree_transition(
            &mut connection,
            &worktree.id,
            &operation.id,
            RepoWorktreeState::Attached,
            Some("abc123".to_owned()),
            TransitionMetadata::new("worktree_attached", "trees"),
        )
        .expect("worktree transition should be recorded");
        record_operation_transition(
            &mut connection,
            &lease_id,
            OperationState::Succeeded,
            TransitionMetadata::new("operation_succeeded", "trees").with_pending_step("complete"),
        )
        .expect("operation transition should be recorded");

        let workspace = find_workspace_by_path(
            &mut connection,
            &CanonicalPath::resolve(".").expect("workspace path should resolve"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(workspace.state, WorkspaceState::Ready);
        assert_eq!(
            list_repo_worktrees(&mut connection, &workspace_id).unwrap()[0].state,
            RepoWorktreeState::Attached
        );
        assert_eq!(
            operation_state(&mut connection, &operation.id).unwrap(),
            Some(OperationState::Succeeded),
        );
        assert_eq!(
            list_events_for_operation(&mut connection, &operation.id)
                .unwrap()
                .len(),
            4
        );
        assert!(diesel::update(operations::table.find(&operation.id))
            .set(operations::kind.eq("mutated"))
            .execute(&mut connection)
            .is_err());
        assert!(diesel::delete(operations::table.find(&operation.id))
            .execute(&mut connection)
            .is_err());
        assert!(find_operation_lease(&mut connection, &operation.id)
            .unwrap()
            .is_none());

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }
}
