use diesel::backend::Backend;
use diesel::deserialize::{self, FromSql};
use diesel::prelude::*;
use diesel::serialize::{self, IsNull, Output, ToSql};
use diesel::sql_types::Text;
use diesel::sqlite::Sqlite;

use crate::domain::{
    CanonicalPath, CheckoutId, EventId, JsonDocument, OperationId, OperationState, RepoWorktreeId,
    RepoWorktreeState, Timestamp, WorkspaceId, WorkspaceManagementMode, WorkspaceState,
};
use crate::lease::WorkspaceLease;
use crate::schema::{lifecycle_events, operations, repo_worktrees, workspace_leases, workspaces};

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
impl_text_codec!(RepoWorktreeId);
impl_text_codec!(OperationId);
impl_text_codec!(EventId);
impl_text_codec!(CheckoutId);
impl_text_codec!(WorkspaceState);
impl_text_codec!(WorkspaceManagementMode);
impl_text_codec!(RepoWorktreeState);
impl_text_codec!(OperationState);
impl_text_codec!(CanonicalPath);
impl_text_codec!(JsonDocument);
impl_text_codec!(Timestamp);

#[derive(Debug, Queryable, Selectable, Identifiable)]
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
    pub pool_key: Option<String>,
    pub workspace_root: Option<CanonicalPath>,
    pub last_checked_in_at: Option<Timestamp>,
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
    pub pool_key: Option<String>,
    pub workspace_root: Option<CanonicalPath>,
    pub last_checked_in_at: Option<Timestamp>,
    pub reclaimed_at: Option<Timestamp>,
}

#[derive(Debug, Queryable, Selectable, Identifiable)]
#[diesel(table_name = workspace_leases)]
#[diesel(check_for_backend(Sqlite))]
pub struct WorkspaceLeaseRow {
    pub id: CheckoutId,
    pub workspace_id: WorkspaceId,
    pub owner_id: String,
    pub checked_out_at: Timestamp,
    pub lease_expires_at: Timestamp,
    pub last_heartbeat_at: Timestamp,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = workspace_leases)]
pub struct NewWorkspaceLease {
    pub id: CheckoutId,
    pub workspace_id: WorkspaceId,
    pub owner_id: String,
    pub checked_out_at: Timestamp,
    pub lease_expires_at: Timestamp,
    pub last_heartbeat_at: Timestamp,
}

impl From<&WorkspaceLease> for NewWorkspaceLease {
    fn from(value: &WorkspaceLease) -> Self {
        Self {
            id: value.id,
            workspace_id: value.workspace_id,
            owner_id: value.owner_id.clone(),
            checked_out_at: value.checked_out_at.clone(),
            lease_expires_at: value.lease_expires_at.clone(),
            last_heartbeat_at: value.last_heartbeat_at.clone(),
        }
    }
}

impl From<WorkspaceLeaseRow> for WorkspaceLease {
    fn from(value: WorkspaceLeaseRow) -> Self {
        Self {
            id: value.id,
            workspace_id: value.workspace_id,
            owner_id: value.owner_id,
            checked_out_at: value.checked_out_at,
            lease_expires_at: value.lease_expires_at,
            last_heartbeat_at: value.last_heartbeat_at,
        }
    }
}

#[derive(Debug, Queryable, Selectable, Identifiable)]
#[diesel(table_name = repo_worktrees)]
#[diesel(check_for_backend(Sqlite))]
pub struct RepoWorktreeRow {
    pub id: RepoWorktreeId,
    pub workspace_id: WorkspaceId,
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
    pub repository_identity: CanonicalPath,
    pub source_path: CanonicalPath,
    pub worktree_path: CanonicalPath,
    pub state: RepoWorktreeState,
    pub last_head: Option<String>,
    pub last_observed_at: Timestamp,
}

#[derive(Debug, Queryable, Selectable, Identifiable)]
#[diesel(table_name = operations)]
#[diesel(check_for_backend(Sqlite))]
pub struct OperationRow {
    pub id: OperationId,
    pub workspace_id: WorkspaceId,
    pub kind: String,
    pub state: OperationState,
    pub owner_id: String,
    pub lease_expires_at: Timestamp,
    pub last_heartbeat_at: Timestamp,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub pending_step: String,
    pub intent_json: JsonDocument,
    pub error_json: Option<JsonDocument>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = operations)]
pub struct NewOperation {
    pub id: OperationId,
    pub workspace_id: WorkspaceId,
    pub kind: String,
    pub state: OperationState,
    pub owner_id: String,
    pub lease_expires_at: Timestamp,
    pub last_heartbeat_at: Timestamp,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub pending_step: String,
    pub intent_json: JsonDocument,
    pub error_json: Option<JsonDocument>,
}

#[derive(Debug, Clone)]
pub struct OperationIntent {
    pub id: OperationId,
    pub workspace_id: WorkspaceId,
    pub kind: String,
    pub owner_id: String,
    pub lease_expires_at: Timestamp,
    pub pending_step: String,
    pub intent_json: JsonDocument,
    pub started_at: Timestamp,
}

impl OperationIntent {
    pub fn new(
        workspace_id: WorkspaceId,
        kind: impl Into<String>,
        owner_id: impl Into<String>,
        lease_expires_at: Timestamp,
        pending_step: impl Into<String>,
        intent_json: JsonDocument,
    ) -> Self {
        let started_at = Timestamp::now();
        Self {
            id: OperationId::new(),
            workspace_id,
            kind: kind.into(),
            owner_id: owner_id.into(),
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
