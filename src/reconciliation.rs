use std::fmt;

use diesel::sqlite::SqliteConnection;

use crate::domain::{JsonDocument, OperationId, RepoWorktreeState, WorkspaceId, WorkspaceState};
use crate::git::GitError;
use crate::storage::{
    find_workspace, list_repo_worktrees, record_repo_worktree_transition,
    record_workspace_transition, update_workspace_observation, TransitionMetadata,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ReconciliationSummary {
    pub changed_worktrees: usize,
    pub workspace_state: WorkspaceState,
}

pub fn reconcile_workspace(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    operation_id: &OperationId,
) -> Result<ReconciliationSummary, ReconciliationError> {
    let workspace =
        find_workspace(connection, workspace_id).map_err(ReconciliationError::Database)?;
    let repositories =
        list_repo_worktrees(connection, workspace_id).map_err(ReconciliationError::Database)?;
    let mut changed_worktrees = 0;
    let mut observed_states = Vec::with_capacity(repositories.len());

    for repository in repositories {
        let observation = match crate::git::list_worktrees(&repository.source_path) {
            Ok(worktrees) => {
                let worktree = worktrees
                    .into_iter()
                    .find(|worktree| worktree.path.as_path() == repository.worktree_path.as_path());
                match worktree {
                    Some(worktree)
                        if worktree.detached
                            && worktree.head.as_deref() == repository.last_head.as_deref() =>
                    {
                        Observation::Attached {
                            head: worktree.head,
                        }
                    }
                    Some(worktree) => Observation::Diverged {
                        head: worktree.head,
                        branch: worktree.branch,
                    },
                    None => Observation::Missing,
                }
            }
            Err(error) => Observation::Failed(error.to_string()),
        };
        let (state, head, details, error_json) = observation.into_record();
        observed_states.push(state);
        if state != repository.state || head.as_deref() != repository.last_head.as_deref() {
            changed_worktrees += 1;
            record_repo_worktree_transition(
                connection,
                &repository.id,
                operation_id,
                state,
                head,
                TransitionMetadata {
                    event_type: "external_worktree_changed".to_owned(),
                    source: "reconciliation".to_owned(),
                    details_json: details,
                    error_json,
                },
            )
            .map_err(ReconciliationError::Database)?;
        }
    }

    let workspace_state = if observed_states.iter().any(|state| {
        matches!(
            state,
            RepoWorktreeState::Missing | RepoWorktreeState::Diverged | RepoWorktreeState::Failed
        )
    }) {
        WorkspaceState::Degraded
    } else if observed_states
        .iter()
        .all(|state| *state == RepoWorktreeState::Attached)
    {
        WorkspaceState::Ready
    } else {
        WorkspaceState::Creating
    };

    if workspace.state != workspace_state {
        record_workspace_transition(
            connection,
            workspace_id,
            operation_id,
            workspace_state,
            TransitionMetadata::new("workspace_reconciled", "reconciliation"),
        )
        .map_err(ReconciliationError::Database)?;
    } else {
        let now = crate::domain::Timestamp::now();
        update_workspace_observation(connection, workspace_id, workspace_state, &now, &now)
            .map_err(ReconciliationError::Database)?;
    }

    Ok(ReconciliationSummary {
        changed_worktrees,
        workspace_state,
    })
}

enum Observation {
    Attached {
        head: Option<String>,
    },
    Diverged {
        head: Option<String>,
        branch: Option<String>,
    },
    Missing,
    Failed(String),
}

impl Observation {
    fn into_record(
        self,
    ) -> (
        RepoWorktreeState,
        Option<String>,
        Option<JsonDocument>,
        Option<JsonDocument>,
    ) {
        match self {
            Self::Attached { head } => (RepoWorktreeState::Attached, head, None, None),
            Self::Diverged { head, branch } => (
                RepoWorktreeState::Diverged,
                head,
                Some(json_details("diverged", branch)),
                None,
            ),
            Self::Missing => (
                RepoWorktreeState::Missing,
                None,
                Some(json_details("missing", None)),
                None,
            ),
            Self::Failed(error) => (
                RepoWorktreeState::Failed,
                None,
                None,
                Some(json_error(&error)),
            ),
        }
    }
}

fn json_details(state: &str, branch: Option<String>) -> JsonDocument {
    JsonDocument::from_serializable(&serde_json::json!({
        "observed_state": state,
        "branch": branch,
    }))
    .expect("reconciliation details should serialize")
}

fn json_error(error: &str) -> JsonDocument {
    JsonDocument::from_serializable(&serde_json::json!({ "error": error }))
        .expect("reconciliation error should serialize")
}

#[derive(Debug)]
pub enum ReconciliationError {
    Database(diesel::result::Error),
    Git(GitError),
}

impl fmt::Display for ReconciliationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => {
                write!(
                    formatter,
                    "reconciliation database operation failed: {error}"
                )
            }
            Self::Git(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ReconciliationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Git(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::*;
    use crate::workspace::{execute_creation, initialize_creation, prepare_create, CreateRequest};

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("trees-reconciliation-{}", uuid::Uuid::now_v7()))
    }

    fn run_git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .expect("git should run");
        assert!(output.status.success());
    }

    #[test]
    fn records_an_externally_removed_worktree() {
        let root = test_root();
        let source = root.join("repo");
        fs::create_dir_all(&source).expect("repository should be created");
        run_git(&source, &["init", "-q"]);
        run_git(&source, &["config", "user.email", "trees@example.invalid"]);
        run_git(&source, &["config", "user.name", "trees tests"]);
        fs::write(source.join("README"), "test\n").expect("test file should be written");
        run_git(&source, &["add", "README"]);
        run_git(&source, &["commit", "-qm", "initial"]);

        let plan = prepare_create(&CreateRequest {
            workspace_path: root.join("workspace"),
            repositories: vec![source],
        })
        .expect("creation plan should be prepared");
        let database_path = root.join("state.sqlite");
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");
        let context =
            initialize_creation(&mut connection, plan).expect("creation should initialize");
        execute_creation(&mut connection, &context).expect("creation should execute");
        crate::storage::finalize_creation(
            &mut connection,
            &context.workspace_id,
            &context.operation_id,
        )
        .expect("creation should finalize");

        let repository = &context.repositories[0];
        crate::git::remove_worktree(&repository.plan.source_path, &repository.plan.worktree_path)
            .expect("external worktree removal should succeed");
        let summary = reconcile_workspace(
            &mut connection,
            &context.workspace_id,
            &context.operation_id,
        )
        .expect("reconciliation should succeed");

        assert_eq!(summary.changed_worktrees, 1);
        assert_eq!(summary.workspace_state, WorkspaceState::Degraded);
        assert_eq!(
            crate::storage::list_repo_worktrees(&mut connection, &context.workspace_id).unwrap()[0]
                .state,
            RepoWorktreeState::Missing
        );
        assert_eq!(
            crate::storage::find_workspace(&mut connection, &context.workspace_id)
                .unwrap()
                .state,
            WorkspaceState::Degraded
        );
        let event_count =
            crate::storage::list_events_for_operation(&mut connection, &context.operation_id)
                .unwrap()
                .len();
        let repeated = reconcile_workspace(
            &mut connection,
            &context.workspace_id,
            &context.operation_id,
        )
        .expect("repeated reconciliation should succeed");
        assert_eq!(repeated.changed_worktrees, 0);
        assert_eq!(
            crate::storage::list_events_for_operation(&mut connection, &context.operation_id)
                .unwrap()
                .len(),
            event_count
        );

        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }
}
