use diesel::backend::Backend;
use diesel::deserialize::{self, FromSql};
use diesel::prelude::*;
use diesel::serialize::{self, IsNull, Output, ToSql};
use diesel::sql_types::Text;
use diesel::sqlite::Sqlite;

use crate::claim::WorkspaceClaim;
use crate::domain::{
    CanonicalPath, ClaimId, EventId, JsonDocument, LeaseId, OperationId, OperationState,
    OriginRepositoryId, PoolId, RepoWorktreeId, RepoWorktreeState, RepositoryManagementMode,
    Timestamp, WorkspaceId, WorkspaceManagementMode, WorkspaceState,
};
use crate::schema::{
    lifecycle_events, operation_leases, operations, origin_repositories, repo_worktrees,
    workspace_claims, workspace_pool_repositories, workspace_pools, workspaces,
};

macro_rules! impl_text_codec {
    ($type:ty) => {
        impl ToSql<Text, Sqlite> for $type {
            fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Sqlite>) -> serialize::Result {
                out.set_value(self.to_string());
                Ok(IsNull::No)
            }
        }

        impl FromSql<Text, Sqlite> for $type {
            fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> deserialize::Result<Self> {
                let value = <String as FromSql<Text, Sqlite>>::from_sql(bytes)?;
                value
                    .parse()
                    .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
            }
        }
    };
}

impl_text_codec!(WorkspaceId);
impl_text_codec!(PoolId);
impl_text_codec!(OriginRepositoryId);
impl_text_codec!(RepoWorktreeId);
impl_text_codec!(OperationId);
impl_text_codec!(LeaseId);
impl_text_codec!(EventId);
impl_text_codec!(ClaimId);
impl_text_codec!(WorkspaceState);
impl_text_codec!(WorkspaceManagementMode);
impl_text_codec!(RepositoryManagementMode);
impl_text_codec!(RepoWorktreeState);
impl_text_codec!(OperationState);
impl_text_codec!(CanonicalPath);
impl_text_codec!(JsonDocument);
impl_text_codec!(Timestamp);

/// A workspace snapshot; automatic root placement is derived from its path.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = workspaces)]
#[diesel(check_for_backend(Sqlite))]
pub struct WorkspaceRow {
    pub id: WorkspaceId,
    pub canonical_path: CanonicalPath,
    pub state: WorkspaceState,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub last_reconciled_at: Option<Timestamp>,
    pub management_mode: WorkspaceManagementMode,
    pub pool_id: Option<PoolId>,
    pub last_released_at: Option<Timestamp>,
    pub reclaimed_at: Option<Timestamp>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = workspaces)]
pub struct NewWorkspace {
    pub id: WorkspaceId,
    pub canonical_path: CanonicalPath,
    pub state: WorkspaceState,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub last_reconciled_at: Option<Timestamp>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = workspaces)]
pub struct NewManagedWorkspace {
    pub id: WorkspaceId,
    pub canonical_path: CanonicalPath,
    pub state: WorkspaceState,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub last_reconciled_at: Option<Timestamp>,
    pub management_mode: WorkspaceManagementMode,
    pub pool_id: Option<PoolId>,
    pub last_released_at: Option<Timestamp>,
    pub reclaimed_at: Option<Timestamp>,
}

/// A pool registry row for one logical repository set.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = workspace_pools)]
#[diesel(check_for_backend(Sqlite))]
pub struct WorkspacePoolRow {
    pub id: PoolId,
    pub hash_key: String,
    pub repository_ids: String,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = workspace_pools)]
pub struct NewWorkspacePool {
    pub id: PoolId,
    pub hash_key: String,
    pub repository_ids: String,
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = workspace_pool_repositories)]
#[diesel(primary_key(pool_id, repository_id))]
#[diesel(check_for_backend(Sqlite))]
/// Explicit pool-to-origin relationship used for pool-scoped repository queries.
pub struct WorkspacePoolRepositoryRow {
    pub pool_id: PoolId,
    pub repository_id: OriginRepositoryId,
}

#[derive(Debug, Clone)]
pub struct PoolOriginRepository {
    pub pool_id: PoolId,
    pub repository: OriginRepositoryRow,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = workspace_pool_repositories)]
/// Inserts one explicit pool-to-origin relationship.
pub struct NewWorkspacePoolRepository {
    pub pool_id: PoolId,
    pub repository_id: OriginRepositoryId,
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = origin_repositories)]
#[diesel(check_for_backend(Sqlite))]
pub struct OriginRepositoryRow {
    pub id: OriginRepositoryId,
    pub repository_identity: CanonicalPath,
    pub source_path: CanonicalPath,
    pub registered: bool,
    pub management_mode: RepositoryManagementMode,
    pub managed_root: Option<CanonicalPath>,
    pub remote_url: Option<String>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = origin_repositories)]
pub struct NewOriginRepository {
    pub id: OriginRepositoryId,
    pub repository_identity: CanonicalPath,
    pub source_path: CanonicalPath,
}

/// Stores the current usage claim for an automatic workspace.
///
/// The row is created by acquisition and removed by successful release. It is
/// not a history record; rejected release leaves it in place for inspection.
#[derive(Debug, Queryable, Selectable, Identifiable)]
#[diesel(table_name = workspace_claims)]
#[diesel(check_for_backend(Sqlite))]
pub struct WorkspaceClaimRow {
    pub id: ClaimId,
    pub workspace_id: WorkspaceId,
    pub claimed_at: Timestamp,
}

