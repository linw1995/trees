use std::fs;

use diesel::prelude::*;
use diesel_migrations::MigrationHarness;

use trees::database;
use trees::domain::{
    CanonicalPath, EventId, JsonDocument, OperationState, Timestamp, WorkspaceId,
    WorkspaceManagementMode, WorkspaceState,
};
use trees::storage::{
    begin_operation, insert_event, insert_workspace, persist_operation_intent, EventRow, NewEvent,
    NewWorkspace, OperationIntent, OperationIntentError,
};

fn database_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("trees-persistence-{}.sqlite", WorkspaceId::new()))
}

fn workspace(id: WorkspaceId, path: CanonicalPath) -> NewWorkspace {
    let now = Timestamp::now();
    NewWorkspace {
        id,
        canonical_path: path,
        state: WorkspaceState::Creating,
        created_at: now.clone(),
        updated_at: now,
        last_reconciled_at: None,
    }
}

#[test]
fn embedded_migrations_can_be_reverted_and_rerun() {
    let path = database_path();
    let mut connection = database::connect(&path).expect("database should open");

    connection
        .revert_last_migration(database::MIGRATIONS)
        .expect("migration should revert");
    connection
        .run_pending_migrations(database::MIGRATIONS)
        .expect("migration should run again");
    let count = trees::schema::workspaces::table
        .count()
        .get_result::<i64>(&mut connection)
        .expect("migrated table should be queryable");

    assert_eq!(count, 0);
    drop(connection);
    fs::remove_file(path).expect("temporary database should be removable");
}

#[test]
fn database_constraints_and_immutable_events_are_enforced() {
    let path = database_path();
    let mut connection = database::connect(&path).expect("database should open");
    let workspace_id = WorkspaceId::new();
    let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
    insert_workspace(
        &mut connection,
        &workspace(workspace_id, workspace_path.clone()),
    )
    .expect("workspace should be inserted");
    assert!(insert_workspace(
        &mut connection,
        &workspace(WorkspaceId::new(), workspace_path),
    )
    .is_err());

    let invalid_workspace_intent = OperationIntent::new(
        WorkspaceId::new(),
        "create",
        "test-owner",
        Timestamp::after_seconds(300),
        "attach repo",
        JsonDocument::parse("{}").unwrap(),
    );
    assert!(matches!(
        persist_operation_intent(&mut connection, &invalid_workspace_intent),
        Err(diesel::result::Error::DatabaseError(_, _))
    ));

    let operation = begin_operation(
        &mut connection,
        &OperationIntent::new(
            workspace_id,
            "create",
            "test-owner",
            Timestamp::after_seconds(300),
            "attach repo",
            JsonDocument::parse("{}").unwrap(),
        ),
    )
    .expect("operation should start");
    let event = insert_event(
        &mut connection,
        &NewEvent {
            event_id: EventId::new(),
            operation_id: operation.id,
            entity_type: "operation".to_owned(),
            entity_id: operation.id.to_string(),
            event_type: "started".to_owned(),
            source: "trees".to_owned(),
            occurred_at: Timestamp::now(),
            previous_state: None,
            current_state: Some(OperationState::Running.to_string()),
            details_json: None,
            error_json: None,
        },
    )
    .expect("event should be inserted");
    let update_result = diesel::update(trees::schema::lifecycle_events::table.find(event.event_id))
        .set(trees::schema::lifecycle_events::event_type.eq("changed"))
        .execute(&mut connection);
    assert!(update_result.is_err());

    let stored = trees::schema::lifecycle_events::table
        .find(event.event_id)
        .select(EventRow::as_select())
        .first::<EventRow>(&mut connection)
        .expect("event should remain readable");
    assert_eq!(stored.event_type, "started");
    assert!(matches!(
        begin_operation(
            &mut connection,
            &OperationIntent::new(
                workspace_id,
                "create",
                "second-owner",
                Timestamp::after_seconds(300),
                "attach repo",
                JsonDocument::parse("{}").unwrap(),
            ),
        ),
        Err(OperationIntentError::WorkspaceBusy(_))
    ));

    drop(connection);
    fs::remove_file(path).expect("temporary database should be removable");
}

#[test]
fn legacy_workspace_rows_default_to_manual_without_pool_metadata() {
    let path = database_path();
    let mut connection = database::connect(&path).expect("database should open");
    let workspace_id = WorkspaceId::new();
    let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");

    insert_workspace(
        &mut connection,
        &workspace(workspace_id, workspace_path.clone()),
    )
    .expect("legacy workspace should be inserted");
    let stored = trees::storage::find_workspace(&mut connection, &workspace_id)
        .expect("workspace should be queryable");

    assert_eq!(stored.management_mode, WorkspaceManagementMode::Manual);
    assert_eq!(stored.pool_key, None);
    assert_eq!(stored.workspace_root, None);
    assert_eq!(stored.last_checked_in_at, None);
    assert_eq!(stored.reclaimed_at, None);
    assert!(workspace_path.as_path().exists());

    drop(connection);
    fs::remove_file(path).expect("temporary database should be removable");
}
