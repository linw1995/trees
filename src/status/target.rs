use diesel::SqliteConnection;
use snafu::Snafu;

use crate::domain::{CanonicalPath, CanonicalPathError, ClaimId, WorkspaceId};
use crate::storage::WorkspaceRow;
use crate::workspace_locator::{locate, LocateError, WorkspaceSelector};

#[derive(Debug, Snafu)]
pub enum TargetError {
    #[snafu(display("failed to resolve status invocation directory: {source}"))]
    Directory { source: CanonicalPathError },
    #[snafu(display("failed to select status workspace: {source}"))]
    Query { source: diesel::result::Error },
    #[snafu(display("unknown workspace: {workspace_id}"))]
    Unknown { workspace_id: WorkspaceId },
    #[snafu(display("workspace not found: {path}"))]
    UnknownPath { path: CanonicalPath },
    #[snafu(display("workspace claim not found: {claim_id}"))]
    UnknownClaim { claim_id: ClaimId },
    #[snafu(transparent)]
    Locate { source: LocateError },
}

pub fn select(
    selector: &WorkspaceSelector,
    connection: Option<&mut SqliteConnection>,
) -> Result<Option<WorkspaceRow>, TargetError> {
    let located = match connection {
        Some(connection) => locate(connection, selector).map_err(|error| match error {
            LocateError::Database { source } => TargetError::Query { source },
            error => TargetError::from(error),
        })?,
        None => None,
    };
    match located {
        Some(located) => Ok(Some(located.workspace)),
        None => match selector {
            WorkspaceSelector::Id(id) => UnknownSnafu { workspace_id: *id }.fail(),
            WorkspaceSelector::ExactPath(path) => UnknownPathSnafu { path: path.clone() }.fail(),
            WorkspaceSelector::ClaimId(id) => UnknownClaimSnafu { claim_id: *id }.fail(),
            WorkspaceSelector::ContainingDirectory(_) => Ok(None),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_storage_only_allows_implicit_directory_selection() {
        for selector in [
            WorkspaceSelector::Id(WorkspaceId::new()),
            WorkspaceSelector::ExactPath(CanonicalPath::from_absolute("/missing").unwrap()),
            WorkspaceSelector::ClaimId(ClaimId::new()),
        ] {
            assert!(select(&selector, None).is_err());
        }
        let directory = WorkspaceSelector::ContainingDirectory(
            CanonicalPath::from_absolute("/missing").unwrap(),
        );
        assert!(select(&directory, None).unwrap().is_none());
    }
}
