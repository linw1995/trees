use std::path::{Path, PathBuf};

use diesel::connection::SimpleConnection;
use diesel::sqlite::SqliteConnection;
use diesel::Connection;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use snafu::{OptionExt, ResultExt, Snafu};

use crate::paths::{self, PathError, StateDirectoryError};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

const CONNECTION_PRAGMAS: &str =
    "PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;";
const CONNECTION_BUSY_TIMEOUT_PRAGMA: &str = "PRAGMA busy_timeout = 5000;";
const TRY_BUSY_TIMEOUT_PRAGMA: &str = "PRAGMA busy_timeout = 0;";

pub(crate) fn use_try_busy_timeout(
    connection: &mut SqliteConnection,
) -> Result<(), diesel::result::Error> {
    connection.batch_execute(TRY_BUSY_TIMEOUT_PRAGMA)
}

pub(crate) fn restore_busy_timeout(
    connection: &mut SqliteConnection,
) -> Result<(), diesel::result::Error> {
    connection.batch_execute(CONNECTION_BUSY_TIMEOUT_PRAGMA)
}

pub fn open_default() -> Result<SqliteConnection, DatabaseError> {
    let path = paths::database_path()?;
    paths::ensure_state_directory()?;
    connect(&path)
}

pub fn open_read_only() -> Result<SqliteConnection, DatabaseError> {
    let path = paths::database_path()?;
    if !path.exists() {
        return ReadOnlyDatabaseMissingSnafu { path }.fail();
    }
    connect_read_only(&path)
}

pub fn connect_read_only(path: &Path) -> Result<SqliteConnection, DatabaseError> {
    if !path.exists() {
        return ReadOnlyDatabaseMissingSnafu { path }.fail();
    }
    let path_text = path.to_str().context(PathNotUtf8Snafu { path })?;
    let database_url = format!("sqlite://{path_text}?mode=ro");
    SqliteConnection::establish(&database_url).context(ConnectionSnafu)
}

pub fn connect(path: &Path) -> Result<SqliteConnection, DatabaseError> {
    let path_text = path.to_str().context(PathNotUtf8Snafu { path })?;
    let mut connection = SqliteConnection::establish(path_text).context(ConnectionSnafu)?;

    connection
        .batch_execute(CONNECTION_PRAGMAS)
        .context(ConfigurationSnafu)?;
    connection
        .run_pending_migrations(MIGRATIONS)
        .context(MigrationSnafu)?;

    Ok(connection)
}

#[derive(Debug, Snafu)]
pub enum DatabaseError {
    #[snafu(transparent)]
    Path { source: PathError },
    #[snafu(transparent)]
    StateDirectory { source: StateDirectoryError },
    #[snafu(display("database path is not valid UTF-8: {}", path.display()))]
    PathNotUtf8 { path: PathBuf },
    #[snafu(display("read-only lifecycle database does not exist: {}", path.display()))]
    ReadOnlyDatabaseMissing { path: PathBuf },
    #[snafu(display("failed to open SQLite database: {source}"))]
    Connection { source: diesel::ConnectionError },
    #[snafu(display("failed to configure SQLite connection: {source}"))]
    Configuration { source: diesel::result::Error },
    #[snafu(display("failed to apply database migrations: {source}"))]
    Migration {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use diesel::prelude::*;

    use super::*;
    use crate::domain::WorkspaceId;

    fn temporary_database_path() -> PathBuf {
        std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()))
    }

    #[test]
    fn connection_applies_migrations() {
        let path = temporary_database_path();
        let mut connection = connect(&path).expect("database should open");
        let count = crate::schema::workspaces::table
            .count()
            .get_result::<i64>(&mut connection)
            .expect("migrated table should be queryable");

        assert_eq!(count, 0);
        drop(connection);
        fs::remove_file(path).expect("temporary database should be removable");
    }

    #[test]
    fn formats_database_errors() {
        let path = PathBuf::from("/tmp/trees.sqlite");
        let errors = [
            DatabaseError::Path {
                source: PathError::HomeDirectoryUnavailable,
            },
            DatabaseError::StateDirectory {
                source: StateDirectoryError::new(
                    path.clone(),
                    std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
                ),
            },
            DatabaseError::PathNotUtf8 { path: path.clone() },
            DatabaseError::ReadOnlyDatabaseMissing { path: path.clone() },
            DatabaseError::Connection {
                source: diesel::ConnectionError::InvalidConnectionUrl("invalid".to_owned()),
            },
            DatabaseError::Configuration {
                source: diesel::result::Error::NotFound,
            },
            DatabaseError::Migration {
                source: Box::new(std::io::Error::other("migration failed")),
            },
        ];

        for error in errors {
            assert!(!error.to_string().is_empty());
            let _ = std::error::Error::source(&error);
        }
    }

    #[test]
    fn read_only_connection_rejects_database_writes() {
        let path = temporary_database_path();
        let connection = connect(&path).expect("database should open");
        drop(connection);

        let mut connection = connect_read_only(&path).expect("read-only database should open");
        let workspace_id = WorkspaceId::new();
        let now = crate::domain::Timestamp::now();
        let workspace_path = crate::domain::CanonicalPath::resolve("Cargo.toml")
            .expect("workspace path should resolve");
        assert!(diesel::insert_into(crate::schema::workspaces::table)
            .values(&crate::storage::NewWorkspace {
                id: workspace_id,
                canonical_path: workspace_path,
                state: crate::domain::WorkspaceState::Creating,
                created_at: now.clone(),
                updated_at: now,
                last_reconciled_at: None,
            })
            .execute(&mut connection)
            .is_err());

        drop(connection);
        fs::remove_file(path).expect("temporary database should be removable");
    }
}
