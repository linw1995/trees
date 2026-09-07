use diesel::result::QueryResult;
use diesel::sqlite::SqliteConnection;
use std::thread;
use std::time::Duration;

const SQLITE_RETRY_ATTEMPTS: usize = 5;
const SQLITE_RETRY_DELAY: Duration = Duration::from_millis(10);

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
    connection.immediate_transaction(operation)
}

/// Attempts one immediate transaction without waiting for another SQLite writer.
pub fn with_trying_short_transaction<T, F>(
    connection: &mut SqliteConnection,
    operation: F,
) -> QueryResult<T>
where
    F: FnOnce(&mut SqliteConnection) -> QueryResult<T>,
{
    crate::database::use_try_busy_timeout(connection)?;
    let result = connection.immediate_transaction(operation);
    crate::database::restore_busy_timeout(connection)?;
    result
}

/// Runs immediate metadata work with bounded retries for transient SQLite busy errors.
pub fn with_immediate_transaction<T, F>(
    connection: &mut SqliteConnection,
    mut operation: F,
) -> QueryResult<T>
where
    F: FnMut(&mut SqliteConnection) -> QueryResult<T>,
{
    retry_sqlite_transaction(connection, |connection| {
        connection.immediate_transaction(&mut operation)
    })
}

/// Runs a deferred metadata transaction with bounded retries for transient SQLite busy errors.
pub fn with_retrying_short_transaction<T, F>(
    connection: &mut SqliteConnection,
    mut operation: F,
) -> QueryResult<T>
where
    F: FnMut(&mut SqliteConnection) -> QueryResult<T>,
{
    retry_sqlite_transaction(connection, |connection| {
        connection.immediate_transaction(&mut operation)
    })
}

fn retry_sqlite_transaction<T, F>(
    connection: &mut SqliteConnection,
    mut transaction: F,
) -> QueryResult<T>
where
    F: FnMut(&mut SqliteConnection) -> QueryResult<T>,
{
    for attempt in 0..=SQLITE_RETRY_ATTEMPTS {
        match transaction(connection) {
            Ok(value) => return Ok(value),
            Err(error) if attempt < SQLITE_RETRY_ATTEMPTS && is_sqlite_busy(&error) => {
                thread::sleep(SQLITE_RETRY_DELAY * 2_u32.pow(attempt as u32));
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("SQLite transaction retry loop must return from every attempt")
}

fn is_sqlite_busy(error: &diesel::result::Error) -> bool {
    matches!(
        error,
        diesel::result::Error::DatabaseError(_, information)
            if information.message().contains("locked")
                || information.message().contains("busy")
    )
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

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

    #[test]
    fn trying_short_transaction_exits_when_database_is_locked() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut holder = database::connect(&database_path).expect("database should open");
        let mut contender = database::connect(&database_path).expect("database should open");
        holder
            .immediate_transaction::<_, diesel::result::Error, _>(|_| {
                let started_at = Instant::now();
                let result = with_trying_short_transaction(&mut contender, |_| {
                    Ok::<_, diesel::result::Error>(())
                });

                assert!(result.is_err());
                assert!(started_at.elapsed() < Duration::from_secs(1));
                Ok(())
            })
            .expect("holder transaction should complete");
        drop(contender);
        drop(holder);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }
}
