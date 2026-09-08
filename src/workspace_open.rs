use diesel::sqlite::SqliteConnection;
use diesel::Connection;
use snafu::Snafu;

use crate::domain::{CanonicalPath, WorkspaceId, WorkspaceManagementMode, WorkspaceState};
use crate::storage::find_workspace_open_snapshot;

pub fn resolve_target(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
) -> Result<CanonicalPath, WorkspaceOpenError> {
    connection.transaction(|connection| {
        let snapshot = find_workspace_open_snapshot(connection, workspace_id)
            .map_err(|source| WorkspaceOpenError::Database { source })?
            .ok_or(WorkspaceOpenError::NotFound {
                workspace_id: *workspace_id,
            })?;
        if snapshot.workspace.state == WorkspaceState::Reclaimed {
            return Err(WorkspaceOpenError::Reclaimed {
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
    #[snafu(display("workspace not found: {workspace_id}"))]
    NotFound { workspace_id: WorkspaceId },
    #[snafu(display("workspace has been reclaimed: {workspace_id}"))]
    Reclaimed { workspace_id: WorkspaceId },
    #[snafu(display("workspace has an active operation: {workspace_id}"))]
    OperationActive { workspace_id: WorkspaceId },
    #[snafu(display("automatic workspace is unclaimed: {workspace_id}"))]
    AutomaticUnclaimed { workspace_id: WorkspaceId },
    #[snafu(context(false), display("failed to read workspace: {source}"))]
    Database { source: diesel::result::Error },
}
