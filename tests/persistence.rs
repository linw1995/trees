use std::fs;

use diesel::migration::MigrationSource;
use diesel::prelude::*;
use diesel_migrations::MigrationHarness;

use trees::database;
use trees::domain::{
    CanonicalPath, EventId, JsonDocument, OperationState, Timestamp, WorkspaceId,
    WorkspaceManagementMode, WorkspaceState,
};
use trees::storage::{
    begin_operation, ensure_origin_repository, find_operation, insert_event, insert_repo_worktree,
    insert_workspace, list_repo_worktrees, operation_state, persist_operation_intent, EventRow,
    NewEvent, NewRepoWorktree, NewWorkspace, OperationIntent, OperationIntentError,
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
fn combined_feature_migration_preserves_legacy_workspace() {
    let path = database_path();
    let mut connection = database::connect(&path).expect("database should open");
    connection
        .revert_last_migration(database::MIGRATIONS)
        .expect("feature migration should revert");

    let workspace_id = WorkspaceId::new();
    let repo_worktree_id = trees::domain::RepoWorktreeId::new();
    let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
    let source_path = CanonicalPath::resolve("Cargo.toml").expect("source path should resolve");
    let worktree_path = CanonicalPath::resolve("/tmp").expect("worktree path should resolve");
    let now = Timestamp::now();

    diesel::sql_query(
        "INSERT INTO workspaces \
         (id, canonical_path, state, created_at, updated_at, last_reconciled_at) \
         VALUES (?, ?, 'ready', ?, ?, NULL)",
    )
    .bind::<diesel::sql_types::Text, _>(workspace_id.to_string())
    .bind::<diesel::sql_types::Text, _>(workspace_path.to_string())
    .bind::<diesel::sql_types::Text, _>(now.to_string())
    .bind::<diesel::sql_types::Text, _>(now.to_string())
    .execute(&mut connection)
    .expect("legacy workspace should be inserted");
    diesel::sql_query(
        "INSERT INTO repo_worktrees \
         (id, workspace_id, repository_identity, source_path, worktree_path, state, last_head, \
          last_observed_at) VALUES (?, ?, ?, ?, ?, 'attached', ?, ?)",
    )
    .bind::<diesel::sql_types::Text, _>(repo_worktree_id.to_string())
    .bind::<diesel::sql_types::Text, _>(workspace_id.to_string())
    .bind::<diesel::sql_types::Text, _>(source_path.to_string())
    .bind::<diesel::sql_types::Text, _>(source_path.to_string())
    .bind::<diesel::sql_types::Text, _>(worktree_path.to_string())
    .bind::<diesel::sql_types::Text, _>("abc123")
    .bind::<diesel::sql_types::Text, _>(now.to_string())
    .execute(&mut connection)
    .expect("legacy repo worktree should be inserted");

    connection
        .run_pending_migrations(database::MIGRATIONS)
        .expect("feature migration should apply");
    let migrated = trees::storage::find_workspace(&mut connection, &workspace_id)
        .expect("migrated workspace should be queryable");
    assert_eq!(migrated.management_mode, WorkspaceManagementMode::Manual);
    assert_eq!(migrated.pool_id, None);
    assert_eq!(migrated.last_released_at, None);
    assert!(
        trees::storage::find_workspace_claim(&mut connection, &workspace_id)
            .expect("claim table should be queryable")
            .is_none()
    );
    let migrated_worktree = trees::storage::list_repo_worktrees(&mut connection, &workspace_id)
        .expect("migrated worktree should be queryable")
        .pop()
        .expect("migrated workspace should have a worktree");
    assert_eq!(migrated_worktree.repository_identity, source_path);
    assert_eq!(migrated_worktree.source_path, source_path);

    drop(connection);
    fs::remove_file(path).expect("temporary database should be removable");
}

#[test]
fn combined_feature_migration_preserves_legacy_operation_lease() {
    let path = database_path();
    let mut connection = database::connect(&path).expect("database should open");
    connection
        .revert_last_migration(database::MIGRATIONS)
        .expect("feature migration should revert");

    let workspace_id = WorkspaceId::new();
    let operation_id = trees::domain::OperationId::new();
    let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
    let started_at = Timestamp::now();
    let lease_expires_at = Timestamp::after_seconds(300);

    diesel::sql_query(
        "INSERT INTO workspaces \
         (id, canonical_path, state, created_at, updated_at, last_reconciled_at) \
         VALUES (?, ?, 'creating', ?, ?, NULL)",
    )
    .bind::<diesel::sql_types::Text, _>(workspace_id.to_string())
    .bind::<diesel::sql_types::Text, _>(workspace_path.to_string())
    .bind::<diesel::sql_types::Text, _>(started_at.to_string())
    .bind::<diesel::sql_types::Text, _>(started_at.to_string())
    .execute(&mut connection)
    .expect("legacy workspace should be inserted");
    diesel::sql_query(
        "INSERT INTO operations \
         (id, workspace_id, kind, state, owner_id, lease_expires_at, last_heartbeat_at, \
          started_at, finished_at, pending_step, intent_json, error_json) \
         VALUES (?, ?, 'create', 'running', ?, ?, ?, ?, NULL, ?, ?, NULL)",
    )
    .bind::<diesel::sql_types::Text, _>(operation_id.to_string())
    .bind::<diesel::sql_types::Text, _>(workspace_id.to_string())
    .bind::<diesel::sql_types::Text, _>("legacy-process")
    .bind::<diesel::sql_types::Text, _>(lease_expires_at.to_string())
    .bind::<diesel::sql_types::Text, _>(started_at.to_string())
    .bind::<diesel::sql_types::Text, _>(started_at.to_string())
    .bind::<diesel::sql_types::Text, _>("prepare worktrees")
    .bind::<diesel::sql_types::Text, _>(r#"{"legacy":true}"#)
    .execute(&mut connection)
    .expect("legacy operation should be inserted");

    connection
        .run_pending_migrations(database::MIGRATIONS)
        .expect("feature migration should apply");

    let operation = find_operation(&mut connection, &operation_id)
        .expect("migrated operation fact should be queryable");
    assert_eq!(operation.id, operation_id);
    assert_eq!(operation.workspace_id, workspace_id);
    assert_eq!(operation.kind, "create");
    assert_eq!(operation.started_at, started_at);
    assert_eq!(
        operation_state(&mut connection, &operation_id)
            .expect("migrated operation state should be queryable"),
        Some(OperationState::Running)
    );
    let lease = trees::storage::find_operation_lease(&mut connection, &operation_id)
        .expect("migrated operation lease should be queryable")
        .expect("running legacy operation should retain its lease");
    assert_eq!(lease.operation_id, operation_id);
    assert_eq!(lease.workspace_id, workspace_id);
    assert_eq!(lease.id.to_string(), operation_id.to_string());
    assert_eq!(lease.lease_expires_at, lease_expires_at);

    drop(connection);
    fs::remove_file(path).expect("temporary database should be removable");
}

#[test]
fn migration_preserves_operation_and_event_rows_without_rebuilding_them() {
    let path = database_path();
    let mut connection = database::connect(&path).expect("database should open");
    let workspace_id = WorkspaceId::new();
    let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
    insert_workspace(
        &mut connection,
        &workspace(workspace_id, workspace_path.clone()),
    )
    .expect("workspace should be inserted");
    let origin = ensure_origin_repository(&mut connection, &workspace_path, &workspace_path)
        .expect("origin repository should be inserted");
    insert_repo_worktree(
        &mut connection,
        &NewRepoWorktree {
            id: trees::domain::RepoWorktreeId::new(),
            workspace_id,
            origin_repository_id: origin.id,
            worktree_path: CanonicalPath::resolve("/tmp").expect("worktree path should resolve"),
            state: trees::domain::RepoWorktreeState::Pending,
            last_head: None,
            last_observed_at: Timestamp::now(),
        },
    )
    .expect("repo worktree should be inserted");
    let operation = begin_operation(
        &mut connection,
        &OperationIntent::new(
            workspace_id,
            "create",
            Timestamp::after_seconds(300),
            "attach repository",
            JsonDocument::parse("{}").unwrap(),
        ),
    )
    .expect("operation should be inserted");
    let event = insert_event(
        &mut connection,
        &NewEvent {
            event_id: EventId::new(),
            operation_id: operation.id,
            entity_type: "operation".to_owned(),
            entity_id: operation.id.to_string(),
            event_type: "migration_probe".to_owned(),
            source: "test".to_owned(),
            occurred_at: Timestamp::now(),
            previous_state: None,
            current_state: Some(OperationState::Running.to_string()),
            details_json: None,
            error_json: None,
        },
    )
    .expect("event should be inserted");

    connection
        .revert_last_migration(database::MIGRATIONS)
        .expect("migration should downgrade");
    assert_eq!(
        find_operation(&mut connection, &operation.id)
            .expect("operation should survive downgrade")
            .id,
        operation.id
    );
    assert_eq!(
        trees::schema::lifecycle_events::table
            .find(event.event_id)
            .select(EventRow::as_select())
            .first::<EventRow>(&mut connection)
            .expect("event should survive downgrade")
            .event_type,
        "migration_probe"
    );

    connection
        .run_pending_migrations(database::MIGRATIONS)
        .expect("migration should upgrade");
    assert_eq!(
        operation_state(&mut connection, &operation.id)
            .expect("operation state should survive upgrade"),
        Some(OperationState::Running)
    );
    assert_eq!(
        trees::schema::lifecycle_events::table
            .find(event.event_id)
            .select(EventRow::as_select())
            .first::<EventRow>(&mut connection)
            .expect("event should survive upgrade")
            .event_type,
        "migration_probe"
    );
    assert!(
        diesel::update(trees::schema::lifecycle_events::table.find(event.event_id))
            .set(trees::schema::lifecycle_events::event_type.eq("should_fail"))
            .execute(&mut connection)
            .is_err()
    );
    assert_eq!(
        trees::storage::find_workspace(&mut connection, &workspace_id)
            .expect("workspace should survive upgrade")
            .canonical_path,
        workspace_path
    );
    let repositories = list_repo_worktrees(&mut connection, &workspace_id)
        .expect("repo worktrees should survive upgrade");
    assert_eq!(repositories.len(), 1);
    assert_eq!(repositories[0].repository_identity, workspace_path);
    assert_eq!(
        repositories[0].source_path,
        repositories[0].repository_identity
    );

    drop(connection);
    fs::remove_file(path).expect("temporary database should be removable");
}

#[test]
fn embedded_migrations_are_consolidated() {
    let path = database_path();
    let mut connection = database::connect(&path).expect("database should open");
    let migrations = MigrationSource::<diesel::sqlite::Sqlite>::migrations(&database::MIGRATIONS)
        .expect("embedded migrations should load");
    assert_eq!(migrations.len(), 2);
    assert!(trees::storage::find_workspace_claim_by_id(
        &mut connection,
        &trees::domain::ClaimId::new(),
    )
    .is_err());
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
    assert_eq!(stored.pool_id, None);
    assert_eq!(stored.last_released_at, None);
    assert_eq!(stored.reclaimed_at, None);
    assert!(workspace_path.as_path().exists());

    drop(connection);
    fs::remove_file(path).expect("temporary database should be removable");
}

#[test]
fn automatic_workspace_metadata_and_timestamps_round_trip_as_absolute_values() {
    let path = database_path();
    let mut connection = database::connect(&path).expect("database should open");
    let workspace_id = WorkspaceId::new();
    let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
    let repository_path =
        CanonicalPath::from_absolute("/repo/api").expect("repository path should be absolute");
    let repository_id =
        ensure_origin_repository(&mut connection, &repository_path, &repository_path)
            .expect("origin repository should be available")
            .id;
    let now = Timestamp::now();
    let pool = trees::storage::ensure_workspace_pool(
        &mut connection,
        &trees::pool::RepositorySetKey::from_repository_ids(&[repository_id]),
    )
    .expect("workspace pool should be available");

    insert_workspace(&mut connection, &workspace(workspace_id, workspace_path))
        .expect("workspace should be inserted");
    diesel::update(trees::schema::workspaces::table.find(&workspace_id))
        .set((
            trees::schema::workspaces::management_mode.eq(WorkspaceManagementMode::Automatic),
            trees::schema::workspaces::pool_id.eq(Some(pool.id)),
            trees::schema::workspaces::last_released_at.eq(Some(now.clone())),
            trees::schema::workspaces::reclaimed_at.eq::<Option<Timestamp>>(None),
        ))
        .execute(&mut connection)
        .expect("automatic metadata should update");

    let stored = trees::storage::find_workspace(&mut connection, &workspace_id)
        .expect("workspace should be queryable");
    assert_eq!(stored.management_mode, WorkspaceManagementMode::Automatic);
    assert_eq!(stored.pool_id, Some(pool.id));
    assert_eq!(stored.last_released_at, Some(now));
    assert!(stored.canonical_path.as_path().is_absolute());

    drop(connection);
    fs::remove_file(path).expect("temporary database should be removable");
}
