use diesel::prelude::*;
use snafu::{OptionExt, ResultExt, Snafu};

use crate::domain::{CanonicalPath, CanonicalPathError, WorkspaceId};
use crate::storage::{find_workspace, list_workspaces, WorkspaceRow};

#[derive(Debug, Snafu)]
pub enum TargetError {
    #[snafu(display("failed to resolve status invocation directory: {source}"))]
    Directory { source: CanonicalPathError },
    #[snafu(display("failed to select status workspace: {source}"))]
    Query { source: diesel::result::Error },
    #[snafu(display("unknown workspace: {workspace_id}"))]
    Unknown { workspace_id: WorkspaceId },
}

#[derive(Debug)]
pub enum TargetSelector {
    Id(WorkspaceId),
    Directory(CanonicalPath),
}

impl TargetSelector {
    pub fn resolve(workspace_id: Option<WorkspaceId>) -> Result<Self, TargetError> {
        match workspace_id {
            Some(id) => Ok(Self::Id(id)),
            None => Ok(Self::Directory(
                CanonicalPath::resolve(".").context(DirectorySnafu)?,
            )),
        }
    }

    pub fn select(
        &self,
        connection: Option<&mut SqliteConnection>,
    ) -> Result<Option<WorkspaceRow>, TargetError> {
        match self {
            Self::Id(workspace_id) => {
                let workspace = match connection {
                    Some(connection) => find_workspace(connection, workspace_id)
                        .optional()
                        .context(QuerySnafu)?,
                    None => None,
                };
                Ok(Some(workspace.context(UnknownSnafu {
                    workspace_id: *workspace_id,
                })?))
            }
            Self::Directory(directory) => {
                let Some(connection) = connection else {
                    return Ok(None);
                };
                Ok(list_workspaces(connection, true)
                    .context(QuerySnafu)?
                    .into_iter()
                    .filter(|row| {
                        directory
                            .as_path()
                            .starts_with(row.canonical_path.as_path())
                    })
                    .max_by_key(|row| row.canonical_path.as_path().components().count()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::WorkspaceState;
    use crate::status::tests::insert_workspace;

    #[test]
    fn selects_nearest_registered_boundary_including_removed() {
        let mut db = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let outer = insert_workspace(&mut db, "/work/api", WorkspaceState::Ready);
        let inner = insert_workspace(&mut db, "/work/api/nested", WorkspaceState::Removed);
        for (directory, expected) in [
            ("/work/api/repo/src", Some(outer)),
            ("/work/api/nested/repo", Some(inner)),
            ("/work/api-extra", None),
            ("/elsewhere", None),
        ] {
            let selector =
                TargetSelector::Directory(CanonicalPath::from_absolute(directory).unwrap());
            assert_eq!(
                selector.select(Some(&mut db)).unwrap().map(|row| row.id),
                expected
            );
        }
        assert_eq!(
            TargetSelector::Id(inner)
                .select(Some(&mut db))
                .unwrap()
                .unwrap()
                .id,
            inner
        );
        assert!(matches!(
            TargetSelector::Id(WorkspaceId::new()).select(Some(&mut db)),
            Err(TargetError::Unknown { .. })
        ));
        assert!(matches!(
            TargetSelector::Id(outer).select(None),
            Err(TargetError::Unknown { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn canonicalizes_symlinked_directories() {
        let root = std::env::temp_dir().join(format!("trees-target-{}", WorkspaceId::new()));
        std::fs::create_dir_all(root.join("workspace/repo")).unwrap();
        std::os::unix::fs::symlink(root.join("workspace"), root.join("alias")).unwrap();
        let canonical = CanonicalPath::resolve(root.join("workspace")).unwrap();
        let mut db = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let id = insert_workspace(&mut db, &canonical.to_string(), WorkspaceState::Ready);
        let selector =
            TargetSelector::Directory(CanonicalPath::resolve(root.join("alias/repo")).unwrap());
        assert_eq!(selector.select(Some(&mut db)).unwrap().unwrap().id, id);
        std::fs::remove_dir_all(root).unwrap();
    }
}
