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

pub fn list(connection: &mut SqliteConnection) -> QueryResult<Vec<OriginRepositoryRow>> {
    origins::table
        .order((origins::source_path.asc(), origins::id.asc()))
        .select(OriginRepositoryRow::as_select())
        .load(connection)
}

pub fn resolve_name(
    connection: &mut SqliteConnection,
    name: &str,
) -> Result<OriginRepositoryRow, NameError> {
    resolve_name_in(&list(connection).context(StorageSnafu)?, name)
}

pub fn resolve_name_in(
    rows: &[OriginRepositoryRow],
    name: &str,
) -> Result<OriginRepositoryRow, NameError> {
    let mut matches = rows.iter().filter(|row| {
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
        let paths = std::iter::once(first)
            .chain(others)
            .map(|row| row.source_path.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return AmbiguousSnafu { name, paths }.fail();
    }
    Ok(first.clone())
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

pub fn pending_by_url(
    connection: &mut SqliteConnection,
    url: &str,
) -> QueryResult<Option<super::models::PendingOriginClone>> {
    use crate::schema::pending_origin_clones as pending;
    pending::table
        .filter(pending::remote_url.eq(url))
        .select(super::models::PendingOriginClone::as_select())
        .first(connection)
        .optional()
}

pub fn insert_pending(
    connection: &mut SqliteConnection,
    value: &super::models::PendingOriginClone,
) -> QueryResult<()> {
    diesel::insert_into(crate::schema::pending_origin_clones::table)
        .values(value)
        .execute(connection)?;
    Ok(())
}

pub fn delete_pending(
    connection: &mut SqliteConnection,
    id: OriginRepositoryId,
) -> QueryResult<()> {
    diesel::delete(crate::schema::pending_origin_clones::table.find(id)).execute(connection)?;
    Ok(())
}

pub fn publish_clone(
    connection: &mut SqliteConnection,
    pending: &super::models::PendingOriginClone,
    identity: &crate::domain::CanonicalPath,
) -> QueryResult<OriginRepositoryRow> {
    crate::storage::with_short_transaction(connection, |connection| {
        diesel::insert_into(origins::table)
            .values((
                origins::id.eq(pending.id),
                origins::source_path.eq(&pending.source_path),
                origins::repository_identity.eq(identity),
            ))
            .execute(connection)?;
        delete_pending(connection, pending.id)?;
        find(connection, pending.id)?.ok_or(diesel::result::Error::NotFound)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::CanonicalPath;

    #[test]
    fn names_preserve_distinct_identities() {
        let mut connection = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let one = CanonicalPath::from_absolute("/origins/one/api").unwrap();
        let two = CanonicalPath::from_absolute("/origins/two/api").unwrap();
        let first = crate::storage::ensure_origin_repository(&mut connection, &one, &one).unwrap();
        assert_eq!(resolve_name(&mut connection, "api").unwrap().id, first.id);
        let second = crate::storage::ensure_origin_repository(&mut connection, &two, &two).unwrap();
        assert!(matches!(
            resolve_name(&mut connection, "api"),
            Err(NameError::Ambiguous { .. })
        ));
        assert!(matches!(
            resolve_name(&mut connection, "API"),
            Err(NameError::Unknown { .. })
        ));
        assert_eq!(
            crate::storage::ensure_origin_repository(&mut connection, &two, &two)
                .unwrap()
                .id,
            second.id
        );
    }
}
