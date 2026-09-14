use diesel::sqlite::SqliteConnection;
use diesel::Connection;
use snafu::{OptionExt, Snafu};

use crate::domain::{CanonicalPath, ClaimId, WorkspaceId, WorkspaceManagementMode, WorkspaceState};
use crate::storage::find_workspace_open_snapshot;
use crate::workspace_locator::{locate, LocateError, WorkspaceSelector};

pub fn resolve_target(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
) -> Result<CanonicalPath, WorkspaceOpenError> {
    resolve_selector(connection, &WorkspaceSelector::Id(*workspace_id))
}

pub fn resolve_selector(
    connection: &mut SqliteConnection,
    selector: &WorkspaceSelector,
) -> Result<CanonicalPath, WorkspaceOpenError> {
    connection.transaction(|connection| {
        let located = locate(connection, selector).map_err(|error| match error {
            LocateError::Database { source } => WorkspaceOpenError::Database { source },
            error => WorkspaceOpenError::from(error),
        })?;
        let Some(located) = located else {
            return match selector {
                WorkspaceSelector::Id(id) => NotFoundSnafu { workspace_id: *id }.fail(),
                WorkspaceSelector::ExactPath(path)
                | WorkspaceSelector::ContainingDirectory(path) => {
                    UnknownPathSnafu { path: path.clone() }.fail()
                }
                WorkspaceSelector::ClaimId(id) => UnknownClaimSnafu { claim_id: *id }.fail(),
            };
        };
        let workspace_id = &located.workspace.id;
        let snapshot =
            find_workspace_open_snapshot(connection, workspace_id)?.context(NotFoundSnafu {
                workspace_id: *workspace_id,
            })?;
        if snapshot.workspace.state == WorkspaceState::Removed {
            return Err(WorkspaceOpenError::Removed {
                workspace_id: *workspace_id,
            });
        }
        if snapshot.operation_lease.is_some() {
            return Err(WorkspaceOpenError::OperationActive {
                workspace_id: *workspace_id,
            });
        }
        if snapshot.workspace.management_mode == WorkspaceManagementMode::Automatic
            && snapshot.claim.is_none()
        {
            return Err(WorkspaceOpenError::AutomaticUnclaimed {
                workspace_id: *workspace_id,
            });
        }
        Ok(snapshot.workspace.canonical_path)
    })
}

#[derive(Debug, Snafu)]
pub enum WorkspaceOpenError {
    #[snafu(transparent)]
    Locate { source: LocateError },
    #[snafu(display("workspace not found: {path}"))]
    UnknownPath { path: CanonicalPath },
    #[snafu(display("workspace claim not found: {claim_id}"))]
    UnknownClaim { claim_id: ClaimId },
    #[snafu(display("workspace not found: {workspace_id}"))]
    NotFound { workspace_id: WorkspaceId },
    #[snafu(display("workspace has been removed: {workspace_id}"))]
    Removed { workspace_id: WorkspaceId },
    #[snafu(display("workspace has an active operation: {workspace_id}"))]
    OperationActive { workspace_id: WorkspaceId },
    #[snafu(display("automatic workspace is unclaimed: {workspace_id}"))]
    AutomaticUnclaimed { workspace_id: WorkspaceId },
    #[snafu(context(false), display("failed to read workspace: {source}"))]
    Database { source: diesel::result::Error },
}
