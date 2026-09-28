use diesel::prelude::*;
use diesel::sql_types::Text;
use diesel_migrations::MigrationHarness;

#[derive(QueryableByName)]
struct ColumnName {
    #[diesel(sql_type = Text)]
    name: String,
}

fn has_cache_column(connection: &mut SqliteConnection, table: &str) -> bool {
    diesel::sql_query(format!("PRAGMA table_info({table})"))
        .load::<ColumnName>(connection)
        .unwrap()
        .iter()
        .any(|column| column.name == "disk_usage_json")
}

#[test]
fn migration_is_reversible_for_all_three_tables() {
    let mut connection = trees::database::connect(std::path::Path::new(":memory:")).unwrap();
    for table in ["workspaces", "repo_worktrees", "origin_repositories"] {
        assert!(has_cache_column(&mut connection, table));
    }
    connection
        .revert_last_migration(trees::database::MIGRATIONS)
        .unwrap();
    connection
        .revert_last_migration(trees::database::MIGRATIONS)
        .unwrap();
    for table in ["workspaces", "repo_worktrees", "origin_repositories"] {
        assert!(!has_cache_column(&mut connection, table));
    }
    connection
        .run_pending_migrations(trees::database::MIGRATIONS)
        .unwrap();
    for table in ["workspaces", "repo_worktrees", "origin_repositories"] {
        assert!(has_cache_column(&mut connection, table));
    }
}
