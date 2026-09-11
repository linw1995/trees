use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel_migrations::MigrationHarness;
use trees::database;
use trees::domain::{WorkspaceId, WorkspaceState};

#[derive(Debug, QueryableByName, PartialEq)]
struct StoredRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    state: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    timestamp: String,
}

#[test]
fn removal_upgrade_and_rollback_preserve_records_and_history() {
    let mut db = database::connect(std::path::Path::new(":memory:")).unwrap();
    db.revert_last_migration(database::MIGRATIONS).unwrap();
    let workspace_id = WorkspaceId::new();
    let repo_id = WorkspaceId::new();
    let worktree_id = WorkspaceId::new();
    let operation_id = WorkspaceId::new();
    let event_id = WorkspaceId::new();
    let claim_id = WorkspaceId::new();
    let lease_id = WorkspaceId::new();
    db.batch_execute(&format!(
        "INSERT INTO workspaces (id, canonical_path, state, created_at, updated_at, reclaimed_at)
         VALUES ('{workspace_id}', '/workspace', 'reclaimed', '2026-09-01T00:00:00Z',
                 '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z');
         INSERT INTO origin_repositories VALUES ('{repo_id}', '/origin/.git', '/origin');
         INSERT INTO repo_worktrees VALUES ('{worktree_id}', '{workspace_id}', '{repo_id}',
                 '/workspace/repo', 'reclaimed', 'abc123', '2026-09-02T00:00:00Z');
         INSERT INTO operations VALUES ('{operation_id}', '{workspace_id}', 'gc',
                 '2026-09-02T00:00:00Z', '{{}}');
         INSERT INTO lifecycle_events VALUES ('{event_id}', '{operation_id}', 'workspace',
                 '{workspace_id}', 'workspace_reclaimed', 'trees', '2026-09-02T00:00:00Z',
                 'ready', 'reclaimed', NULL, NULL);
         INSERT INTO workspace_claims VALUES ('{claim_id}', '{workspace_id}', '2026-09-01T00:00:00Z');
         INSERT INTO operation_leases VALUES ('{lease_id}', '{operation_id}', '{workspace_id}',
                 '2026-09-02T00:00:00Z');"
    ))
    .unwrap();

    for _ in 0..2 {
        db.run_pending_migrations(database::MIGRATIONS).unwrap();
        let workspace = trees::storage::find_workspace(&mut db, &workspace_id).unwrap();
        assert_eq!(workspace.state, WorkspaceState::Removed);
        assert_eq!(
            workspace.removed_at.unwrap().to_string(),
            "2026-09-02T00:00:00Z"
        );
        let worktrees = trees::storage::list_repo_worktrees(&mut db, &workspace_id).unwrap();
        assert_eq!(worktrees.len(), 1);
        assert_eq!(
            worktrees[0].state,
            trees::domain::RepoWorktreeState::Removed
        );
        assert_eq!(worktrees[0].last_head.as_deref(), Some("abc123"));
        let event = trees::schema::lifecycle_events::table
            .select(trees::storage::EventRow::as_select())
            .first(&mut db)
            .unwrap();
        assert_eq!(event.event_type, "workspace_reclaimed");
        assert_eq!(event.current_state.as_deref(), Some("reclaimed"));
        assert!(db
            .batch_execute("UPDATE lifecycle_events SET event_type = 'changed'")
            .is_err());
        assert!(db.batch_execute("DELETE FROM operations").is_err());
        assert!(db
            .batch_execute("UPDATE workspaces SET state = 'reclaimed'")
            .is_err());
        assert!(db
            .batch_execute("UPDATE repo_worktrees SET state = 'reclaimed'")
            .is_err());
        assert_eq!(
            trees::schema::workspace_claims::table
                .count()
                .get_result::<i64>(&mut db)
                .unwrap(),
            1
        );
        assert_eq!(
            trees::schema::operation_leases::table
                .count()
                .get_result::<i64>(&mut db)
                .unwrap(),
            1
        );
        #[derive(QueryableByName)]
        struct ForeignKeyViolation {
            #[diesel(sql_type = diesel::sql_types::Text)]
            #[diesel(column_name = "table")]
            _table: String,
        }
        assert!(diesel::sql_query("PRAGMA foreign_key_check")
            .load::<ForeignKeyViolation>(&mut db)
            .unwrap()
            .is_empty());
        db.revert_last_migration(database::MIGRATIONS).unwrap();
        let row = diesel::sql_query("SELECT state, reclaimed_at AS timestamp FROM workspaces")
            .get_result::<StoredRow>(&mut db)
            .unwrap();
        assert_eq!(row.state, "reclaimed");
        assert_eq!(row.timestamp, "2026-09-02T00:00:00Z");
    }
}

#[test]
fn read_only_access_requires_upgrade_without_migrating() {
    let path = std::env::temp_dir().join(format!(
        "trees-removal-migration-{}.sqlite",
        WorkspaceId::new()
    ));
    let mut db = database::connect(&path).unwrap();
    db.revert_last_migration(database::MIGRATIONS).unwrap();
    drop(db);
    assert!(matches!(
        database::connect_read_only(&path),
        Err(database::DatabaseError::SchemaUpgradeRequired { .. })
    ));
    let mut raw = SqliteConnection::establish(path.to_str().unwrap()).unwrap();
    assert!(raw.has_pending_migration(database::MIGRATIONS).unwrap());
    drop(raw);
    drop(database::connect(&path).unwrap());
    assert!(database::connect_read_only(&path).is_ok());
    std::fs::remove_file(path).unwrap();
}