#[derive(Debug)]
pub struct WorkspaceOpenSnapshot {
    pub workspace: WorkspaceRow,
    pub claim: Option<WorkspaceClaimRow>,
    pub operation_lease: Option<OperationLeaseRow>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = workspace_claims)]
pub struct NewWorkspaceClaim {
    pub id: ClaimId,
    pub workspace_id: WorkspaceId,
    pub claimed_at: Timestamp,
}

impl From<&WorkspaceClaim> for NewWorkspaceClaim {
    fn from(value: &WorkspaceClaim) -> Self {
        Self {
            id: value.id,
            workspace_id: value.workspace_id,
            claimed_at: value.claimed_at.clone(),
        }
    }
}

impl From<WorkspaceClaimRow> for WorkspaceClaim {
    fn from(value: WorkspaceClaimRow) -> Self {
        Self {
            id: value.id,
            workspace_id: value.workspace_id,
            claimed_at: value.claimed_at,
        }
    }
}

#[derive(Debug, Queryable, Identifiable)]
#[diesel(table_name = repo_worktrees)]
pub struct RepoWorktreeRow {
    pub id: RepoWorktreeId,
    pub workspace_id: WorkspaceId,
    pub origin_repository_id: OriginRepositoryId,
    pub repository_identity: CanonicalPath,
    pub source_path: CanonicalPath,
    pub worktree_path: CanonicalPath,
    pub state: RepoWorktreeState,
    pub last_head: Option<String>,
    pub last_observed_at: Timestamp,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = repo_worktrees)]
pub struct NewRepoWorktree {
    pub id: RepoWorktreeId,
    pub workspace_id: WorkspaceId,
    pub origin_repository_id: OriginRepositoryId,
    pub worktree_path: CanonicalPath,
    pub state: RepoWorktreeState,
    pub last_head: Option<String>,
    pub last_observed_at: Timestamp,
}

/// An immutable operation fact; mutable lease and state data live elsewhere.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = operations)]
#[diesel(check_for_backend(Sqlite))]
pub struct OperationRow {
    pub id: OperationId,
    pub workspace_id: WorkspaceId,
    pub kind: String,
    pub started_at: Timestamp,
    pub intent_json: JsonDocument,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = operations)]
pub struct NewOperation {
    pub id: OperationId,
    pub workspace_id: WorkspaceId,
    pub kind: String,
    pub started_at: Timestamp,
    pub intent_json: JsonDocument,
}

/// The current lease for an in-flight operation.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = operation_leases)]
#[diesel(primary_key(id))]
#[diesel(check_for_backend(Sqlite))]
pub struct OperationLeaseRow {
    pub id: LeaseId,
    pub operation_id: OperationId,
    pub workspace_id: WorkspaceId,
    pub lease_expires_at: Timestamp,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = operation_leases)]
pub struct NewOperationLease {
    pub id: LeaseId,
    pub operation_id: OperationId,
    pub workspace_id: WorkspaceId,
    pub lease_expires_at: Timestamp,
}

/// The operation fact and current lease used by recovery checks.
#[derive(Debug, Clone)]
pub struct RunningOperation {
    pub operation: OperationRow,
    pub lease: OperationLeaseRow,
}

/// An operation fact paired with its retained current lease.
#[derive(Debug, Clone)]
pub struct LeasedOperation {
    pub operation: OperationRow,
    pub lease: OperationLeaseRow,
}

#[derive(Debug, Clone)]
pub struct OperationIntent {
    pub id: OperationId,
    pub workspace_id: WorkspaceId,
    pub kind: String,
    pub lease_id: LeaseId,
    pub lease_expires_at: Timestamp,
    pub pending_step: String,
    pub intent_json: JsonDocument,
    pub started_at: Timestamp,
}

impl OperationIntent {
    pub fn new(
        workspace_id: WorkspaceId,
        kind: impl Into<String>,
        lease_expires_at: Timestamp,
        pending_step: impl Into<String>,
        intent_json: JsonDocument,
    ) -> Self {
        let started_at = Timestamp::now();
        Self {
            id: OperationId::new(),
            workspace_id,
            kind: kind.into(),
            lease_id: LeaseId::new(),
            lease_expires_at,
            pending_step: pending_step.into(),
            intent_json,
            started_at,
        }
    }
}

#[derive(Debug, Queryable, Selectable, Identifiable)]
#[diesel(table_name = lifecycle_events)]
#[diesel(primary_key(event_id))]
#[diesel(check_for_backend(Sqlite))]
pub struct EventRow {
    pub event_id: EventId,
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

#[derive(Debug, Insertable)]
#[diesel(table_name = lifecycle_events)]
pub struct NewEvent {
    pub event_id: EventId,
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

#[derive(Debug, Clone, Queryable, Selectable, Insertable, Identifiable)]
#[diesel(table_name = crate::schema::pending_origin_clones)]
#[diesel(check_for_backend(Sqlite))]
pub struct PendingOriginClone {
    pub id: OriginRepositoryId,
    pub remote_url: String,
    pub managed_root: CanonicalPath,
    pub source_path: CanonicalPath,
    pub ownership_token: String,
}
