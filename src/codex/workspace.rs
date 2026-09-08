use std::path::{Path, PathBuf};

use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sqlite::SqliteConnection;
use snafu::{ResultExt, Snafu};

use crate::domain::{
    CanonicalPath, JsonDocument, LeaseId, OperationId, OperationState, Timestamp, WorkspaceId,
    WorkspaceState,
};
use crate::reconciliation::{self, ReconciliationError, RecoveryOutcome};
use crate::storage::{
    find_workspace_by_path, list_repo_worktrees, persist_operation_intent,
    record_operation_transition, with_short_transaction, OperationIntent, OperationIntentError,
    RepoWorktreeRow, TransitionMetadata,
};
use crate::validation::{self, ValidationError};

#[derive(Debug, Clone)]
pub struct PreparedWorkspace {
    pub id: WorkspaceId,
    pub path: CanonicalPath,
    pub name: String,
    pub roots: Vec<PathBuf>,
}

pub fn prepare(
    connection: &mut SqliteConnection,
    workspace_path: &Path,
) -> Result<PreparedWorkspace, WorkspacePreparationError> {
    let path = validation::resolve_workspace_path(workspace_path)?;
    let workspace = find_workspace_by_path(connection, &path)
        .context(DatabaseSnafu)?
        .ok_or_else(|| WorkspacePreparationError::NotManaged { path: path.clone() })?;

    match reconciliation::recover_expired_operation(connection, &workspace.id)
        .context(ReconciliationSnafu)?
    {
        RecoveryOutcome::LeaseActive => {
            return Err(WorkspacePreparationError::OperationActive {
                workspace_id: workspace.id,
            });
        }
        RecoveryOutcome::NoRunningOperation
        | RecoveryOutcome::Succeeded
        | RecoveryOutcome::RolledBack
        | RecoveryOutcome::Failed => {}
    }

    let (operation_id, lease_id) = start_reconciliation_operation(connection, workspace.id)?;
    let summary = match reconciliation::reconcile_workspace_with_lease(
        connection,
        &workspace.id,
        &operation_id,
        &lease_id,
    ) {
        Ok(summary) => summary,
        Err(error) => {
            let _ = finish_reconciliation_operation(connection, &lease_id, OperationState::Failed);
            return Err(WorkspacePreparationError::Reconciliation { source: error });
        }
    };
    finish_reconciliation_operation(connection, &lease_id, OperationState::Succeeded)
        .context(DatabaseSnafu)?;

    if summary.workspace_state != WorkspaceState::Ready {
        return Err(WorkspacePreparationError::NotReady {
            path,
            state: summary.workspace_state,
        });
    }

    let repositories = list_repo_worktrees(connection, &workspace.id).context(DatabaseSnafu)?;
    let roots = validate_worktree_roots(&repositories)?;
    let name = path
        .as_path()
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("workspace")
        .to_owned();

    Ok(PreparedWorkspace {
        id: workspace.id,
        path,
        name,
        roots,
    })
}

fn start_reconciliation_operation(
    connection: &mut SqliteConnection,
    workspace_id: WorkspaceId,
) -> Result<(OperationId, LeaseId), WorkspacePreparationError> {
    let intent = OperationIntent::new(
        workspace_id,
        "codex_reconciliation",
        Timestamp::after_seconds(300),
        "reconcile workspace",
        JsonDocument::parse(r#"{"command":"codex"}"#)?,
    );
    with_short_transaction(connection, |connection| {
        persist_operation_intent(connection, &intent)?;
        Ok::<(), DieselError>(())
    })
    .map_err(|error| match error {
        DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
            WorkspacePreparationError::OperationActive { workspace_id }
        }
        source => WorkspacePreparationError::Database { source },
    })?;
    Ok((intent.id, intent.lease_id))
}

