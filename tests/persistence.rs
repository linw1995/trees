use std::fs;

use diesel::migration::MigrationSource;
use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use diesel::Connection;
use diesel_migrations::MigrationHarness;

use trees::database;
use trees::domain::{
    CanonicalPath, ClaimId, EventId, JsonDocument, OperationState, Timestamp, WorkspaceId,
    WorkspaceManagementMode, WorkspaceState,
};
use trees::storage::{
    begin_operation, ensure_origin_repository, find_operation, insert_event, insert_repo_worktree,
    insert_workspace, list_repo_worktrees, persist_operation_intent, EventRow, NewEvent,
    NewRepoWorktree, NewWorkspace, OperationIntent, OperationIntentError,
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
fn workspace_claim_migration_preserves_claim_metadata() {
    let path = database_path();
    let mut connection = database::connect(&path).expect("database should open");
    connection
        .revert_last_migration(database::MIGRATIONS)
        .expect("claim migration should revert");

    let workspace_id = WorkspaceId::new();
    let claim_id = ClaimId::new();
    let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
    let claimed_at =
        Timestamp::parse("2026-01-01T00:00:00Z").expect("claim timestamp should parse");

    diesel::sql_query(
        "INSERT INTO workspaces (id, canonical_path, state, created_at, updated_at, \
         last_reconciled_at, management_mode, pool_key, last_checked_in_at, reclaimed_at) \
         VALUES (?, ?, 'ready', ?, ?, NULL, 'manual', NULL, NULL, NULL)",
    )
    .bind::<diesel::sql_types::Text, _>(workspace_id.to_string())
    .bind::<diesel::sql_types::Text, _>(workspace_path.to_string())
    .bind::<diesel::sql_types::Text, _>(claimed_at.to_string())
    .bind::<diesel::sql_types::Text, _>(claimed_at.to_string())
    .execute(&mut connection)
    .expect("legacy workspace should be inserted");
    diesel::sql_query(
        "INSERT INTO workspace_leases \
         (id, workspace_id, owner_id, checked_out_at, lease_expires_at, last_heartbeat_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind::<diesel::sql_types::Text, _>(claim_id.to_string())
    .bind::<diesel::sql_types::Text, _>(workspace_id.to_string())
    .bind::<diesel::sql_types::Text, _>("process:legacy")
    .bind::<diesel::sql_types::Text, _>(claimed_at.to_string())
    .bind::<diesel::sql_types::Text, _>("9999-12-31T23:59:59Z")
    .bind::<diesel::sql_types::Text, _>(claimed_at.to_string())
    .execute(&mut connection)
    .expect("legacy workspace lease should be inserted");

    connection
        .run_pending_migrations(database::MIGRATIONS)
        .expect("claim migration should apply");
    let migrated = trees::storage::find_workspace_claim_by_id(&mut connection, &claim_id)
        .expect("migrated claim should be queryable");
    assert_eq!(migrated.workspace_id, workspace_id);
    assert_eq!(migrated.claimed_at, claimed_at);

    connection
        .revert_last_migration(database::MIGRATIONS)
        .expect("claim migration should downgrade");
    connection
        .run_pending_migrations(database::MIGRATIONS)
        .expect("claim migration should rerun");
    let rerun = trees::storage::find_workspace_claim_by_id(&mut connection, &claim_id)
        .expect("rerun claim should be queryable");
    assert_eq!(rerun.claimed_at, claimed_at);

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
            "migration-test",
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
        find_operation(&mut connection, &operation.id)
            .expect("operation should survive upgrade")
            .state,
        OperationState::Running
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
fn migration_three_normalizes_a_database_already_at_migration_two() {
    let root = std::env::temp_dir().join(format!("trees-migration-{}", WorkspaceId::new()));
    fs::create_dir_all(&root).expect("migration test root should be created");
    let path = root.join("state.sqlite");
    let mut connection = SqliteConnection::establish(path.to_str().expect("path should be UTF-8"))
        .expect("legacy database should open");
    let migrations = MigrationSource::<diesel::sqlite::Sqlite>::migrations(&database::MIGRATIONS)
        .expect("embedded migrations should load");
    diesel::migration::MigrationConnection::setup(&mut connection)
        .expect("migration metadata table should be created");
    connection
        .run_migrations(&migrations[..2])
        .expect("legacy migrations should apply");

    let workspace_id = WorkspaceId::new();
    let repo_worktree_id = trees::domain::RepoWorktreeId::new();
    let workspace_path = CanonicalPath::from_absolute(root.join("workspace"))
        .expect("workspace path should be absolute");
    let workspace_root = CanonicalPath::from_absolute(root.join("managed"))
        .expect("workspace root should be absolute");
    let source_path =
        CanonicalPath::from_absolute(root.join("source")).expect("source path should be absolute");
    let worktree_path = CanonicalPath::from_absolute(root.join("workspace/repo"))
        .expect("worktree path should be absolute");
    let repository_set =
        trees::pool::RepositorySetKey::from_repositories(std::slice::from_ref(&source_path));
    let now = Timestamp::now();

    diesel::sql_query(
        "INSERT INTO workspaces (id, canonical_path, state, created_at, updated_at, \
         last_reconciled_at, management_mode, pool_key, workspace_root, \
         last_checked_in_at, reclaimed_at) \
         VALUES (?, ?, 'ready', ?, ?, NULL, 'automatic', ?, ?, ?, NULL)",
    )
    .bind::<diesel::sql_types::Text, _>(workspace_id.to_string())
    .bind::<diesel::sql_types::Text, _>(workspace_path.to_string())
    .bind::<diesel::sql_types::Text, _>(now.to_string())
    .bind::<diesel::sql_types::Text, _>(now.to_string())
    .bind::<diesel::sql_types::Text, _>(repository_set.hash_key().to_owned())
    .bind::<diesel::sql_types::Text, _>(workspace_root.to_string())
    .bind::<diesel::sql_types::Text, _>(now.to_string())
    .execute(&mut connection)
    .expect("legacy workspace should be inserted");
    diesel::sql_query(
        "INSERT INTO repo_worktrees (id, workspace_id, repository_identity, source_path, \
         worktree_path, state, last_head, last_observed_at) \
         VALUES (?, ?, ?, ?, ?, 'attached', ?, ?)",
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
    let second_workspace_id = WorkspaceId::new();
    let second_repo_worktree_id = trees::domain::RepoWorktreeId::new();
    let second_workspace_path = CanonicalPath::from_absolute(root.join("workspace-two"))
        .expect("second workspace path should be absolute");
    let second_worktree_path = CanonicalPath::from_absolute(root.join("workspace-two/repo"))
        .expect("second worktree path should be absolute");
    diesel::sql_query(
        "INSERT INTO workspaces (id, canonical_path, state, created_at, updated_at, \
         last_reconciled_at, management_mode, pool_key, workspace_root, \
         last_checked_in_at, reclaimed_at) \
         VALUES (?, ?, 'ready', ?, ?, NULL, 'automatic', ?, ?, ?, NULL)",
    )
    .bind::<diesel::sql_types::Text, _>(second_workspace_id.to_string())
    .bind::<diesel::sql_types::Text, _>(second_workspace_path.to_string())
    .bind::<diesel::sql_types::Text, _>(now.to_string())
    .bind::<diesel::sql_types::Text, _>(now.to_string())
    .bind::<diesel::sql_types::Text, _>(repository_set.repositories_json().to_owned())
    .bind::<diesel::sql_types::Text, _>(workspace_root.to_string())
    .bind::<diesel::sql_types::Text, _>(now.to_string())
    .execute(&mut connection)
    .expect("second legacy workspace should be inserted");
    diesel::sql_query(
        "INSERT INTO repo_worktrees (id, workspace_id, repository_identity, source_path, \
         worktree_path, state, last_head, last_observed_at) \
         VALUES (?, ?, ?, ?, ?, 'attached', ?, ?)",
    )
    .bind::<diesel::sql_types::Text, _>(second_repo_worktree_id.to_string())
    .bind::<diesel::sql_types::Text, _>(second_workspace_id.to_string())
    .bind::<diesel::sql_types::Text, _>(source_path.to_string())
    .bind::<diesel::sql_types::Text, _>(source_path.to_string())
    .bind::<diesel::sql_types::Text, _>(second_worktree_path.to_string())
    .bind::<diesel::sql_types::Text, _>("def456")
    .bind::<diesel::sql_types::Text, _>(now.to_string())
    .execute(&mut connection)
    .expect("second legacy repo worktree should be inserted");

    connection
        .run_pending_migrations(database::MIGRATIONS)
        .expect("pool registry migration should apply");
    let workspace = trees::storage::find_workspace(&mut connection, &workspace_id)
        .expect("workspace should be queryable");
    let pool_id = workspace
        .pool_id
        .expect("automatic workspace should reference a pool");
    let pool = trees::storage::find_workspace_pool_by_id(&mut connection, &pool_id)
        .expect("pool should be queryable");
    assert_eq!(pool.workspace_root, workspace_root);
    assert_eq!(pool.repositories_json, repository_set.repositories_json());
    let origin = trees::storage::find_origin_repository_by_identity(&mut connection, &source_path)
        .expect("origin repository should be queryable")
        .expect("origin repository should exist");
    let links = trees::storage::list_workspace_pool_repositories(&mut connection, &pool_id)
        .expect("pool repository links should be queryable");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].repository_id, origin.id);
    assert_eq!(origin.source_path, source_path);
    assert_eq!(
        trees::storage::find_workspace(&mut connection, &second_workspace_id)
            .expect("second workspace should be queryable")
            .pool_id,
        Some(pool_id)
    );

    connection
        .revert_last_migration(database::MIGRATIONS)
        .expect("claim migration should revert");
    connection
        .revert_last_migration(database::MIGRATIONS)
        .expect("pool registry migration should revert");
    connection
        .run_pending_migrations(database::MIGRATIONS)
        .expect("pool and claim migrations should rerun");

    drop(connection);
    fs::remove_file(path).expect("migration database should be removable");
    fs::remove_dir_all(root).expect("migration test root should be removable");
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
    let workspace_root = CanonicalPath::resolve("/tmp").expect("workspace root should resolve");
    let now = Timestamp::now();
    let pool = trees::storage::ensure_workspace_pool(
        &mut connection,
        &workspace_root,
        &trees::pool::RepositorySetKey::from_repositories(&[CanonicalPath::from_absolute(
            "/repo/api",
        )
        .expect("repository path should be absolute")]),
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
