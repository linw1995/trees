#[cfg(test)]
mod origin_migration_tests {
    use diesel::connection::SimpleConnection;
    use diesel::prelude::*;

    #[test]
    fn origin_upgrade_and_rollback_preserve_identity() {
        let mut connection = SqliteConnection::establish(":memory:").unwrap();
        connection
            .batch_execute(include_str!(
                "../migrations/00000000000001_create_lifecycle_tables/up.sql"
            ))
            .unwrap();
        connection
            .batch_execute(include_str!(
                "../migrations/00000000000002_workspace_reuse/up.sql"
            ))
            .unwrap();
        let id = trees::domain::OriginRepositoryId::new();
        diesel::insert_into(trees::schema::origin_repositories::table)
            .values(trees::storage::NewOriginRepository {
                id,
                repository_identity: trees::domain::CanonicalPath::from_absolute(
                    "/tmp/origin/.git",
                )
                .unwrap(),
                source_path: trees::domain::CanonicalPath::from_absolute("/tmp/origin").unwrap(),
            })
            .execute(&mut connection)
            .unwrap();
        connection
            .batch_execute(include_str!(
                "../migrations/00000000000003_origin_management/up.sql"
            ))
            .unwrap();
        let row = trees::schema::origin_repositories::table
            .select(trees::storage::OriginRepositoryRow::as_select())
            .first(&mut connection)
            .unwrap();
        assert_eq!(row.id, id);
        #[derive(diesel::QueryableByName)]
        struct Column {
            #[diesel(sql_type = diesel::sql_types::Text)]
            name: String,
        }
        let columns = diesel::sql_query("PRAGMA table_info(origin_repositories)")
            .load::<Column>(&mut connection)
            .unwrap();
        assert_eq!(
            columns
                .into_iter()
                .map(|column| column.name)
                .collect::<Vec<_>>(),
            ["id", "repository_identity", "source_path"]
        );
        connection
            .batch_execute(include_str!(
                "../migrations/00000000000003_origin_management/down.sql"
            ))
            .unwrap();
        let retained: trees::domain::OriginRepositoryId = trees::schema::origin_repositories::table
            .select(trees::schema::origin_repositories::id)
            .first(&mut connection)
            .unwrap();
        assert_eq!(retained, id);
    }
}

#[test]
fn rollback_requires_pending_clone_recovery() {
    use diesel_migrations::MigrationHarness;
    use trees::domain::{CanonicalPath, OriginRepositoryId};
    let mut db = trees::database::connect(std::path::Path::new(":memory:")).unwrap();
    db.revert_last_migration(trees::database::MIGRATIONS)
        .expect("terminology migration should revert");
    let id = OriginRepositoryId::new();
    trees::storage::origin::insert_pending(
        &mut db,
        &trees::storage::models::PendingOriginClone {
            id,
            remote_url: "https://example.com/api.git".to_owned(),
            managed_root: CanonicalPath::from_absolute("/tmp/origins").unwrap(),
            source_path: CanonicalPath::from_absolute(format!("/tmp/origins/{id}/api")).unwrap(),
            ownership_token: OriginRepositoryId::new().to_string(),
        },
    )
    .unwrap();
    assert!(db
        .revert_last_migration(trees::database::MIGRATIONS)
        .is_err());
    assert!(
        trees::storage::origin::pending_by_url(&mut db, "https://example.com/api.git")
            .unwrap()
            .is_some()
    );
    trees::storage::origin::delete_pending(&mut db, id).unwrap();
    db.revert_last_migration(trees::database::MIGRATIONS)
        .unwrap();
}
