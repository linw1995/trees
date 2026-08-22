use std::fmt;
use std::path::{Path, PathBuf};

use diesel::connection::SimpleConnection;
use diesel::sqlite::SqliteConnection;
use diesel::Connection;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

use crate::paths::{self, PathError, StateDirectoryError};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

const CONNECTION_PRAGMAS: &str =
    "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;";

pub fn open_default() -> Result<SqliteConnection, DatabaseError> {
    let path = paths::database_path().map_err(DatabaseError::Path)?;
    paths::ensure_state_directory().map_err(DatabaseError::StateDirectory)?;
    connect(&path)
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
}
