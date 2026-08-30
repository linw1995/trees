use std::fmt;
use std::path::{Path, PathBuf};

use diesel::connection::SimpleConnection;
use diesel::sqlite::SqliteConnection;
use diesel::Connection;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

use crate::paths::{self, PathError, StateDirectoryError};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

const CONNECTION_PRAGMAS: &str =
    "PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;";

pub fn open_default() -> Result<SqliteConnection, DatabaseError> {
    let path = paths::database_path().map_err(DatabaseError::Path)?;
    paths::ensure_state_directory().map_err(DatabaseError::StateDirectory)?;
    connect(&path)
}

pub fn open_read_only() -> Result<SqliteConnection, DatabaseError> {
    let path = paths::database_path().map_err(DatabaseError::Path)?;
    if !path.exists() {
        return Err(DatabaseError::ReadOnlyDatabaseMissing(path));
    }
    connect_read_only(&path)
}

pub fn connect_read_only(path: &Path) -> Result<SqliteConnection, DatabaseError> {
    if !path.exists() {
        return Err(DatabaseError::ReadOnlyDatabaseMissing(path.to_owned()));
    }
    let path_text = path
        .to_str()
        .ok_or_else(|| DatabaseError::PathNotUtf8(path.to_owned()))?;
    let database_url = format!("sqlite://{path_text}?mode=ro");
    SqliteConnection::establish(&database_url).map_err(DatabaseError::Connection)
}

pub fn connect(path: &Path) -> Result<SqliteConnection, DatabaseError> {
    let path_text = path
        .to_str()
        .ok_or_else(|| DatabaseError::PathNotUtf8(path.to_owned()))?;
    let mut connection =
        SqliteConnection::establish(path_text).map_err(DatabaseError::Connection)?;

    connection
        .batch_execute(CONNECTION_PRAGMAS)
        .map_err(DatabaseError::Configuration)?;
    connection
        .run_pending_migrations(MIGRATIONS)
        .map_err(DatabaseError::Migration)?;

    Ok(connection)
}

#[derive(Debug)]
pub enum DatabaseError {
    Path(PathError),
    StateDirectory(StateDirectoryError),
    PathNotUtf8(PathBuf),
    ReadOnlyDatabaseMissing(PathBuf),
    Connection(diesel::ConnectionError),
    Configuration(diesel::result::Error),
    Migration(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(error) => error.fmt(formatter),
            Self::StateDirectory(error) => error.fmt(formatter),
            Self::PathNotUtf8(path) => {
                write!(
                    formatter,
                    "database path is not valid UTF-8: {}",
                    path.display()
                )
            }
            Self::ReadOnlyDatabaseMissing(path) => {
                write!(
                    formatter,
                    "read-only lifecycle database does not exist: {}",
                    path.display()
                )
            }
            Self::Connection(error) => write!(formatter, "failed to open SQLite database: {error}"),
            Self::Configuration(error) => {
                write!(formatter, "failed to configure SQLite connection: {error}")
            }
            Self::Migration(error) => {
                write!(formatter, "failed to apply database migrations: {error}")
            }
        }
    }
}

impl std::error::Error for DatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Path(error) => Some(error),
            Self::StateDirectory(error) => Some(error),
            Self::PathNotUtf8(_) => None,
            Self::ReadOnlyDatabaseMissing(_) => None,
            Self::Connection(error) => Some(error),
            Self::Configuration(error) => Some(error),
            Self::Migration(error) => Some(&**error),
        }
    }
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
            DatabaseError::Path(PathError::HomeDirectoryUnavailable),
            DatabaseError::StateDirectory(StateDirectoryError::new(
                path.clone(),
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
            )),
            DatabaseError::PathNotUtf8(path.clone()),
            DatabaseError::ReadOnlyDatabaseMissing(path.clone()),
            DatabaseError::Connection(diesel::ConnectionError::InvalidConnectionUrl(
                "invalid".to_owned(),
            )),
            DatabaseError::Configuration(diesel::result::Error::NotFound),
            DatabaseError::Migration(Box::new(std::io::Error::other("migration failed"))),
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
