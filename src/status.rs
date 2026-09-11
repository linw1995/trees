pub mod combined;
pub mod repos;
pub mod target;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};
use unicode_width::UnicodeWidthChar;

use crate::domain::{
    CanonicalPath, ClaimId, LeaseId, OperationId, OperationState, OriginRepositoryId, PoolId,
    RepoWorktreeId, RepoWorktreeState, Timestamp, WorkspaceId, WorkspaceManagementMode,
    WorkspaceState,
};
use crate::storage::{
    list_current_automatic_operation_leases, list_current_automatic_pool_repositories,
    list_current_automatic_workspace_claims, list_current_automatic_workspaces,
    list_status_leased_operations, list_status_operation_events, list_status_repo_worktrees,
    list_status_workspace_claims, list_workspaces, EventRow, LeasedOperation, PoolOriginRepository,
    RepoWorktreeRow, WorkspaceClaimRow, WorkspaceRow,
};

pub const STATUS_SCHEMA_VERSION: u8 = 2;

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct StatusSnapshot {
    pub schema_version: u8,
    pub view: StatusView,
    pub snapshot_at: Timestamp,
    pub target_workspace: Option<WorkspaceStatus>,
    pub workspaces: Vec<WorkspaceStatus>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusView {
    Pools,
    Workspaces,
    Repos,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct PoolStatusSnapshot {
    pub schema_version: u8,
    pub view: StatusView,
    pub snapshot_at: Timestamp,
    pub target_workspace: Option<WorkspaceStatus>,
    pub pools: Vec<PoolStatus>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct PoolStatus {
    pub pool_id: PoolId,
    pub repositories: Vec<PoolRepositoryStatus>,
    pub available: usize,
    pub capacity: usize,
    pub abnormal: usize,
    pub updated_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct PoolRepositoryStatus {
    pub origin_repository_id: OriginRepositoryId,
    pub source_path: CanonicalPath,
    pub label: String,
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
    pub removed_at: Option<Timestamp>,
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
            view: StatusView::Workspaces,
            snapshot_at: Timestamp::now(),
            target_workspace: None,
            workspaces: Vec::new(),
        }
    }
}

impl PoolStatusSnapshot {
    pub fn empty() -> Self {
        Self {
            schema_version: STATUS_SCHEMA_VERSION,
            view: StatusView::Pools,
            snapshot_at: Timestamp::now(),
            target_workspace: None,
            pools: Vec::new(),
        }
    }
}

pub fn load_pool_snapshot(connection: &mut SqliteConnection) -> QueryResult<PoolStatusSnapshot> {
    connection.transaction(|connection| load_pools_in_transaction(connection, Timestamp::now()))
}

fn load_pools_in_transaction(
    connection: &mut SqliteConnection,
    snapshot_at: Timestamp,
) -> QueryResult<PoolStatusSnapshot> {
    let workspaces = list_current_automatic_workspaces(connection)?;
    let claims = list_current_automatic_workspace_claims(connection)?;
    let leases = list_current_automatic_operation_leases(connection)?;
    let repositories = list_current_automatic_pool_repositories(connection)?;
    Ok(assemble_pool_snapshot(
        snapshot_at,
        workspaces,
        claims,
        leases,
        repositories,
    ))
}

pub fn render_pools_human(snapshot: &PoolStatusSnapshot, color: bool) -> String {
    if snapshot.pools.is_empty() {
        return "No workspace pools.".to_owned();
    }
    let headers = ["REPOS", "CAPACITY", "UPDATED"];
    let rows = snapshot
        .pools
        .iter()
        .map(|pool| {
            [
                pool.repositories
                    .iter()
                    .map(|repository| escape_human_label(&repository.label))
                    .collect::<Vec<_>>()
                    .join(","),
                capacity_summary(pool, color),
                pool.updated_at.as_ref().map_or_else(
                    || "never".to_owned(),
                    |timestamp| compact_timestamp(timestamp, &snapshot.snapshot_at),
                ),
            ]
        })
        .collect::<Vec<_>>();
    render_table(&headers, &rows)
}

fn capacity_summary(pool: &PoolStatus, color: bool) -> String {
    if color {
        format!(
            "{}/{}/{}",
            colored_count(pool.available, 32),
            colored_count(pool.capacity, 34),
            colored_count(pool.abnormal, 31)
        )
    } else {
        format!("{}/{}/{}", pool.available, pool.capacity, pool.abnormal)
    }
}

pub fn load_snapshot(
    connection: &mut SqliteConnection,
    include_removed: bool,
) -> QueryResult<StatusSnapshot> {
    connection.transaction(|connection| {
        load_workspaces_in_transaction(connection, include_removed, Timestamp::now())
    })
}

fn load_workspaces_in_transaction(
    connection: &mut SqliteConnection,
    include_removed: bool,
    snapshot_at: Timestamp,
) -> QueryResult<StatusSnapshot> {
    let workspaces = list_workspaces(connection, include_removed)?;
    let claims = list_status_workspace_claims(connection, include_removed)?;
    let operations = list_status_leased_operations(connection, include_removed)?;
    let operation_events = list_status_operation_events(connection, include_removed)?;
    let repositories = list_status_repo_worktrees(connection, include_removed)?;
    assemble_snapshot(
        snapshot_at,
        workspaces,
        claims,
        operations,
        operation_events,
        repositories,
    )
}

pub fn render_workspaces_human(snapshot: &StatusSnapshot, color: bool) -> String {
    if snapshot.workspaces.is_empty() {
        return "No workspaces.".to_owned();
    }

    let headers = ["STATUS", "MODE", "REPOS", "RECONCILED", "ID"];
    let rows = snapshot
        .workspaces
        .iter()
        .map(|workspace| {
            [
                workspace_status_summary(workspace),
                mode_symbol(workspace.management_mode).to_owned(),
                repository_summary(&workspace.repo_worktrees, color),
                workspace.last_reconciled_at.as_ref().map_or_else(
                    || "never".to_owned(),
                    |timestamp| compact_timestamp(timestamp, &snapshot.snapshot_at),
                ),
                workspace.workspace_id.to_string(),
            ]
        })
        .collect::<Vec<_>>();
    render_table(&headers, &rows)
}

fn render_table<const N: usize>(headers: &[&str; N], rows: &[[String; N]]) -> String {
    let widths = std::array::from_fn::<_, N, _>(|column| {
        rows.iter()
            .map(|row| display_width(row[column].as_str()))
            .max()
            .unwrap_or(0)
            .max(display_width(headers[column]))
    });
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(format_table_row(headers, &widths));
    for row in rows {
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
            output.push_str(&" ".repeat(widths[index] - display_width(value) + 2));
        }
    }
    output
}

fn display_width(value: &str) -> usize {
    let mut in_escape = false;
    value
        .chars()
        .filter_map(|character| {
            if in_escape {
                if character == 'm' {
                    in_escape = false;
                }
                return None;
            }
            if character == '\u{1b}' {
                in_escape = true;
                return None;
            }
            UnicodeWidthChar::width(character)
        })
        .sum()
}

fn colored_count(value: usize, ansi_color: u8) -> String {
    colorize(&value.to_string(), ansi_color)
}

fn colorize(value: &str, ansi_color: u8) -> String {
    format!("\u{1b}[{ansi_color}m{value}\u{1b}[0m")
}

fn mode_symbol(mode: WorkspaceManagementMode) -> &'static str {
    match mode {
        WorkspaceManagementMode::Automatic => "🤖",
        WorkspaceManagementMode::Manual => "👤",
    }
}

