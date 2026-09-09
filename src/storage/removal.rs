use diesel::prelude::*;
use snafu::{ResultExt, Snafu};

use super::{OriginRepositoryRow, WorkspaceRow};
use crate::domain::WorkspaceId;

#[derive(Debug)]
pub enum RemovalTarget {
    Workspace(WorkspaceRow),
    Origin(OriginRepositoryRow),
}

pub fn resolve(
    connection: &mut SqliteConnection,
    id: WorkspaceId,
) -> Result<RemovalTarget, TargetError> {
    connection.transaction(|connection| {
        let workspace = super::find_workspace(connection, &id)
            .optional()
            .context(StorageSnafu)?;
        let origin = crate::schema::origin_repositories::table
            .filter(crate::schema::origin_repositories::id.eq(id.to_string()))
            .select(OriginRepositoryRow::as_select())
            .first(connection)
            .optional()
            .context(StorageSnafu)?;
        match (workspace, origin) {
            (Some(workspace), None) => Ok(RemovalTarget::Workspace(workspace)),
            (None, Some(origin)) => Ok(RemovalTarget::Origin(origin)),
            (None, None) => UnknownSnafu { id }.fail(),
            (Some(_), Some(_)) => AmbiguousSnafu { id }.fail(),
        }
    })
}

#[derive(Debug, Snafu)]
pub enum TargetError {
    #[snafu(display("no workspace or repository with ID {id}"))]
    Unknown { id: WorkspaceId },
    #[snafu(display(
        "ID {id} matches both a workspace and a repository; refusing ambiguous removal"
    ))]
    Ambiguous { id: WorkspaceId },
    #[snafu(display("failed to resolve removal target: {source}"))]
    Storage { source: diesel::result::Error },
}

impl From<diesel::result::Error> for TargetError {
    fn from(source: diesel::result::Error) -> Self {
        use snafu::IntoError;
        StorageSnafu.into_error(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CanonicalPath, Timestamp, WorkspaceState};

    #[test]
    fn resolves_entity_type_and_rejects_cross_table_collision() {
        let mut db = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let path = CanonicalPath::from_absolute("/tmp/origin").unwrap();
        let origin = super::super::ensure_origin_repository(&mut db, &path, &path).unwrap();
        let id = origin.id.to_string().parse::<WorkspaceId>().unwrap();
        assert!(matches!(
            resolve(&mut db, id).unwrap(),
            RemovalTarget::Origin(_)
        ));
        assert!(matches!(
            resolve(&mut db, WorkspaceId::new()),
            Err(TargetError::Unknown { .. })
        ));
        super::super::insert_workspace(
            &mut db,
            &super::super::NewWorkspace {
                id,
                canonical_path: path,
                state: WorkspaceState::Ready,
                created_at: Timestamp::now(),
                updated_at: Timestamp::now(),
                last_reconciled_at: None,
            },
        )
        .unwrap();
        assert!(matches!(
            resolve(&mut db, id),
            Err(TargetError::Ambiguous { .. })
        ));
    }
}
