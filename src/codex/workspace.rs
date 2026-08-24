use std::fmt;
use std::path::{Path, PathBuf};

use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sqlite::SqliteConnection;

use crate::domain::{
    CanonicalPath, JsonDocument, OperationId, OperationState, Timestamp, WorkspaceId,
    WorkspaceState,
};
use crate::reconciliation::{self, ReconciliationError, RecoveryOutcome};
use crate::storage::{
    append_event, find_workspace_by_path, list_repo_worktrees, persist_operation_intent,
    record_operation_transition, with_short_transaction, EventDraft, OperationIntent,
    OperationIntentError, RepoWorktreeRow, TransitionMetadata,
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
    let path = validation::resolve_workspace_path(workspace_path)
        .map_err(WorkspacePreparationError::Validation)?;
    let workspace = find_workspace_by_path(connection, &path)
        .map_err(WorkspacePreparationError::Database)?
        .ok_or_else(|| WorkspacePreparationError::NotManaged(path.clone()))?;

    match reconciliation::recover_expired_operation(connection, &workspace.id)
        .map_err(WorkspacePreparationError::Reconciliation)?
    {
        RecoveryOutcome::LeaseActive => {
            return Err(WorkspacePreparationError::OperationActive(workspace.id));
        }
        RecoveryOutcome::NoRunningOperation
        | RecoveryOutcome::Succeeded
        | RecoveryOutcome::RolledBack
        | RecoveryOutcome::Failed => {}
    }

    let operation_id = start_reconciliation_operation(connection, workspace.id)?;
    let summary =
        match reconciliation::reconcile_workspace(connection, &workspace.id, &operation_id) {
            Ok(summary) => summary,
            Err(error) => {
                let _ = finish_reconciliation_operation(
                    connection,
                    &operation_id,
                    OperationState::Failed,
                );
                return Err(WorkspacePreparationError::Reconciliation(error));
            }
        };
    finish_reconciliation_operation(connection, &operation_id, OperationState::Succeeded)
        .map_err(WorkspacePreparationError::Database)?;

    if summary.workspace_state != WorkspaceState::Ready {
        return Err(WorkspacePreparationError::NotReady {
            path,
            state: summary.workspace_state,
        });
    }

    let repositories = list_repo_worktrees(connection, &workspace.id)
        .map_err(WorkspacePreparationError::Database)?;
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
) -> Result<OperationId, WorkspacePreparationError> {
    let intent = OperationIntent::new(
        workspace_id,
        "codex_reconciliation",
        format!("process:{}", std::process::id()),
        Timestamp::after_seconds(300),
        "reconcile workspace",
        JsonDocument::parse(r#"{"command":"codex"}"#).map_err(WorkspacePreparationError::Json)?,
    );
    with_short_transaction(connection, |connection| {
        persist_operation_intent(connection, &intent)?;
        append_event(
            connection,
            &EventDraft {
                operation_id: intent.id,
                entity_type: "operation".to_owned(),
                entity_id: intent.id.to_string(),
                event_type: "codex_reconciliation_started".to_owned(),
                source: "trees".to_owned(),
                occurred_at: intent.started_at.clone(),
                previous_state: None,
                current_state: Some(OperationState::Running.to_string()),
                details_json: None,
                error_json: None,
            },
        )?;
        Ok::<(), DieselError>(())
    })
    .map_err(|error| match error {
        DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
            WorkspacePreparationError::OperationActive(workspace_id)
        }
        error => WorkspacePreparationError::Database(error),
    })?;
    Ok(intent.id)
}

fn finish_reconciliation_operation(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
    state: OperationState,
) -> Result<(), DieselError> {
    record_operation_transition(
        connection,
        operation_id,
        state,
        "reconciliation complete",
        None,
        TransitionMetadata::new("codex_reconciliation_finished", "trees"),
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

#[derive(Debug)]
pub enum WorkspacePreparationError {
    Validation(ValidationError),
    Database(DieselError),
    Reconciliation(ReconciliationError),
    Json(crate::domain::JsonDocumentError),
    NotManaged(CanonicalPath),
    NotReady {
        path: CanonicalPath,
        state: WorkspaceState,
    },
    OperationActive(WorkspaceId),
    NoWorktrees,
    InvalidWorktree {
        path: PathBuf,
        reason: String,
    },
}

impl fmt::Display for WorkspacePreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(formatter),
            Self::Database(error) => write!(formatter, "database operation failed: {error}"),
            Self::Reconciliation(error) => write!(formatter, "reconciliation failed: {error}"),
            Self::Json(error) => error.fmt(formatter),
            Self::NotManaged(path) => write!(formatter, "workspace is not managed: {path}"),
            Self::NotReady { path, state } => {
                write!(formatter, "workspace is not ready: {} ({state})", path)
            }
            Self::OperationActive(workspace_id) => {
                write!(
                    formatter,
                    "workspace has an active operation: {workspace_id}"
                )
            }
            Self::NoWorktrees => formatter.write_str("workspace has no tracked worktrees"),
            Self::InvalidWorktree { path, reason } => {
                write!(
                    formatter,
                    "invalid managed worktree {}: {reason}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for WorkspacePreparationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::Reconciliation(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<OperationIntentError> for WorkspacePreparationError {
    fn from(error: OperationIntentError) -> Self {
        match error {
            OperationIntentError::WorkspaceBusy(workspace_id) => {
                Self::OperationActive(workspace_id)
            }
            OperationIntentError::Database(error) => Self::Database(error),
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
            WorkspacePreparationError::Validation(ValidationError::NoRepositories),
            WorkspacePreparationError::Database(DieselError::NotFound),
            WorkspacePreparationError::Reconciliation(ReconciliationError::Database(
                DieselError::NotFound,
            )),
            WorkspacePreparationError::Json(json_error),
            WorkspacePreparationError::NotManaged(canonical_path.clone()),
            WorkspacePreparationError::NotReady {
                path: canonical_path,
                state: WorkspaceState::Degraded,
            },
            WorkspacePreparationError::OperationActive(WorkspaceId::new()),
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
