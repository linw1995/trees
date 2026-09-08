use std::collections::HashMap;

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};
use unicode_width::UnicodeWidthStr;

use crate::domain::{
    CanonicalPath, ClaimId, LeaseId, OperationId, OperationState, OriginRepositoryId, PoolId,
    RepoWorktreeId, RepoWorktreeState, Timestamp, WorkspaceId, WorkspaceManagementMode,
    WorkspaceState,
};
use crate::storage::{
    list_leased_operations, list_operation_events, list_repo_worktrees_for_workspaces,
    list_workspace_claims, list_workspaces, EventRow, LeasedOperation, RepoWorktreeRow,
    WorkspaceClaimRow, WorkspaceRow,
};

pub const STATUS_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct StatusSnapshot {
    pub schema_version: u8,
    pub snapshot_at: Timestamp,
    pub workspaces: Vec<WorkspaceStatus>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct WorkspaceStatus {
    pub workspace_id: WorkspaceId,
    pub path: CanonicalPath,
    pub management_mode: WorkspaceManagementMode,
    pub state: WorkspaceState,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub last_reconciled_at: Option<Timestamp>,
    pub last_released_at: Option<Timestamp>,
    pub reclaimed_at: Option<Timestamp>,
    pub pool_id: Option<PoolId>,
    pub claim: Option<ClaimStatus>,
    pub current_operation: Option<CurrentOperationStatus>,
    pub repo_worktrees: Vec<RepoWorktreeStatus>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct ClaimStatus {
    pub claim_id: ClaimId,
    pub claimed_at: Timestamp,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct CurrentOperationStatus {
    pub operation_id: OperationId,
    pub kind: String,
    pub state: OperationState,
    pub lease_id: LeaseId,
    pub lease_expires_at: Timestamp,
    pub lease_status: LeaseStatus,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseStatus {
    Active,
    Expired,
    Inconsistent,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct RepoWorktreeStatus {
    pub repo_worktree_id: RepoWorktreeId,
    pub origin_repository_id: OriginRepositoryId,
    pub source_path: CanonicalPath,
    pub worktree_path: CanonicalPath,
    pub state: RepoWorktreeState,
    pub last_head: Option<String>,
    pub last_observed_at: Timestamp,
}

impl StatusSnapshot {
    pub fn empty() -> Self {
        Self {
            schema_version: STATUS_SCHEMA_VERSION,
            snapshot_at: Timestamp::now(),
            workspaces: Vec::new(),
        }
    }
}

pub fn load_snapshot(
    connection: &mut SqliteConnection,
    include_reclaimed: bool,
) -> QueryResult<StatusSnapshot> {
    connection.transaction(|connection| {
        let snapshot_at = Timestamp::now();
        let workspaces = list_workspaces(connection, include_reclaimed)?;
        let workspace_ids = workspaces
            .iter()
            .map(|workspace| workspace.id)
            .collect::<Vec<_>>();
        let claims = list_workspace_claims(connection, &workspace_ids)?;
        let operations = list_leased_operations(connection, &workspace_ids)?;
        let operation_ids = operations
            .iter()
            .map(|operation| operation.operation.id)
            .collect::<Vec<_>>();
        let operation_events = list_operation_events(connection, &operation_ids)?;
        let repositories = list_repo_worktrees_for_workspaces(connection, &workspace_ids)?;

        assemble_snapshot(
            snapshot_at.clone(),
            workspaces,
            claims,
            operations,
            operation_events,
            repositories,
        )
    })
}

pub fn render_human(snapshot: &StatusSnapshot) -> String {
    if snapshot.workspaces.is_empty() {
        return "No workspaces.".to_owned();
    }

    let headers = ["STATE", "USAGE", "MODE", "REPOS", "RECONCILED", "PATH"];
    let rows = snapshot
        .workspaces
        .iter()
        .map(|workspace| {
            [
                workspace.state.to_string(),
                if workspace.claim.is_some() {
                    "claimed".to_owned()
                } else {
                    "unclaimed".to_owned()
                },
                mode_symbol(workspace.management_mode).to_owned(),
                repository_summary(&workspace.repo_worktrees),
                workspace.last_reconciled_at.as_ref().map_or_else(
                    || "never".to_owned(),
                    |timestamp| compact_timestamp(timestamp, &snapshot.snapshot_at),
                ),
                workspace.path.to_string(),
            ]
        })
        .collect::<Vec<_>>();
    let widths = std::array::from_fn::<_, 6, _>(|column| {
        rows.iter()
            .map(|row| UnicodeWidthStr::width(row[column].as_str()))
            .max()
            .unwrap_or(0)
            .max(UnicodeWidthStr::width(headers[column]))
    });
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(format_table_row(&headers, &widths));
    for row in &rows {
        lines.push(format_table_row(row, &widths));
    }
    lines.join("\n")
}

fn format_table_row<S: AsRef<str>, const N: usize>(
    columns: &[S; N],
    widths: &[usize; N],
) -> String {
    let mut output = String::new();
    for (index, column) in columns.iter().enumerate() {
        let value = column.as_ref();
        output.push_str(value);
        if index + 1 < columns.len() {
            output.push_str(&" ".repeat(widths[index] - UnicodeWidthStr::width(value) + 2));
        }
    }
    output
}

fn mode_symbol(mode: WorkspaceManagementMode) -> &'static str {
    match mode {
        WorkspaceManagementMode::Automatic => "🤖",
        WorkspaceManagementMode::Manual => "👤",
    }
}

fn repository_summary(repositories: &[RepoWorktreeStatus]) -> String {
    let available = repositories
        .iter()
        .filter(|repository| repository.state == RepoWorktreeState::Attached)
        .count();
    format!("{available}/{}", repositories.len())
}

fn compact_timestamp(timestamp: &Timestamp, snapshot_at: &Timestamp) -> String {
    let timestamp = parse_utc(timestamp);
    let snapshot_at = parse_utc(snapshot_at);
    if timestamp.date() == snapshot_at.date() {
        format!("{:02}:{:02}", timestamp.hour(), timestamp.minute())
    } else if timestamp.year() == snapshot_at.year() {
        format!(
            "{:02}-{:02} {:02}:{:02}",
            u8::from(timestamp.month()),
            timestamp.day(),
            timestamp.hour(),
            timestamp.minute()
        )
    } else {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            timestamp.year(),
            u8::from(timestamp.month()),
            timestamp.day(),
            timestamp.hour(),
            timestamp.minute()
        )
    }
}

fn parse_utc(timestamp: &Timestamp) -> OffsetDateTime {
    OffsetDateTime::parse(timestamp.as_str(), &Rfc3339)
        .expect("validated timestamp should parse")
        .to_offset(UtcOffset::UTC)
}

fn assemble_snapshot(
    snapshot_at: Timestamp,
    workspaces: Vec<WorkspaceRow>,
    claims: Vec<WorkspaceClaimRow>,
    operations: Vec<LeasedOperation>,
    operation_events: Vec<EventRow>,
    repositories: Vec<RepoWorktreeRow>,
) -> QueryResult<StatusSnapshot> {
    let mut claims_by_workspace = claims
        .into_iter()
        .map(|claim| (claim.workspace_id, claim.into()))
        .collect::<HashMap<_, ClaimStatus>>();
    let latest_operation_states = latest_operation_states(operation_events)?;
    let mut operations_by_workspace = operations
        .into_iter()
        .map(|operation| {
            let state = latest_operation_states
                .get(&operation.operation.id)
                .copied()
                .unwrap_or(OperationState::Running);
            let status = CurrentOperationStatus::from_leased(operation, state, &snapshot_at);
            (status.0, status.1)
        })
        .collect::<HashMap<_, _>>();
    let mut repositories_by_workspace = HashMap::<WorkspaceId, Vec<RepoWorktreeStatus>>::new();
    for repository in repositories {
        repositories_by_workspace
            .entry(repository.workspace_id)
            .or_default()
            .push(repository.into());
    }

    let workspaces = workspaces
        .into_iter()
        .map(|workspace| {
            let workspace_id = workspace.id;
            WorkspaceStatus {
                workspace_id,
                path: workspace.canonical_path,
                management_mode: workspace.management_mode,
                state: workspace.state,
                created_at: workspace.created_at,
                updated_at: workspace.updated_at,
                last_reconciled_at: workspace.last_reconciled_at,
                last_released_at: workspace.last_released_at,
                reclaimed_at: workspace.reclaimed_at,
                pool_id: workspace.pool_id,
                claim: claims_by_workspace.remove(&workspace_id),
                current_operation: operations_by_workspace.remove(&workspace_id),
                repo_worktrees: repositories_by_workspace
                    .remove(&workspace_id)
                    .unwrap_or_default(),
            }
        })
        .collect();

    Ok(StatusSnapshot {
        schema_version: STATUS_SCHEMA_VERSION,
        snapshot_at,
        workspaces,
    })
}

fn latest_operation_states(
    events: Vec<EventRow>,
) -> QueryResult<HashMap<OperationId, OperationState>> {
    let mut states = HashMap::new();
    for event in events {
        if states.contains_key(&event.operation_id) {
            continue;
        }
        let Some(state) = event.current_state else {
            continue;
        };
        let state = state
            .parse()
            .map_err(|error| diesel::result::Error::DeserializationError(Box::new(error)))?;
        states.insert(event.operation_id, state);
    }
    Ok(states)
}

impl From<WorkspaceClaimRow> for ClaimStatus {
    fn from(value: WorkspaceClaimRow) -> Self {
        Self {
            claim_id: value.id,
            claimed_at: value.claimed_at,
        }
    }
}

impl CurrentOperationStatus {
    fn from_leased(
        value: LeasedOperation,
        state: OperationState,
        snapshot_at: &Timestamp,
    ) -> (WorkspaceId, Self) {
        let lease_status = if state != OperationState::Running {
            LeaseStatus::Inconsistent
        } else if value.lease.lease_expires_at > *snapshot_at {
            LeaseStatus::Active
        } else {
            LeaseStatus::Expired
        };
        (
            value.lease.workspace_id,
            Self {
                operation_id: value.operation.id,
                kind: value.operation.kind,
                state,
                lease_id: value.lease.id,
                lease_expires_at: value.lease.lease_expires_at,
                lease_status,
            },
        )
    }
}

impl From<RepoWorktreeRow> for RepoWorktreeStatus {
    fn from(value: RepoWorktreeRow) -> Self {
        Self {
            repo_worktree_id: value.id,
            origin_repository_id: value.origin_repository_id,
            source_path: value.source_path,
            worktree_path: value.worktree_path,
            state: value.state,
            last_head: value.last_head,
            last_observed_at: value.last_observed_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::database;
    use crate::domain::JsonDocument;
    use crate::storage::{
        append_event, ensure_origin_repository, insert_managed_workspace, insert_repo_worktree,
        insert_workspace_claim, persist_operation_intent, EventDraft, NewManagedWorkspace,
        NewRepoWorktree, NewWorkspaceClaim, OperationIntent,
    };

    fn temporary_database_path() -> PathBuf {
        std::env::temp_dir().join(format!("trees-status-{}.sqlite", WorkspaceId::new()))
    }

    fn path(value: &str) -> CanonicalPath {
        CanonicalPath::from_absolute(value).expect("test path should be absolute")
    }

    fn insert_workspace(
        connection: &mut SqliteConnection,
        value: &str,
        state: WorkspaceState,
    ) -> WorkspaceId {
        let id = WorkspaceId::new();
        let now = Timestamp::now();
        insert_managed_workspace(
            connection,
            &NewManagedWorkspace {
                id,
                canonical_path: path(value),
                state,
                created_at: now.clone(),
                updated_at: now,
                last_reconciled_at: None,
                management_mode: WorkspaceManagementMode::Manual,
                pool_id: None,
                last_released_at: None,
                reclaimed_at: (state == WorkspaceState::Reclaimed).then(Timestamp::now),
            },
        )
        .expect("workspace should be inserted");
        id
    }

    #[test]
    fn loads_empty_snapshot() {
        let database_path = temporary_database_path();
        let mut connection = database::connect(&database_path).expect("database should open");

        let snapshot = load_snapshot(&mut connection, false).expect("snapshot should load");

        assert_eq!(snapshot.schema_version, STATUS_SCHEMA_VERSION);
        assert!(snapshot.workspaces.is_empty());
        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn loads_ordered_related_rows_and_filters_reclaimed_workspaces() {
        let database_path = temporary_database_path();
        let mut connection = database::connect(&database_path).expect("database should open");
        let second_id = insert_workspace(&mut connection, "/status/zeta", WorkspaceState::Ready);
        let first_id = insert_workspace(&mut connection, "/status/alpha", WorkspaceState::Ready);
        insert_workspace(
            &mut connection,
            "/status/reclaimed",
            WorkspaceState::Reclaimed,
        );
        let claim_id = ClaimId::new();
        insert_workspace_claim(
            &mut connection,
            &NewWorkspaceClaim {
                id: claim_id,
                workspace_id: first_id,
                claimed_at: Timestamp::now(),
            },
        )
        .expect("claim should be inserted");
        let origin = ensure_origin_repository(
            &mut connection,
            &path("/origins/example.git"),
            &path("/origins/example"),
        )
        .expect("origin should be inserted");
        insert_repo_worktree(
            &mut connection,
            &NewRepoWorktree {
                id: RepoWorktreeId::new(),
                workspace_id: first_id,
                origin_repository_id: origin.id,
                worktree_path: path("/status/alpha/example"),
                state: RepoWorktreeState::Attached,
                last_head: Some("0123456789abcdef".to_owned()),
                last_observed_at: Timestamp::now(),
            },
        )
        .expect("repo worktree should be inserted");

        let snapshot = load_snapshot(&mut connection, false).expect("snapshot should load");

        assert_eq!(
            snapshot
                .workspaces
                .iter()
                .map(|workspace| workspace.workspace_id)
                .collect::<Vec<_>>(),
            [first_id, second_id]
        );
        assert_eq!(
            snapshot.workspaces[0].claim.as_ref().unwrap().claim_id,
            claim_id
        );
        assert_eq!(snapshot.workspaces[0].repo_worktrees.len(), 1);
        assert!(snapshot.workspaces[1].claim.is_none());
        assert!(snapshot.workspaces[1].current_operation.is_none());

        let all = load_snapshot(&mut connection, true).expect("complete snapshot should load");
        assert_eq!(all.workspaces.len(), 3);
        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn classifies_current_operation_leases_at_the_snapshot_time() {
        let database_path = temporary_database_path();
        let mut connection = database::connect(&database_path).expect("database should open");
        let active_id = insert_workspace(&mut connection, "/status/active", WorkspaceState::Ready);
        let expired_id =
            insert_workspace(&mut connection, "/status/expired", WorkspaceState::Ready);
        let inconsistent_id = insert_workspace(
            &mut connection,
            "/status/inconsistent",
            WorkspaceState::Ready,
        );

        let active = operation(active_id, Timestamp::parse("9999-01-01T00:00:00Z").unwrap());
        persist_operation_intent(&mut connection, &active).expect("operation should be inserted");
        let expired = operation(
            expired_id,
            Timestamp::parse("2000-01-01T00:00:00Z").unwrap(),
        );
        persist_operation_intent(&mut connection, &expired).expect("operation should be inserted");
        let inconsistent = operation(
            inconsistent_id,
            Timestamp::parse("9999-01-01T00:00:00Z").unwrap(),
        );
        persist_operation_intent(&mut connection, &inconsistent)
            .expect("operation should be inserted");
        append_event(
            &mut connection,
            &EventDraft {
                operation_id: inconsistent.id,
                entity_type: "operation".to_owned(),
                entity_id: inconsistent.id.to_string(),
                event_type: "operation_succeeded".to_owned(),
                source: "test".to_owned(),
                occurred_at: Timestamp::parse("9998-01-01T00:00:00Z").unwrap(),
                previous_state: Some(OperationState::Running.to_string()),
                current_state: Some(OperationState::Succeeded.to_string()),
                details_json: None,
                error_json: None,
            },
        )
        .expect("terminal event should be inserted");

        let snapshot = load_snapshot(&mut connection, false).expect("snapshot should load");
        let statuses = snapshot
            .workspaces
            .iter()
            .map(|workspace| {
                (
                    workspace.path.to_string(),
                    workspace
                        .current_operation
                        .as_ref()
                        .map(|operation| operation.lease_status),
                )
            })
            .collect::<HashMap<_, _>>();

        assert_eq!(statuses["/status/active"], Some(LeaseStatus::Active));
        assert_eq!(statuses["/status/expired"], Some(LeaseStatus::Expired));
        assert_eq!(
            statuses["/status/inconsistent"],
            Some(LeaseStatus::Inconsistent)
        );
        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn renders_empty_and_deterministic_human_output() {
        assert_eq!(render_human(&StatusSnapshot::empty()), "No workspaces.");

        let workspace_id = WorkspaceId::new();
        let snapshot = StatusSnapshot {
            schema_version: STATUS_SCHEMA_VERSION,
            snapshot_at: Timestamp::parse("2026-09-08T12:00:00Z").unwrap(),
            workspaces: vec![WorkspaceStatus {
                workspace_id,
                path: path("/status/example"),
                management_mode: WorkspaceManagementMode::Automatic,
                state: WorkspaceState::Degraded,
                created_at: Timestamp::parse("2026-09-01T00:00:00Z").unwrap(),
                updated_at: Timestamp::parse("2026-09-08T10:00:00Z").unwrap(),
                last_reconciled_at: Some(Timestamp::parse("2026-09-08T10:00:00Z").unwrap()),
                last_released_at: None,
                reclaimed_at: None,
                pool_id: None,
                claim: Some(ClaimStatus {
                    claim_id: ClaimId::new(),
                    claimed_at: Timestamp::parse("2026-09-08T09:00:00Z").unwrap(),
                }),
                current_operation: None,
                repo_worktrees: vec![RepoWorktreeStatus {
                    repo_worktree_id: RepoWorktreeId::new(),
                    origin_repository_id: OriginRepositoryId::new(),
                    source_path: path("/origins/example"),
                    worktree_path: path("/status/example/repo"),
                    state: RepoWorktreeState::Dirty,
                    last_head: None,
                    last_observed_at: Timestamp::parse("2026-09-08T10:00:00Z").unwrap(),
                }],
            }],
        };

        let output = render_human(&snapshot);

        assert_eq!(
            output,
            "STATE     USAGE    MODE  REPOS  RECONCILED  PATH\n\
             degraded  claimed  🤖    0/1    10:00       /status/example"
        );
    }

    #[test]
    fn compacts_reconciliation_times_relative_to_the_snapshot() {
        let snapshot = Timestamp::parse("2026-09-08T12:00:00Z").unwrap();

        assert_eq!(
            compact_timestamp(
                &Timestamp::parse("2026-09-08T09:07:00+00:00").unwrap(),
                &snapshot,
            ),
            "09:07"
        );
        assert_eq!(
            compact_timestamp(
                &Timestamp::parse("2026-08-31T23:59:00+00:00").unwrap(),
                &snapshot,
            ),
            "08-31 23:59"
        );
        assert_eq!(
            compact_timestamp(
                &Timestamp::parse("2025-12-31T23:59:00+00:00").unwrap(),
                &snapshot,
            ),
            "2025-12-31 23:59"
        );
    }

    #[test]
    fn serializes_the_versioned_json_contract() {
        let snapshot = StatusSnapshot::empty();
        let value = serde_json::to_value(snapshot).expect("status snapshot should serialize");

        assert_eq!(value["schema_version"], STATUS_SCHEMA_VERSION);
        assert!(value["snapshot_at"].is_string());
        assert_eq!(value["workspaces"], serde_json::json!([]));
    }

    fn operation(workspace_id: WorkspaceId, lease_expires_at: Timestamp) -> OperationIntent {
        OperationIntent::new(
            workspace_id,
            "test",
            lease_expires_at,
            "test operation",
            JsonDocument::parse("{}").unwrap(),
        )
    }
}
