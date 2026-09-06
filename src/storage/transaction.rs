use diesel::result::QueryResult;
use diesel::sqlite::SqliteConnection;
use diesel::Connection;

/// Runs only database metadata work in one short transaction.
///
/// External Git and filesystem operations must run before or after this
/// closure so SQLite does not keep a transaction open while waiting for them.
pub fn with_short_transaction<T, F>(
    connection: &mut SqliteConnection,
    operation: F,
) -> QueryResult<T>
where
    F: FnOnce(&mut SqliteConnection) -> QueryResult<T>,
{
    connection.transaction(operation)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use diesel::prelude::*;

    use super::*;
    use crate::database;
    use crate::domain::{CanonicalPath, Timestamp, WorkspaceId, WorkspaceState};
    use crate::storage::repository::insert_workspace;
    use crate::storage::NewWorkspace;

    #[test]
    fn failed_short_transaction_rolls_back_all_database_work() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let workspace_id = WorkspaceId::new();
        let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
        let now = Timestamp::now();

        let result: Result<(), diesel::result::Error> =
            with_short_transaction(&mut connection, |connection| {
                insert_workspace(
                    connection,
                    &NewWorkspace {
                        id: workspace_id,
                        canonical_path: workspace_path,
                        state: WorkspaceState::Creating,
                        created_at: now.clone(),
                        updated_at: now,
                        last_reconciled_at: None,
                        workspace_root: None,
                    },
                )?;
                Err(diesel::result::Error::RollbackTransaction)
            });

        assert!(result.is_err());
        let count = crate::schema::workspaces::table
            .count()
            .get_result::<i64>(&mut connection)
            .expect("workspace count should be queryable");
        assert_eq!(count, 0);

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }
}
