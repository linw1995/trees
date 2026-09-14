pub mod persistence;
pub mod planning;
pub mod workflow;
pub use workflow::execute;

use std::path::PathBuf;

use diesel::{Connection, SqliteConnection};
use serde::{Deserialize, Serialize};
use snafu::{ensure, ResultExt, Snafu};

use crate::domain::{
    CanonicalPath, ClaimId, OperationId, OriginRepositoryId, PoolId, RepoWorktreeId, WorkspaceId,
    WorkspaceManagementMode, WorkspaceState,
};
use crate::storage::{self, WorkspaceRow};
use crate::workspace_locator::{self, WorkspaceSelector};

#[derive(Debug)]
pub struct AddRequest {
    pub selector: WorkspaceSelector,
    pub repositories: Vec<PathBuf>,
    pub offline: bool,
}

#[derive(Debug)]
pub struct AddTarget {
    pub workspace: WorkspaceRow,
    pub claim_id: Option<ClaimId>,
}

pub fn locate_target(
    connection: &mut SqliteConnection,
    selector: &WorkspaceSelector,
) -> Result<AddTarget, AddError> {
    connection.transaction(|connection| {
        let located = workspace_locator::locate(connection, selector)?
            .ok_or_else(|| NotFoundSnafu.build())?;
        ensure!(
            located.workspace.state != WorkspaceState::Removed,
            RemovedSnafu
        );
        let claim = storage::find_workspace_claim(connection, &located.workspace.id)?;
        let claim_id = claim.map(|claim| claim.id);
        ensure!(
            located.selected_claim_id.is_none() || located.selected_claim_id == claim_id,
            ClaimChangedSnafu
        );
        ensure!(
            located.workspace.management_mode == WorkspaceManagementMode::Manual
                || claim_id.is_some(),
            UnclaimedSnafu
        );
        Ok(AddTarget {
            workspace: located.workspace,
            claim_id,
        })
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryPlan {
    pub origin_repository_id: OriginRepositoryId,
    pub worktree_id: RepoWorktreeId,
    pub source_path: CanonicalPath,
    pub repository_identity: CanonicalPath,
    pub worktree_path: CanonicalPath,
    pub head: String,
    pub git_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relocation {
    pub worktree_id: RepoWorktreeId,
    pub previous_path: CanonicalPath,
    pub worktree_path: CanonicalPath,
    pub staging_path: CanonicalPath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddPlan {
    pub version: u32,
    pub workspace_id: WorkspaceId,
    pub workspace_path: CanonicalPath,
    pub claim_id: Option<ClaimId>,
    pub previous_pool_id: Option<PoolId>,
    pub existing: Vec<RepositoryPlan>,
    pub additions: Vec<RepositoryPlan>,
    pub requested: Vec<OriginRepositoryId>,
    pub relocation: Option<Relocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryOutcome {
    Added,
    AlreadyPresent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryResult {
    pub origin_repository_id: OriginRepositoryId,
    pub worktree_id: RepoWorktreeId,
    pub worktree_path: CanonicalPath,
    pub result: RepositoryOutcome,
}

#[derive(Debug, Serialize)]
pub struct RelocatedResult {
    pub worktree_id: RepoWorktreeId,
    pub previous_path: CanonicalPath,
    pub worktree_path: CanonicalPath,
}

#[derive(Debug, Serialize)]
pub struct AddResult {
    pub schema_version: u32,
    pub operation_id: OperationId,
    pub workspace_id: WorkspaceId,
    pub workspace_path: CanonicalPath,
    pub claim_id: Option<ClaimId>,
    pub previous_pool_id: Option<PoolId>,
    pub pool_id: Option<PoolId>,
    pub repositories: Vec<RepositoryResult>,
    pub relocated: Vec<RelocatedResult>,
}

#[derive(Debug, Snafu)]
pub enum AddError {
    #[snafu(transparent)]
    Reconcile {
        source: crate::reconciliation::ReconciliationError,
    },
    #[snafu(transparent)]
    Locate {
        source: workspace_locator::LocateError,
    },
    #[snafu(
        context(false),
        display("addition database operation failed: {source}")
    )]
    Database { source: diesel::result::Error },
    #[snafu(transparent)]
    Git { source: crate::git::GitError },
    #[snafu(transparent)]
    Resolve {
        source: crate::origin::resolve::ResolveError,
    },
    #[snafu(transparent)]
    Path {
        source: crate::domain::CanonicalPathError,
    },
    #[snafu(transparent)]
    Validation {
        source: crate::validation::ValidationError,
    },
    #[snafu(display("failed to serialize addition journal: {source}"))]
    Json {
        source: crate::domain::JsonDocumentError,
    },
    #[snafu(display("invalid addition journal: {source}"))]
    Decode { source: serde_json::Error },
    #[snafu(display("filesystem operation failed for {}: {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("workspace target was not found"))]
    NotFound,
    #[snafu(display("cannot add repositories to a removed workspace"))]
    Removed,
    #[snafu(display("automatic workspace must have an active claim before adding repositories"))]
    Unclaimed,
    #[snafu(display("workspace claim changed before addition"))]
    ClaimChanged,
    #[snafu(display("workspace has an unresolved addition; retry trees add to recover it"))]
    Unresolved,
    #[snafu(display("workspace has an active operation"))]
    Busy,
    #[snafu(display("unsafe addition path or worktree: {}", path.display()))]
    Unsafe { path: PathBuf },
    #[snafu(display("unsupported or incomplete addition journal"))]
    Journal,
    #[snafu(display("at least one repository is required"))]
    Empty,
    #[snafu(display("addition failed: {source}; compensation failed: {cleanup}"))]
    Compensation {
        source: Box<AddError>,
        cleanup: Box<AddError>,
    },
}

fn document(value: &impl Serialize) -> Result<crate::domain::JsonDocument, AddError> {
    crate::domain::JsonDocument::from_serializable(value).context(JsonSnafu)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Arguments {
        #[command(flatten)]
        add: crate::cli::AddArgs,
    }

    #[test]
    fn selectors_are_exclusive_and_repositories_are_required() {
        let id = WorkspaceId::new().to_string();
        let claim = ClaimId::new().to_string();
        let selections = [
            vec!["."],
            vec!["--workspace-dir", "."],
            vec!["--workspace-id", &id],
            vec!["--claim-id", &claim],
        ];
        for (index, selection) in selections.iter().enumerate() {
            let mut args = vec!["add", "--repo", "web"];
            args.extend(selection);
            Arguments::try_parse_from(&args)
                .unwrap()
                .add
                .selector()
                .unwrap();
            for other in selections.iter().skip(index + 1) {
                let mut conflicting = args.clone();
                conflicting.extend(other);
                assert!(Arguments::try_parse_from(conflicting).is_err());
            }
        }
        assert!(Arguments::try_parse_from(["add"]).is_err());
        let arguments = Arguments::try_parse_from([
            "add",
            "--repo",
            "web",
            "--repo",
            "api",
            "--offline",
            "--json",
        ])
        .unwrap();
        assert_eq!(arguments.add.repositories.len(), 2);
        assert!(arguments.add.offline && arguments.add.json);
    }

    #[test]
    fn removed_inner_workspace_does_not_fall_back() {
        use crate::domain::Timestamp;
        let mut db = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        for (path, state) in [
            ("/outer", WorkspaceState::Ready),
            ("/outer/inner", WorkspaceState::Removed),
        ] {
            storage::insert_workspace(
                &mut db,
                &storage::NewWorkspace {
                    id: WorkspaceId::new(),
                    canonical_path: CanonicalPath::from_absolute(path).unwrap(),
                    state,
                    created_at: Timestamp::now(),
                    updated_at: Timestamp::now(),
                    last_reconciled_at: None,
                },
            )
            .unwrap();
        }
        let error = locate_target(
            &mut db,
            &WorkspaceSelector::ContainingDirectory(
                CanonicalPath::from_absolute("/outer/inner/repo").unwrap(),
            ),
        )
        .unwrap_err();
        assert!(matches!(error, AddError::Removed));
    }
}