fn finish_reconciliation_operation(
    connection: &mut SqliteConnection,
    lease_id: &LeaseId,
    state: OperationState,
) -> Result<(), DieselError> {
    record_operation_transition(
        connection,
        lease_id,
        state,
        TransitionMetadata::new("codex_reconciliation_finished", "trees")
            .with_pending_step("reconciliation complete"),
    )
}

fn validate_worktree_roots(
    repositories: &[RepoWorktreeRow],
) -> Result<Vec<PathBuf>, WorkspacePreparationError> {
    if repositories.is_empty() {
        return Err(WorkspacePreparationError::NoWorktrees);
    }

    repositories
        .iter()
        .map(|repository| {
            if repository.state != crate::domain::RepoWorktreeState::Attached {
                return Err(WorkspacePreparationError::InvalidWorktree {
                    path: repository.worktree_path.clone().into_path_buf(),
                    reason: format!("worktree state is {}", repository.state),
                });
            }
            let path = repository.worktree_path.clone().into_path_buf();
            if !path.is_absolute() || !path.is_dir() {
                return Err(WorkspacePreparationError::InvalidWorktree {
                    path,
                    reason: "worktree path is not an existing directory".to_owned(),
                });
            }
            Ok(path)
        })
        .collect()
}

#[derive(Debug, Snafu)]
pub enum WorkspacePreparationError {
    #[snafu(transparent)]
    Validation { source: ValidationError },
    #[snafu(display("database operation failed: {source}"))]
    Database { source: DieselError },
    #[snafu(display("reconciliation failed: {source}"))]
    Reconciliation { source: ReconciliationError },
    #[snafu(transparent)]
    Json {
        source: crate::domain::JsonDocumentError,
    },
    #[snafu(display("workspace is not managed: {path}"))]
    NotManaged { path: CanonicalPath },
    #[snafu(display("workspace is not ready: {path} ({state})"))]
    NotReady {
        path: CanonicalPath,
        state: WorkspaceState,
    },
    #[snafu(display("workspace has an active operation: {workspace_id}"))]
    OperationActive { workspace_id: WorkspaceId },
    #[snafu(display("workspace has no tracked worktrees"))]
    NoWorktrees,
    #[snafu(display("invalid managed worktree {}: {reason}", path.display()))]
    InvalidWorktree { path: PathBuf, reason: String },
}

impl From<OperationIntentError> for WorkspacePreparationError {
    fn from(error: OperationIntentError) -> Self {
        match error {
            OperationIntentError::WorkspaceBusy { workspace_id } => {
                Self::OperationActive { workspace_id }
            }
            OperationIntentError::Database { source } => Self::Database { source },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_preparation_errors_and_exposes_sources() {
        let path = PathBuf::from("/tmp/workspace");
        let canonical_path = CanonicalPath::from_absolute(&path).expect("path should be absolute");
        let json_error =
            crate::domain::JsonDocument::parse("not-json").expect_err("JSON should be invalid");
        let errors = [
            WorkspacePreparationError::Validation {
                source: ValidationError::NoRepositories,
            },
            WorkspacePreparationError::Database {
                source: DieselError::NotFound,
            },
            WorkspacePreparationError::Reconciliation {
                source: ReconciliationError::Database {
                    source: DieselError::NotFound,
                },
            },
            WorkspacePreparationError::Json { source: json_error },
            WorkspacePreparationError::NotManaged {
                path: canonical_path.clone(),
            },
            WorkspacePreparationError::NotReady {
                path: canonical_path,
                state: WorkspaceState::Degraded,
            },
            WorkspacePreparationError::OperationActive {
                workspace_id: WorkspaceId::new(),
            },
            WorkspacePreparationError::NoWorktrees,
            WorkspacePreparationError::InvalidWorktree {
                path,
                reason: "missing".to_owned(),
            },
        ];

        for error in errors {
            assert!(!error.to_string().is_empty());
            let _ = std::error::Error::source(&error);
        }
    }
}