fn workspace_status_summary(workspace: &WorkspaceStatus) -> String {
    if workspace.claim.is_some() {
        format!("{} 🔒", workspace.state)
    } else {
        workspace.state.to_string()
    }
}

fn repository_summary(repositories: &[RepoWorktreeStatus], color: bool) -> String {
    let ready = repositories
        .iter()
        .filter(|repository| repository.state == RepoWorktreeState::Attached)
        .count();
    let capacity = repositories.len();
    if capacity == 0 {
        return if color {
            format!("{}/{}", colored_count(0, 32), colored_count(0, 34))
        } else {
            "0/0".to_owned()
        };
    }
    let paths = repositories
        .iter()
        .map(|repository| repository.source_path.clone())
        .collect::<Vec<_>>();
    let labels = shortest_unique_path_labels(&paths);
    let labels = repositories
        .iter()
        .zip(labels)
        .map(|(repository, label)| repository_label(&label, repository.state, color))
        .collect::<Vec<_>>();
    let counts = if color {
        format!(
            "{}/{}",
            colored_count(ready, 32),
            colored_count(capacity, 34)
        )
    } else {
        format!("{ready}/{capacity}")
    };
    format!("{counts} {}", labels.join(","))
}

fn repository_label(label: &str, state: RepoWorktreeState, color: bool) -> String {
    let (suffix, ansi_color) = match state {
        RepoWorktreeState::Attached => (None, 32),
        RepoWorktreeState::Pending => (Some("pending"), 33),
        RepoWorktreeState::Dirty => (Some("dirty"), 31),
        RepoWorktreeState::Missing => (Some("missing"), 31),
        RepoWorktreeState::Diverged => (Some("mismatch"), 31),
        RepoWorktreeState::Failed => (Some("error"), 31),
        RepoWorktreeState::Removed => (Some("removed"), 90),
    };
    let label = escape_human_label(label);
    let display = suffix.map_or_else(|| label.clone(), |suffix| format!("{label}({suffix})"));
    if color {
        colorize(&display, ansi_color)
    } else {
        display
    }
}

