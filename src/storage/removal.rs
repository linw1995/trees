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

pub struct OriginReferences {
    pub worktrees: i64,
    pub pools: i64,
}

impl OriginReferences {
    pub fn any(&self) -> bool {
        self.worktrees != 0 || self.pools != 0
    }
}

pub fn references(
    connection: &mut SqliteConnection,
    id: crate::domain::OriginRepositoryId,
) -> QueryResult<OriginReferences> {
    use crate::schema::{repo_worktrees, workspace_pool_repositories};
    connection.transaction(|connection| {
        Ok(OriginReferences {
            worktrees: repo_worktrees::table
                .filter(repo_worktrees::origin_repository_id.eq(id))
                .count()
                .get_result(connection)?,
            pools: workspace_pool_repositories::table
                .filter(workspace_pool_repositories::repository_id.eq(id))
                .count()
                .get_result(connection)?,
        })
    })
}

pub fn remove_origin(
    connection: &mut SqliteConnection,
    expected: &OriginRepositoryRow,
) -> Result<(), TargetError> {
    let id = expected.id.to_string().parse::<WorkspaceId>()?;
    connection.immediate_transaction(|connection| {
        let target = resolve(connection, id)?;
        let RemovalTarget::Origin(current) = target else {
            return ChangedSnafu { id }.fail();
        };
        snafu::ensure!(
            current.repository_identity == expected.repository_identity
                && current.source_path == expected.source_path,
            ChangedSnafu { id }
        );
        let references = references(connection, current.id).context(StorageSnafu)?;
        snafu::ensure!(
            !references.any(),
            ReferencedSnafu {
                id,
                worktrees: references.worktrees,
                pools: references.pools
            }
        );
        diesel::delete(crate::schema::origin_repositories::table.find(current.id))
            .execute(connection)
            .context(StorageSnafu)?;
        Ok(())
    })
}

#[derive(Debug, Snafu)]
pub enum TargetError {
    #[snafu(display(
        "repository {id} is referenced by {worktrees} worktrees and {pools} pool memberships"
    ))]
    Referenced {
        id: WorkspaceId,
        worktrees: i64,
        pools: i64,
    },
    #[snafu(transparent)]
    Identifier {
        source: crate::domain::IdentifierError,
    },
    #[snafu(display("removal target {id} changed after preflight; retry"))]
    Changed { id: WorkspaceId },
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
    #[test]
    fn rejects_references_added_after_preflight() {
        let mut db = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let path = CanonicalPath::from_absolute("/tmp/source").unwrap();
        let origin = super::super::ensure_origin_repository(&mut db, &path, &path).unwrap();
        assert!(!references(&mut db, origin.id).unwrap().any());
        let key = crate::pool::RepositorySetKey::from_repository_ids(&[origin.id]);
        let pool = super::super::ensure_workspace_pool(&mut db, &key).unwrap();
        super::super::insert_workspace_pool_repositories(
            &mut db,
            &[super::super::NewWorkspacePoolRepository {
                pool_id: pool.id,
                repository_id: origin.id,
            }],
        )
        .unwrap();
        assert!(matches!(
            remove_origin(&mut db, &origin),
            Err(TargetError::Referenced {
                worktrees: 0,
                pools: 1,
                ..
            })
        ));
        assert!(super::super::origin::find(&mut db, origin.id)
            .unwrap()
            .is_some());
    }
}
