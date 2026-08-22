use diesel::prelude::*;
use diesel::result::QueryResult;
use diesel::sqlite::SqliteConnection;

use crate::domain::{CanonicalPath, OperationId, OperationState, Timestamp, WorkspaceId};
use crate::schema::{lifecycle_events, operations, repo_worktrees, workspaces};

use super::models::{
    EventRow, NewEvent, NewOperation, NewRepoWorktree, NewWorkspace, OperationRow, RepoWorktreeRow,
    WorkspaceRow,
};

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

pub fn insert_repo_worktree(
    connection: &mut SqliteConnection,
    value: &NewRepoWorktree,
) -> QueryResult<RepoWorktreeRow> {
    diesel::insert_into(repo_worktrees::table)
        .values(value)
        .execute(connection)?;
    repo_worktrees::table
        .find(&value.id)
        .select(RepoWorktreeRow::as_select())
        .first(connection)
}

pub fn list_repo_worktrees(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
) -> QueryResult<Vec<RepoWorktreeRow>> {
    repo_worktrees::table
        .filter(repo_worktrees::workspace_id.eq(workspace_id))
        .order(repo_worktrees::worktree_path.asc())
        .select(RepoWorktreeRow::as_select())
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

pub fn find_operation(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
) -> QueryResult<OperationRow> {
    operations::table
        .find(operation_id)
        .select(OperationRow::as_select())
        .first(connection)
}

pub fn find_running_operation(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
) -> QueryResult<Option<OperationRow>> {
    operations::table
        .filter(operations::workspace_id.eq(workspace_id))
        .filter(operations::state.eq(OperationState::Running))
        .select(OperationRow::as_select())
        .first(connection)
        .optional()
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
    use crate::database;
    use crate::domain::{
        CanonicalPath, EventId, JsonDocument, OperationId, OperationState, RepoWorktreeId,
        RepoWorktreeState, Timestamp, WorkspaceId, WorkspaceState,
    };

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

        let worktree = insert_repo_worktree(
            &mut connection,
            &NewRepoWorktree {
                id: RepoWorktreeId::new(),
                workspace_id,
                repository_identity: workspace_path.clone(),
                source_path: workspace_path.clone(),
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

        let operation_id = OperationId::new();
        let operation = insert_operation(
            &mut connection,
            &NewOperation {
                id: operation_id,
                workspace_id,
                kind: "create".to_owned(),
                state: OperationState::Running,
                owner_id: "test-owner".to_owned(),
                lease_expires_at: now.clone(),
                last_heartbeat_at: now.clone(),
                started_at: now.clone(),
                finished_at: None,
                pending_step: "attach".to_owned(),
                intent_json: JsonDocument::parse(r#"{"workspace":"test"}"#).unwrap(),
                error_json: None,
            },
        )
        .expect("operation should be inserted");
        assert_eq!(
            find_running_operation(&mut connection, &workspace_id)
                .unwrap()
                .unwrap()
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
            1
        );
        assert_eq!(event.operation_id, operation_id);

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }
}
