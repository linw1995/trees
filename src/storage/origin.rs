use diesel::prelude::*;
use diesel::result::QueryResult;
use snafu::{ResultExt, Snafu};

use crate::domain::OriginRepositoryId;
use crate::schema::origin_repositories as origins;
use crate::storage::OriginRepositoryRow;

pub fn find(
    connection: &mut SqliteConnection,
    id: OriginRepositoryId,
) -> QueryResult<Option<OriginRepositoryRow>> {
    origins::table
        .find(id)
        .select(OriginRepositoryRow::as_select())
        .first(connection)
        .optional()
}

pub fn find_by_url(
    connection: &mut SqliteConnection,
    url: &str,
) -> QueryResult<Option<OriginRepositoryRow>> {
    origins::table
        .filter(origins::remote_url.eq(url))
        .select(OriginRepositoryRow::as_select())
        .first(connection)
        .optional()
}

pub fn list(connection: &mut SqliteConnection, all: bool) -> QueryResult<Vec<OriginRepositoryRow>> {
    let mut query = origins::table.into_boxed();
    if !all {
        query = query.filter(origins::registered.eq(true));
    }
    query
        .order((origins::source_path.asc(), origins::id.asc()))
        .select(OriginRepositoryRow::as_select())
        .load(connection)
}

pub fn resolve_name(
    connection: &mut SqliteConnection,
    name: &str,
) -> Result<OriginRepositoryRow, NameError> {
    let mut matches = list(connection, false)
        .context(StorageSnafu)?
        .into_iter()
        .filter(|row| {
            row.source_path
                .as_path()
                .file_name()
                .is_some_and(|part| part == name)
        });
    let Some(first) = matches.next() else {
        return UnknownSnafu { name }.fail();
    };
    let others: Vec<_> = matches.collect();
    if !others.is_empty() {
        let paths = std::iter::once(&first)
            .chain(others.iter())
            .map(|row| row.source_path.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return AmbiguousSnafu { name, paths }.fail();
    }
    Ok(first)
}

pub fn set_registered(
    connection: &mut SqliteConnection,
    id: OriginRepositoryId,
    registered: bool,
) -> QueryResult<usize> {
    diesel::update(origins::table.find(id))
        .set(origins::registered.eq(registered))
        .execute(connection)
}

#[derive(Debug, Snafu)]
pub enum NameError {
    #[snafu(display(
        "no registered repository named {name:?}; use a repository path or remote URL"
    ))]
    Unknown { name: String },
    #[snafu(display("repository name {name:?} is ambiguous; use an explicit path: {paths}"))]
    Ambiguous { name: String, paths: String },
    #[snafu(display("failed to look up repository name: {source}"))]
    Storage { source: diesel::result::Error },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CanonicalPath, RepositoryManagementMode};

    #[test]
    fn names_preserve_distinct_identities_and_registration() {
        let mut connection = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let one = CanonicalPath::from_absolute("/origins/one/api").unwrap();
        let two = CanonicalPath::from_absolute("/origins/two/api").unwrap();
        let first = crate::storage::ensure_origin_repository(&mut connection, &one, &one).unwrap();
        assert_eq!(first.management_mode, RepositoryManagementMode::Manual);
        assert_eq!(resolve_name(&mut connection, "api").unwrap().id, first.id);
        let second = crate::storage::ensure_origin_repository(&mut connection, &two, &two).unwrap();
        assert!(matches!(
            resolve_name(&mut connection, "api"),
            Err(NameError::Ambiguous { .. })
        ));
        set_registered(&mut connection, second.id, false).unwrap();
        assert_eq!(resolve_name(&mut connection, "api").unwrap().id, first.id);
        assert!(matches!(
            resolve_name(&mut connection, "API"),
            Err(NameError::Unknown { .. })
        ));
        let repeated =
            crate::storage::ensure_origin_repository(&mut connection, &two, &two).unwrap();
        assert_eq!(repeated.id, second.id);
        assert!(repeated.registered);
    }
    #[test]
    fn path_registration_preserves_automatic_url_and_root() {
        let mut connection = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let path = CanonicalPath::from_absolute("/origins/id/api").unwrap();
        let root = CanonicalPath::from_absolute("/origins").unwrap();
        let row = crate::storage::ensure_origin_repository(&mut connection, &path, &path).unwrap();
        diesel::update(origins::table.find(row.id))
            .set((
                origins::management_mode.eq(RepositoryManagementMode::Automatic),
                origins::managed_root.eq(Some(&root)),
                origins::remote_url.eq(Some("https://example.com/api.git")),
            ))
            .execute(&mut connection)
            .unwrap();
        set_registered(&mut connection, row.id, false).unwrap();
        let registered =
            crate::storage::ensure_origin_repository(&mut connection, &path, &path).unwrap();
        assert_eq!(
            registered.management_mode,
            RepositoryManagementMode::Automatic
        );
        assert_eq!(registered.managed_root, Some(root));
        assert_eq!(
            find_by_url(&mut connection, "https://example.com/api.git")
                .unwrap()
                .unwrap()
                .id,
            row.id
        );
        assert!(registered.registered);
    }
}