fn escape_human_label(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            ',' => escaped.push_str("\\,"),
            '(' => escaped.push_str("\\("),
            ')' => escaped.push_str("\\)"),
            character if character.is_control() => escaped.extend(character.escape_default()),
            character => escaped.push(character),
        }
    }
    escaped
}

fn shortest_unique_path_labels(paths: &[CanonicalPath]) -> Vec<String> {
    let components = paths
        .iter()
        .map(|source_path| {
            let parts = source_path
                .as_path()
                .iter()
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            if parts.is_empty() {
                vec![source_path.as_path().as_os_str().to_owned()]
            } else {
                parts
            }
        })
        .collect::<Vec<_>>();
    let mut depths = vec![1; components.len()];

    loop {
        let labels = components
            .iter()
            .zip(&depths)
            .map(|(parts, depth)| {
                parts[parts.len().saturating_sub(*depth)..]
                    .iter()
                    .collect::<PathBuf>()
                    .display()
                    .to_string()
            })
            .collect::<Vec<_>>();
        let counts = labels.iter().fold(HashMap::new(), |mut counts, label| {
            *counts.entry(label).or_insert(0_usize) += 1;
            counts
        });
        let mut expanded = false;
        for ((label, depth), parts) in labels.iter().zip(&mut depths).zip(&components) {
            if counts[label] > 1 && *depth < parts.len() {
                *depth += 1;
                expanded = true;
            }
        }
        if !expanded {
            return labels;
        }
    }
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

fn assemble_pool_snapshot(
    snapshot_at: Timestamp,
    workspaces: Vec<WorkspaceRow>,
    claims: Vec<WorkspaceClaimRow>,
    leases: Vec<crate::storage::OperationLeaseRow>,
    repositories: Vec<PoolOriginRepository>,
) -> PoolStatusSnapshot {
    let claimed = claims
        .into_iter()
        .map(|claim| claim.workspace_id)
        .collect::<HashSet<_>>();
    let leased = leases
        .into_iter()
        .map(|lease| lease.workspace_id)
        .collect::<HashSet<_>>();
    let mut workspaces_by_pool = HashMap::<PoolId, Vec<WorkspaceRow>>::new();
    for workspace in workspaces {
        if let Some(pool_id) = workspace.pool_id {
            workspaces_by_pool
                .entry(pool_id)
                .or_default()
                .push(workspace);
        }
    }
    let mut repositories_by_pool = HashMap::<PoolId, Vec<_>>::new();
    for relation in repositories {
        repositories_by_pool
            .entry(relation.pool_id)
            .or_default()
            .push(relation.repository);
    }

    let mut pools = workspaces_by_pool
        .into_iter()
        .map(|(pool_id, workspaces)| {
            let capacity = workspaces.len();
            let available = workspaces
                .iter()
                .filter(|workspace| {
                    workspace.state == WorkspaceState::Ready
                        && !claimed.contains(&workspace.id)
                        && !leased.contains(&workspace.id)
                })
                .count();
            let abnormal = workspaces
                .iter()
                .filter(|workspace| {
                    matches!(
                        workspace.state,
                        WorkspaceState::Degraded | WorkspaceState::Failed
                    )
                })
                .count();
            let updated_at = workspaces
                .iter()
                .map(|workspace| &workspace.updated_at)
                .max()
                .cloned();
            let origins = repositories_by_pool.remove(&pool_id).unwrap_or_default();
            let paths = origins
                .iter()
                .map(|origin| origin.source_path.clone())
                .collect::<Vec<_>>();
            let labels = shortest_unique_path_labels(&paths);
            let repositories = origins
                .into_iter()
                .zip(labels)
                .map(|(origin, label)| PoolRepositoryStatus {
                    origin_repository_id: origin.id,
                    source_path: origin.source_path,
                    label,
                })
                .collect();
            PoolStatus {
                pool_id,
                repositories,
                available,
                capacity,
                abnormal,
                updated_at,
            }
        })
        .collect::<Vec<_>>();
    pools.sort_by(|left, right| {
        let left_paths = left
            .repositories
            .iter()
            .map(|repository| repository.source_path.to_string())
            .collect::<Vec<_>>();
        let right_paths = right
            .repositories
            .iter()
            .map(|repository| repository.source_path.to_string())
            .collect::<Vec<_>>();
        left_paths
            .cmp(&right_paths)
            .then_with(|| left.pool_id.to_string().cmp(&right.pool_id.to_string()))
    });
    PoolStatusSnapshot {
        schema_version: STATUS_SCHEMA_VERSION,
        view: StatusView::Pools,
        snapshot_at,
        target_workspace: None,
        pools,
    }
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
                removed_at: workspace.removed_at,
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
        view: StatusView::Workspaces,
        snapshot_at,
        target_workspace: None,
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
        insert_workspace_claim, insert_workspace_pool, insert_workspace_pool_repositories,
        persist_operation_intent, EventDraft, NewManagedWorkspace, NewRepoWorktree,
        NewWorkspaceClaim, NewWorkspacePool, NewWorkspacePoolRepository, OperationIntent,
    };

    fn temporary_database_path() -> PathBuf {
        std::env::temp_dir().join(format!("trees-status-{}.sqlite", WorkspaceId::new()))
    }

    fn path(value: &str) -> CanonicalPath {
        CanonicalPath::from_absolute(value).expect("test path should be absolute")
    }

    pub(super) fn insert_workspace(
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
                removed_at: (state == WorkspaceState::Removed).then(Timestamp::now),
            },
        )
        .expect("workspace should be inserted");
        id
    }

    pub(super) fn insert_automatic_workspace(
        connection: &mut SqliteConnection,
        value: &str,
        state: WorkspaceState,
        pool_id: PoolId,
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
                management_mode: WorkspaceManagementMode::Automatic,
                pool_id: Some(pool_id),
                last_released_at: None,
                removed_at: (state == WorkspaceState::Removed).then(Timestamp::now),
            },
        )
        .expect("automatic workspace should be inserted");
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
    fn aggregates_current_automatic_pool_allocation() {
        let database_path = temporary_database_path();
        let mut connection = database::connect(&database_path).expect("database should open");
        let pool_id = PoolId::new();
        insert_workspace_pool(
            &mut connection,
            &NewWorkspacePool {
                id: pool_id,
                hash_key: "pool".to_owned(),
                repository_ids: "[]".to_owned(),
            },
        )
        .expect("pool should be inserted");
        let origin = ensure_origin_repository(
            &mut connection,
            &path("/origins/api"),
            &path("/origins/api"),
        )
        .expect("origin should be inserted");
        insert_workspace_pool_repositories(
            &mut connection,
            &[NewWorkspacePoolRepository {
                pool_id,
                repository_id: origin.id,
            }],
        )
        .expect("pool relation should be inserted");

        insert_automatic_workspace(
            &mut connection,
            "/pool/available",
            WorkspaceState::Ready,
            pool_id,
        );
        let allocated_id = insert_automatic_workspace(
            &mut connection,
            "/pool/allocated",
            WorkspaceState::Ready,
            pool_id,
        );
        insert_workspace_claim(
            &mut connection,
            &NewWorkspaceClaim {
                id: ClaimId::new(),
                workspace_id: allocated_id,
                claimed_at: Timestamp::now(),
            },
        )
        .expect("claim should be inserted");
        let leased_id = insert_automatic_workspace(
            &mut connection,
            "/pool/leased",
            WorkspaceState::Ready,
            pool_id,
        );
        persist_operation_intent(
            &mut connection,
            &operation(leased_id, Timestamp::parse("9999-01-01T00:00:00Z").unwrap()),
        )
        .expect("operation should be inserted");
        insert_automatic_workspace(
            &mut connection,
            "/pool/degraded",
            WorkspaceState::Degraded,
            pool_id,
        );
        insert_automatic_workspace(
            &mut connection,
            "/pool/removed",
            WorkspaceState::Removed,
            pool_id,
        );
        insert_workspace(&mut connection, "/pool/manual", WorkspaceState::Ready);

        let snapshot = load_pool_snapshot(&mut connection).expect("pool snapshot should load");

        assert_eq!(snapshot.view, StatusView::Pools);
        assert_eq!(snapshot.pools.len(), 1);
        let pool = &snapshot.pools[0];
        assert_eq!(pool.pool_id, pool_id);
        assert_eq!(pool.available, 1);
        assert_eq!(pool.capacity, 4);
        assert_eq!(pool.abnormal, 1);
        assert_eq!(pool.repositories[0].label, "api");
        assert!(render_pools_human(&snapshot, false).contains("1/4/1"));
        let colored = render_pools_human(&snapshot, true);
        assert!(colored.contains("\u{1b}[32m1\u{1b}[0m"));
        assert!(colored.contains("\u{1b}[34m4\u{1b}[0m"));
        assert!(colored.contains("\u{1b}[31m1\u{1b}[0m"));
        assert_eq!(display_width("\u{1b}[31m1\u{1b}[0m"), 1);

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn loads_ordered_related_rows_and_filters_removed_workspaces() {
        let database_path = temporary_database_path();
        let mut connection = database::connect(&database_path).expect("database should open");
        let second_id = insert_workspace(&mut connection, "/status/zeta", WorkspaceState::Ready);
        let first_id = insert_workspace(&mut connection, "/status/alpha", WorkspaceState::Ready);
        insert_workspace(&mut connection, "/status/removed", WorkspaceState::Removed);
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
        assert_eq!(
            render_workspaces_human(&StatusSnapshot::empty(), false),
            "No workspaces."
        );

        let workspace_id = WorkspaceId::new();
        let snapshot = StatusSnapshot {
            schema_version: STATUS_SCHEMA_VERSION,
            view: StatusView::Workspaces,
            snapshot_at: Timestamp::parse("2026-09-08T12:00:00Z").unwrap(),
            target_workspace: None,
            workspaces: vec![WorkspaceStatus {
                workspace_id,
                path: path("/status/example"),
                management_mode: WorkspaceManagementMode::Automatic,
                state: WorkspaceState::Degraded,
                created_at: Timestamp::parse("2026-09-01T00:00:00Z").unwrap(),
                updated_at: Timestamp::parse("2026-09-08T10:00:00Z").unwrap(),
                last_reconciled_at: Some(Timestamp::parse("2026-09-08T10:00:00Z").unwrap()),
                last_released_at: None,
                removed_at: None,
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

        let output = render_workspaces_human(&snapshot, false);

        assert_eq!(
            output,
            format!(
                "STATUS       MODE  REPOS               RECONCILED  ID\n\
                 degraded 🔒  🤖    0/1 example(dirty)  10:00       {workspace_id}"
            )
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
    fn renders_shortest_unique_repository_labels() {
        let unique = vec![
            repository("/origins/api", RepoWorktreeState::Attached),
            repository("/origins/web", RepoWorktreeState::Attached),
        ];
        assert_eq!(repository_summary(&unique, false), "2/2 api,web");
        let colored = repository_summary(&unique, true);
        assert!(colored.starts_with("\u{1b}[32m2\u{1b}[0m/\u{1b}[34m2\u{1b}[0m "));

        let conflicting = vec![
            repository("/teams/one/api", RepoWorktreeState::Attached),
            repository("/teams/two/api", RepoWorktreeState::Attached),
            repository("/teams/two/web", RepoWorktreeState::Dirty),
        ];
        assert_eq!(
            repository_summary(&conflicting, false),
            "2/3 one/api,two/api,web(dirty)"
        );

        let recursive = vec![
            repository("/org/red/services/api", RepoWorktreeState::Attached),
            repository("/org/blue/services/api", RepoWorktreeState::Dirty),
        ];
        assert_eq!(
            repository_summary(&recursive, false),
            "1/2 red/services/api,blue/services/api(dirty)"
        );
        assert_eq!(repository_summary(&[], false), "0/0");
    }

    #[test]
    fn labels_problem_repositories_with_friendly_states() {
        let repositories = [
            repository("/origins/ready", RepoWorktreeState::Attached),
            repository("/origins/pending", RepoWorktreeState::Pending),
            repository("/origins/dirty", RepoWorktreeState::Dirty),
            repository("/origins/missing", RepoWorktreeState::Missing),
            repository("/origins/mismatch", RepoWorktreeState::Diverged),
            repository("/origins/error", RepoWorktreeState::Failed),
            repository("/origins/removed", RepoWorktreeState::Removed),
        ];

        assert_eq!(
            repository_summary(&repositories, false),
            "1/7 ready,pending(pending),dirty(dirty),missing(missing),\
             mismatch(mismatch),error(error),removed(removed)"
        );
        let colored = repository_summary(&repositories, true);
        assert!(colored.contains("\u{1b}[32mready\u{1b}[0m"));
        assert!(colored.contains("\u{1b}[33mpending(pending)\u{1b}[0m"));
        assert!(colored.contains("\u{1b}[31mdirty(dirty)\u{1b}[0m"));
        assert!(colored.contains("\u{1b}[90mremoved(removed)\u{1b}[0m"));
    }

    #[test]
    fn escapes_untrusted_repository_label_characters() {
        assert_eq!(
            escape_human_label("api,\n(tab)\u{1b}\\"),
            "api\\,\\n\\(tab\\)\\u{1b}\\\\"
        );

        let repository = repository("/origins/api,\n(red)\u{1b}", RepoWorktreeState::Dirty);
        let plain = repository_summary(&[repository], false);
        assert_eq!(plain.lines().count(), 1);
        assert!(!plain.contains('\u{1b}'));
        assert!(plain.contains("api\\,\\n\\(red\\)\\u{1b}(dirty)"));
    }

    fn repository(source_path: &str, state: RepoWorktreeState) -> RepoWorktreeStatus {
        RepoWorktreeStatus {
            repo_worktree_id: RepoWorktreeId::new(),
            origin_repository_id: OriginRepositoryId::new(),
            source_path: path(source_path),
            worktree_path: CanonicalPath::from_absolute(format!(
                "/worktrees/{}",
                RepoWorktreeId::new()
            ))
            .expect("test worktree path should be absolute"),
            state,
            last_head: None,
            last_observed_at: Timestamp::now(),
        }
    }

    #[test]
    fn serializes_the_versioned_json_contract() {
        let snapshot = StatusSnapshot::empty();
        let value = serde_json::to_value(snapshot).expect("status snapshot should serialize");

        assert_eq!(value["schema_version"], STATUS_SCHEMA_VERSION);
        assert_eq!(value["view"], "workspaces");
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
