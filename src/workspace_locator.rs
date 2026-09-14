use diesel::{OptionalExtension, SqliteConnection};
use snafu::{ResultExt, Snafu};

use crate::domain::{CanonicalPath, ClaimId, WorkspaceId};
use crate::storage::{
    find_workspace, find_workspace_by_path, find_workspace_claim_by_id, WorkspaceRow,
};

#[derive(Debug, Clone)]
pub enum WorkspaceSelector {
    Id(WorkspaceId),
    ExactPath(CanonicalPath),
    ContainingDirectory(CanonicalPath),
    ClaimId(ClaimId),
}

#[derive(Debug)]
pub struct LocatedWorkspace {
    pub workspace: WorkspaceRow,
    pub selected_claim_id: Option<ClaimId>,
}

#[derive(Debug, Snafu)]
pub enum LocateError {
    #[snafu(display("failed to locate workspace: {source}"))]
    Database { source: diesel::result::Error },
    #[snafu(display("claim {claim_id} references missing workspace {workspace_id}: {source}"))]
    DanglingClaim {
        claim_id: ClaimId,
        workspace_id: WorkspaceId,
        source: diesel::result::Error,
    },
}

/// Reads within the caller's transaction without applying lifecycle eligibility.
pub fn locate(
    connection: &mut SqliteConnection,
    selector: &WorkspaceSelector,
) -> Result<Option<LocatedWorkspace>, LocateError> {
    let workspace = match selector {
        WorkspaceSelector::Id(id) => find_workspace(connection, id)
            .optional()
            .context(DatabaseSnafu)?,
        WorkspaceSelector::ExactPath(path) => {
            find_workspace_by_path(connection, path).context(DatabaseSnafu)?
        }
        WorkspaceSelector::ContainingDirectory(directory) => {
            let mut nearest = None;
            for ancestor in directory.as_path().ancestors() {
                let path = CanonicalPath::from_absolute(ancestor)
                    .expect("an ancestor of a canonical path is absolute");
                if let Some(workspace) =
                    find_workspace_by_path(connection, &path).context(DatabaseSnafu)?
                {
                    nearest = Some(workspace);
                    break;
                }
            }
            nearest
        }
        WorkspaceSelector::ClaimId(claim_id) => {
            let Some(claim) = find_workspace_claim_by_id(connection, claim_id)
                .optional()
                .context(DatabaseSnafu)?
            else {
                return Ok(None);
            };
            let workspace = match find_workspace(connection, &claim.workspace_id) {
                Ok(workspace) => workspace,
                Err(source @ diesel::result::Error::NotFound) => {
                    return Err(LocateError::DanglingClaim {
                        claim_id: *claim_id,
                        workspace_id: claim.workspace_id,
                        source,
                    });
                }
                result => result.context(DatabaseSnafu)?,
            };
            return Ok(Some(LocatedWorkspace {
                workspace,
                selected_claim_id: Some(*claim_id),
            }));
        }
    };
    Ok(workspace.map(|workspace| LocatedWorkspace {
        workspace,
        selected_claim_id: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim::WorkspaceClaim;
    use crate::domain::{Timestamp, WorkspaceState};
    use crate::storage::{
        insert_workspace, insert_workspace_claim, NewWorkspace, NewWorkspaceClaim,
    };

    fn path(value: &str) -> CanonicalPath {
        CanonicalPath::from_absolute(value).unwrap()
    }

    fn insert(db: &mut SqliteConnection, directory: &str) -> WorkspaceId {
        let id = WorkspaceId::new();
        insert_workspace(
            db,
            &NewWorkspace {
                id,
                canonical_path: path(directory),
                state: WorkspaceState::Removed,
                created_at: Timestamp::now(),
                updated_at: Timestamp::now(),
                last_reconciled_at: None,
            },
        )
        .unwrap();
        id
    }

    #[test]
    fn distinguishes_exact_paths_and_nearest_registered_boundaries() {
        let mut db = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let outer = insert(&mut db, "/work/api");
        let inner = insert(&mut db, "/work/api/nested");
        for (directory, expected) in [
            ("/work/api", Some(outer)),
            ("/work/api/repo/src", Some(outer)),
            ("/work/api/nested/repo", Some(inner)),
            ("/work/api-extra", None),
            ("/elsewhere", None),
        ] {
            let result = locate(
                &mut db,
                &WorkspaceSelector::ContainingDirectory(path(directory)),
            )
            .unwrap();
            assert_eq!(result.map(|result| result.workspace.id), expected);
        }
        assert!(locate(
            &mut db,
            &WorkspaceSelector::ExactPath(path("/work/api/repo"))
        )
        .unwrap()
        .is_none());
        assert_eq!(
            locate(&mut db, &WorkspaceSelector::ExactPath(path("/work/api")))
                .unwrap()
                .unwrap()
                .workspace
                .id,
            outer
        );
    }

    #[test]
    fn preserves_claim_provenance_and_distinguishes_missing_targets() {
        let mut db = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let id = insert(&mut db, "/missing/workspace");
        let claim = WorkspaceClaim::new(id);
        insert_workspace_claim(&mut db, &NewWorkspaceClaim::from(&claim)).unwrap();
        let selected = locate(&mut db, &WorkspaceSelector::ClaimId(claim.id))
            .unwrap()
            .unwrap();
        assert_eq!(selected.workspace.id, id);
        assert_eq!(selected.selected_claim_id, Some(claim.id));
        let selected = locate(&mut db, &WorkspaceSelector::Id(id))
            .unwrap()
            .unwrap();
        assert_eq!(selected.workspace.id, id);
        assert_eq!(selected.selected_claim_id, None);
        for selector in [
            WorkspaceSelector::Id(WorkspaceId::new()),
            WorkspaceSelector::ClaimId(ClaimId::new()),
        ] {
            assert!(locate(&mut db, &selector).unwrap().is_none());
        }
    }

    #[test]
    fn preserves_query_errors() {
        use diesel::Connection;
        let mut db = SqliteConnection::establish(":memory:").unwrap();
        let error = locate(&mut db, &WorkspaceSelector::Id(WorkspaceId::new())).unwrap_err();
        assert!(matches!(error, LocateError::Database { .. }));
        assert!(std::error::Error::source(&error).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn normalizes_symlinks_and_missing_final_components() {
        let root = std::env::temp_dir().join(format!("trees-locator-{}", WorkspaceId::new()));
        std::fs::create_dir_all(root.join("workspace/repo")).unwrap();
        std::os::unix::fs::symlink(root.join("workspace"), root.join("alias")).unwrap();
        let canonical = CanonicalPath::resolve(root.join("workspace")).unwrap();
        let mut db = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let id = insert(&mut db, &canonical.to_string());
        let directory = CanonicalPath::resolve(root.join("alias/repo")).unwrap();
        assert_eq!(
            locate(&mut db, &WorkspaceSelector::ContainingDirectory(directory))
                .unwrap()
                .unwrap()
                .workspace
                .id,
            id
        );
        let missing =
            crate::validation::resolve_workspace_path(&root.join("alias/missing")).unwrap();
        assert_eq!(missing.as_path(), canonical.as_path().join("missing"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
