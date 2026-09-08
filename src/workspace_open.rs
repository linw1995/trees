use std::fmt;

use diesel::sqlite::SqliteConnection;
use diesel::Connection;

use crate::domain::{CanonicalPath, WorkspaceId, WorkspaceManagementMode, WorkspaceState};
use crate::storage::find_workspace_open_snapshot;

pub fn resolve_target(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
) -> Result<CanonicalPath, WorkspaceOpenError> {
    connection.transaction(|connection| {
        let snapshot = find_workspace_open_snapshot(connection, workspace_id)
            .map_err(WorkspaceOpenError::Database)?
            .ok_or(WorkspaceOpenError::NotFound(*workspace_id))?;
        if snapshot.workspace.state == WorkspaceState::Reclaimed {
            return Err(WorkspaceOpenError::Reclaimed(*workspace_id));
        }
        if snapshot.operation_lease.is_some() {
            return Err(WorkspaceOpenError::OperationActive(*workspace_id));
        }
        if snapshot.workspace.management_mode == WorkspaceManagementMode::Automatic
            && snapshot.claim.is_none()
        {
            return Err(WorkspaceOpenError::AutomaticUnclaimed(*workspace_id));
        }
        Ok(snapshot.workspace.canonical_path)
    })
}

#[derive(Debug)]
pub enum WorkspaceOpenError {
    NotFound(WorkspaceId),
    Reclaimed(WorkspaceId),
    OperationActive(WorkspaceId),
    AutomaticUnclaimed(WorkspaceId),
    Database(diesel::result::Error),
}

impl From<diesel::result::Error> for WorkspaceOpenError {
    fn from(error: diesel::result::Error) -> Self {
        Self::Database(error)
    }
}

impl fmt::Display for WorkspaceOpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(workspace_id) => {
                write!(formatter, "workspace not found: {workspace_id}")
            }
            Self::Reclaimed(workspace_id) => {
                write!(formatter, "workspace has been reclaimed: {workspace_id}")
            }
            Self::OperationActive(workspace_id) => {
                write!(
                    formatter,
                    "workspace has an active operation: {workspace_id}"
                )
            }
            Self::AutomaticUnclaimed(workspace_id) => {
                write!(
                    formatter,
                    "automatic workspace is unclaimed: {workspace_id}"
                )
            }
            Self::Database(error) => write!(formatter, "failed to read workspace: {error}"),
        }
    }
}

impl std::error::Error for WorkspaceOpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::NotFound(_)
            | Self::Reclaimed(_)
            | Self::OperationActive(_)
            | Self::AutomaticUnclaimed(_) => None,
        }
    }
}
