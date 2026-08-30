use std::fmt;

use diesel::sqlite::SqliteConnection;

use crate::domain::{
    JsonDocument, OperationId, OperationState, RepoWorktreeState, Timestamp, WorkspaceId,
    WorkspaceState,
};
use crate::git::GitError;
use crate::storage::{
    append_event, claim_expired_operation, finalize_creation, find_operation,
    find_running_operation, find_workspace, list_repo_worktrees, record_operation_transition,
    record_repo_worktree_transition, record_workspace_transition, update_workspace_observation,
    EventDraft, OperationRow, RepoWorktreeRow, TransitionMetadata, WorkspaceRow,
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
    if workspace.state == WorkspaceState::Reclaimed {
        return Ok(ReconciliationSummary {
            changed_worktrees: 0,
            workspace_state: WorkspaceState::Reclaimed,
        });
    }
    let repositories =
        list_repo_worktrees(connection, workspace_id).map_err(ReconciliationError::Database)?;
    let mut changed_worktrees = 0;
    let mut observed_states = Vec::with_capacity(repositories.len());

    for repository in repositories {
        let observation = observe_repository(&repository);
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
            RepoWorktreeState::Dirty
                | RepoWorktreeState::Missing
                | RepoWorktreeState::Diverged
                | RepoWorktreeState::Failed
                | RepoWorktreeState::Reclaimed
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

fn observe_repository(repository: &RepoWorktreeRow) -> Observation {
    let actual_repository_identity =
        match crate::git::inspect_repository_identity(&repository.source_path) {
            Ok(identity) => identity,
            Err(error) => return Observation::Failed(error.to_string()),
        };
    if actual_repository_identity != repository.repository_identity {
        return Observation::Diverged {
            head: None,
            branch: None,
            reason: Some(format!(
                "source repository identity changed from {} to {}",
                repository.repository_identity, actual_repository_identity
            )),
        };
    }

    match crate::git::list_worktrees(&repository.source_path) {
        Ok(worktrees) => {
            let worktree = worktrees
                .into_iter()
                .find(|worktree| worktree.path.as_path() == repository.worktree_path.as_path());
            match worktree {
                Some(worktree) if worktree.prunable.is_some() => Observation::Missing {
                    reason: Some("Git marked the worktree as prunable".to_owned()),
                },
                Some(_) if !repository.worktree_path.as_path().exists() => Observation::Missing {
                    reason: Some("worktree path does not exist".to_owned()),
                },
                Some(worktree) => {
                    match crate::git::inspect_worktree_identity(repository.worktree_path.as_path())
                    {
                        Ok(identity) => {
                            let clean =
                                crate::git::is_worktree_clean(repository.worktree_path.as_path())
                                    .unwrap_or(false);
                            let fingerprint = crate::git::ObservationFingerprint::from_worktree(
                                identity, worktree, clean,
                            );
                            if fingerprint.repository_identity != repository.repository_identity {
                                Observation::Diverged {
                                    head: fingerprint.head,
                                    branch: fingerprint.branch,
                                    reason: Some(format!(
                                        "worktree identity changed from {} to {}",
                                        repository.repository_identity,
                                        fingerprint.repository_identity
                                    )),
                                }
                            } else if fingerprint.matches_attachment(
                                &repository.repository_identity,
                                &repository.worktree_path,
                                repository.last_head.as_deref(),
                            ) {
                                if fingerprint.clean {
                                    Observation::Attached {
                                        head: fingerprint.head,
                                    }
                                } else {
                                    Observation::Dirty {
                                        head: fingerprint.head,
                                    }
                                }
                            } else {
                                Observation::Diverged {
                                    head: fingerprint.head,
                                    branch: fingerprint.branch,
                                    reason: Some(
                                        "worktree observation fingerprint changed".to_owned(),
                                    ),
                                }
                            }
                        }
                        Err(error) => Observation::Diverged {
                            head: worktree.head,
                            branch: worktree.branch,
                            reason: Some(format!("worktree identity is unavailable: {error}")),
                        },
                    }
                }
                None if repository.state == RepoWorktreeState::Pending => Observation::Pending {
                    head: repository.last_head.clone(),
                },
                None if !repository.worktree_path.as_path().exists() => {
                    Observation::Missing { reason: None }
                }
                None => {
                    match crate::git::inspect_worktree_identity(repository.worktree_path.as_path())
                    {
                        Ok(identity) => Observation::Diverged {
                            head: None,
                            branch: None,
                            reason: Some(format!(
                                "worktree identity {} is not listed by the source repository",
                                identity
                            )),
                        },
                        Err(error) => Observation::Diverged {
                            head: None,
                            branch: None,
                            reason: Some(format!("worktree path is not a Git worktree: {error}")),
                        },
                    }
                }
            }
        }
        Err(error) => Observation::Failed(error.to_string()),
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RecoveryOutcome {
    NoRunningOperation,
    LeaseActive,
    Succeeded,
    RolledBack,
    Failed,
}

pub fn recover_expired_operation(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
) -> Result<RecoveryOutcome, ReconciliationError> {
    let operation = match find_running_operation(connection, workspace_id)
        .map_err(ReconciliationError::Database)?
    {
        Some(operation) => operation,
        None => return Ok(RecoveryOutcome::NoRunningOperation),
    };
    if !operation.lease_expires_at.has_expired() {
        return Ok(RecoveryOutcome::LeaseActive);
    }
    let recovery_owner = format!("recovery:process:{}", std::process::id());
    let recovery_lease = Timestamp::after_seconds(300);
    if !claim_expired_operation(
        connection,
        &operation.id,
        &operation.owner_id,
        &operation.lease_expires_at,
        &recovery_owner,
        &recovery_lease,
    )
    .map_err(ReconciliationError::Database)?
    {
        return Ok(RecoveryOutcome::LeaseActive);
    }
    let operation =
        find_operation(connection, &operation.id).map_err(ReconciliationError::Database)?;

    let workspace =
        find_workspace(connection, workspace_id).map_err(ReconciliationError::Database)?;
    let repositories =
        list_repo_worktrees(connection, workspace_id).map_err(ReconciliationError::Database)?;
    let observations = repositories
        .iter()
        .map(observe_for_recovery)
        .collect::<Vec<_>>();
    let complete = !repositories.is_empty()
        && repositories
            .iter()
            .zip(&observations)
            .all(|(_, observation)| observation.complete);

    if complete {
        return recover_completed_operation(
            connection,
            workspace_id,
            &operation,
            &repositories,
            &observations,
        );
    }

    recover_incomplete_operation(
        connection,
        workspace_id,
        &workspace,
        &operation,
        &repositories,
        &observations,
    )
}

#[derive(Debug)]
struct RecoveryObservation {
    worktree: Option<crate::git::WorktreeInfo>,
    owned_by_operation: bool,
    complete: bool,
    error: Option<String>,
}

fn observe_for_recovery(repository: &RepoWorktreeRow) -> RecoveryObservation {
    let actual_repository_identity =
        match crate::git::inspect_repository_identity(&repository.source_path) {
            Ok(identity) => identity,
            Err(error) => return unsafe_recovery_observation(error.to_string()),
        };
    if actual_repository_identity != repository.repository_identity {
        return unsafe_recovery_observation(format!(
            "source repository identity changed from {} to {}",
            repository.repository_identity, actual_repository_identity
        ));
    }

    let worktree = match crate::git::list_worktrees(&repository.source_path) {
        Ok(worktrees) => worktrees
            .into_iter()
            .find(|worktree| worktree.path.as_path() == repository.worktree_path.as_path()),
        Err(error) => return unsafe_recovery_observation(error.to_string()),
    };
    let Some(worktree) = worktree else {
        if !repository.worktree_path.as_path().exists() {
            return RecoveryObservation {
                worktree: None,
                owned_by_operation: false,
                complete: false,
                error: None,
            };
        }

        return match crate::git::inspect_worktree_identity(repository.worktree_path.as_path()) {
            Ok(identity) => unsafe_recovery_observation(format!(
                "worktree identity {} is not listed by the source repository",
                identity
            )),
            Err(error) => {
                unsafe_recovery_observation(format!("worktree path is not a Git worktree: {error}"))
            }
        };
    };

    if worktree.prunable.is_some() || !repository.worktree_path.as_path().exists() {
        return RecoveryObservation {
            worktree: None,
            owned_by_operation: false,
            complete: false,
            error: None,
        };
    }

    let actual_worktree_identity =
        match crate::git::inspect_worktree_identity(repository.worktree_path.as_path()) {
            Ok(identity) => identity,
            Err(error) => return unsafe_recovery_observation(error.to_string()),
        };
    let clean = crate::git::is_worktree_clean(repository.worktree_path.as_path()).unwrap_or(false);
    let fingerprint = crate::git::ObservationFingerprint::from_worktree(
        actual_worktree_identity,
        worktree.clone(),
        clean,
    );
    if fingerprint.repository_identity != repository.repository_identity {
        return unsafe_recovery_observation(format!(
            "worktree identity changed from {} to {}",
            repository.repository_identity, fingerprint.repository_identity
        ));
    }

    RecoveryObservation {
        worktree: Some(worktree),
        owned_by_operation: true,
        complete: fingerprint.matches_attached(
            &repository.repository_identity,
            &repository.worktree_path,
            repository.last_head.as_deref(),
        ),
        error: None,
    }
}

fn unsafe_recovery_observation(error: String) -> RecoveryObservation {
    RecoveryObservation {
        worktree: None,
        owned_by_operation: false,
        complete: false,
        error: Some(error),
    }
}

fn recover_completed_operation(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    operation: &OperationRow,
    repositories: &[RepoWorktreeRow],
    observations: &[RecoveryObservation],
) -> Result<RecoveryOutcome, ReconciliationError> {
    for (repository, observation) in repositories.iter().zip(observations) {
        let head = observation
            .worktree
            .as_ref()
            .and_then(|worktree| worktree.head.clone());
        if repository.state != RepoWorktreeState::Attached
            || head.as_deref() != repository.last_head.as_deref()
        {
            record_repo_worktree_transition(
                connection,
                &repository.id,
                &operation.id,
                RepoWorktreeState::Attached,
                head,
                TransitionMetadata::new("worktree_recovered", "recovery"),
            )
            .map_err(ReconciliationError::Database)?;
        }
    }
    finalize_creation(connection, workspace_id, &operation.id)
        .map_err(ReconciliationError::Database)?;
    append_recovery_event(
        connection,
        &operation.id,
        OperationState::Succeeded,
        "operation_recovered",
    )?;
    Ok(RecoveryOutcome::Succeeded)
}

fn recover_incomplete_operation(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    workspace: &WorkspaceRow,
    operation: &OperationRow,
    repositories: &[RepoWorktreeRow],
    observations: &[RecoveryObservation],
) -> Result<RecoveryOutcome, ReconciliationError> {
    let worktree_error_json = json_error("operation did not complete before its lease expired");
    let mut errors = observations
        .iter()
        .filter_map(|observation| observation.error.clone())
        .collect::<Vec<_>>();
    for (repository, observation) in repositories.iter().zip(observations) {
        if observation.owned_by_operation {
            if let Err(error) = crate::git::remove_worktree(
                &repository.source_path,
                repository.worktree_path.as_path(),
            ) {
                errors.push(error.to_string());
            }
        }
        if let Err(error) = record_repo_worktree_transition(
            connection,
            &repository.id,
            &operation.id,
            RepoWorktreeState::Failed,
            None,
            TransitionMetadata::new("worktree_recovery_rollback", "recovery")
                .with_error(worktree_error_json.clone()),
        ) {
            errors.push(error.to_string());
        }
    }
    if errors.is_empty() && workspace.canonical_path.as_path().exists() {
        if let Err(error) = std::fs::remove_dir(&workspace.canonical_path) {
            errors.push(error.to_string());
        }
    }
    let error_json = recovery_error(&errors);

    let operation_state = if errors.is_empty() {
        OperationState::RolledBack
    } else {
        OperationState::Failed
    };
    if let Err(error) = record_operation_transition(
        connection,
        &operation.id,
        operation_state,
        "recovery rollback complete",
        None,
        TransitionMetadata::new("operation_recovered", "recovery").with_error(error_json.clone()),
    ) {
        errors.push(error.to_string());
    }
    record_workspace_transition(
        connection,
        workspace_id,
        &operation.id,
        WorkspaceState::Failed,
        TransitionMetadata::new("workspace_recovery_failed", "recovery").with_error(error_json),
    )
    .map_err(ReconciliationError::Database)?;
    append_recovery_event(
        connection,
        &operation.id,
        operation_state,
        "operation_recovered",
    )?;

    if errors.is_empty() {
        Ok(RecoveryOutcome::RolledBack)
    } else {
        Ok(RecoveryOutcome::Failed)
    }
}

fn append_recovery_event(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
    state: OperationState,
    event_type: &str,
) -> Result<(), ReconciliationError> {
    append_event(
        connection,
        &EventDraft {
            operation_id: *operation_id,
            entity_type: "operation".to_owned(),
            entity_id: operation_id.to_string(),
            event_type: event_type.to_owned(),
            source: "recovery".to_owned(),
            occurred_at: Timestamp::now(),
            previous_state: Some(OperationState::Running.to_string()),
            current_state: Some(state.to_string()),
            details_json: None,
            error_json: None,
        },
    )
    .map(|_| ())
    .map_err(ReconciliationError::Database)
}

enum Observation {
    Attached {
        head: Option<String>,
    },
    Dirty {
        head: Option<String>,
    },
    Pending {
        head: Option<String>,
    },
    Diverged {
        head: Option<String>,
        branch: Option<String>,
        reason: Option<String>,
    },
    Missing {
        reason: Option<String>,
    },
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
            Self::Dirty { head } => (
                RepoWorktreeState::Dirty,
                head,
                Some(json_details(
                    "dirty",
                    None,
                    Some("worktree has local changes".to_owned()),
                )),
                None,
            ),
            Self::Pending { head } => (RepoWorktreeState::Pending, head, None, None),
            Self::Diverged {
                head,
                branch,
                reason,
            } => (
                RepoWorktreeState::Diverged,
                head,
                Some(json_details("diverged", branch, reason)),
                None,
            ),
            Self::Missing { reason } => (
                RepoWorktreeState::Missing,
                None,
                Some(json_details("missing", None, reason)),
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

fn json_details(state: &str, branch: Option<String>, reason: Option<String>) -> JsonDocument {
    JsonDocument::from_serializable(&serde_json::json!({
        "observed_state": state,
        "branch": branch,
        "reason": reason,
    }))
    .expect("reconciliation details should serialize")
}

fn json_error(error: &str) -> JsonDocument {
    JsonDocument::from_serializable(&serde_json::json!({ "error": error }))
        .expect("reconciliation error should serialize")
}

fn recovery_error(errors: &[String]) -> JsonDocument {
    JsonDocument::from_serializable(&serde_json::json!({
        "error": "operation did not complete before its lease expired",
        "details": errors,
    }))
    .expect("recovery error should serialize")
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

    use diesel::prelude::*;

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

    fn setup_context() -> (
        PathBuf,
        diesel::sqlite::SqliteConnection,
        crate::workspace::CreationContext,
    ) {
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
        (root, connection, context)
    }

    fn expire_operation(
        connection: &mut diesel::sqlite::SqliteConnection,
        operation_id: &OperationId,
    ) {
        diesel::update(crate::schema::operations::table.find(operation_id))
            .set(crate::schema::operations::lease_expires_at.eq(Timestamp::now()))
            .execute(connection)
            .expect("operation lease should be updated");
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

    #[test]
    fn records_dirty_worktrees_idempotently() {
        let (root, mut connection, context) = setup_context();
        execute_creation(&mut connection, &context).expect("creation should execute");
        crate::storage::finalize_creation(
            &mut connection,
            &context.workspace_id,
            &context.operation_id,
        )
        .expect("creation should finalize");

        let repository = &context.repositories[0];
        fs::write(
            repository.plan.worktree_path.join("untracked"),
            "local change\n",
        )
        .expect("untracked file should be written");
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
            RepoWorktreeState::Dirty
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

        crate::git::remove_worktree(&repository.plan.source_path, &repository.plan.worktree_path)
            .expect("dirty worktree should be removable during test cleanup");
        drop(connection);
        fs::remove_file(root.join("state.sqlite")).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn records_branch_change_before_dirty_state() {
        let (root, mut connection, context) = setup_context();
        execute_creation(&mut connection, &context).expect("creation should execute");
        crate::storage::finalize_creation(
            &mut connection,
            &context.workspace_id,
            &context.operation_id,
        )
        .expect("creation should finalize");

        let repository = &context.repositories[0];
        run_git(
            &repository.plan.worktree_path,
            &["checkout", "-q", "-b", "external"],
        );
        fs::write(
            repository.plan.worktree_path.join("untracked"),
            "local change\n",
        )
        .expect("untracked file should be written");
        reconcile_workspace(
            &mut connection,
            &context.workspace_id,
            &context.operation_id,
        )
        .expect("reconciliation should succeed");

        assert_eq!(
            crate::storage::list_repo_worktrees(&mut connection, &context.workspace_id).unwrap()[0]
                .state,
            RepoWorktreeState::Diverged
        );

        crate::git::remove_worktree(&repository.plan.source_path, &repository.plan.worktree_path)
            .expect("changed worktree should be removable during test cleanup");
        drop(connection);
        fs::remove_file(root.join("state.sqlite")).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn records_a_replaced_source_repository_as_diverged() {
        let (root, mut connection, context) = setup_context();
        execute_creation(&mut connection, &context).expect("creation should execute");
        crate::storage::finalize_creation(
            &mut connection,
            &context.workspace_id,
            &context.operation_id,
        )
        .expect("creation should finalize");

        let source = context.repositories[0]
            .plan
            .source_path
            .as_path()
            .to_owned();
        fs::remove_dir_all(&source).expect("original source repository should be removed");
        fs::create_dir_all(&source).expect("replacement source repository should be created");
        run_git(&source, &["init", "-q"]);
        run_git(&source, &["config", "user.email", "trees@example.invalid"]);
        run_git(&source, &["config", "user.name", "trees tests"]);
        fs::write(source.join("README"), "replacement\n")
            .expect("replacement file should be written");
        run_git(&source, &["add", "README"]);
        run_git(&source, &["commit", "-qm", "replacement"]);

        let summary = reconcile_workspace(
            &mut connection,
            &context.workspace_id,
            &context.operation_id,
        )
        .expect("reconciliation should succeed");
        assert_eq!(summary.workspace_state, WorkspaceState::Degraded);
        assert_eq!(summary.changed_worktrees, 1);
        assert_eq!(
            crate::storage::list_repo_worktrees(&mut connection, &context.workspace_id).unwrap()[0]
                .state,
            RepoWorktreeState::Diverged
        );

        drop(connection);
        fs::remove_file(root.join("state.sqlite")).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn records_a_replaced_worktree_repository_as_diverged() {
        let (root, mut connection, context) = setup_context();
        execute_creation(&mut connection, &context).expect("creation should execute");
        crate::storage::finalize_creation(
            &mut connection,
            &context.workspace_id,
            &context.operation_id,
        )
        .expect("creation should finalize");

        let repository = &context.repositories[0];
        let worktree_path = repository.plan.worktree_path.clone();
        crate::git::remove_worktree(&repository.plan.source_path, &worktree_path)
            .expect("original worktree should be removed");
        fs::create_dir_all(&worktree_path).expect("replacement worktree repository should exist");
        run_git(&worktree_path, &["init", "-q"]);
        run_git(
            &worktree_path,
            &["config", "user.email", "trees@example.invalid"],
        );
        run_git(&worktree_path, &["config", "user.name", "trees tests"]);
        fs::write(worktree_path.join("README"), "replacement\n")
            .expect("replacement file should be written");
        run_git(&worktree_path, &["add", "README"]);
        run_git(&worktree_path, &["commit", "-qm", "replacement"]);

        let summary = reconcile_workspace(
            &mut connection,
            &context.workspace_id,
            &context.operation_id,
        )
        .expect("reconciliation should succeed");
        assert_eq!(summary.workspace_state, WorkspaceState::Degraded);
        assert_eq!(summary.changed_worktrees, 1);
        assert_eq!(
            crate::storage::list_repo_worktrees(&mut connection, &context.workspace_id).unwrap()[0]
                .state,
            RepoWorktreeState::Diverged
        );

        drop(connection);
        fs::remove_file(root.join("state.sqlite")).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn recovers_a_completed_git_operation() {
        let (root, mut connection, context) = setup_context();
        execute_creation(&mut connection, &context).expect("creation should execute");
        expire_operation(&mut connection, &context.operation_id);

        assert_eq!(
            recover_expired_operation(&mut connection, &context.workspace_id).unwrap(),
            RecoveryOutcome::Succeeded
        );
        assert_eq!(
            crate::storage::find_operation(&mut connection, &context.operation_id)
                .unwrap()
                .state,
            OperationState::Succeeded
        );
        assert!(
            crate::storage::list_events_for_operation(&mut connection, &context.operation_id)
                .unwrap()
                .iter()
                .any(|event| event.event_type == "operation_recovered")
        );

        for repository in &context.repositories {
            crate::git::remove_worktree(
                &repository.plan.source_path,
                &repository.plan.worktree_path,
            )
            .expect("recovered worktree should be removable");
        }
        drop(connection);
        fs::remove_file(root.join("state.sqlite")).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn rolls_back_a_partial_expired_operation() {
        let (root, mut connection, context) = setup_context();
        execute_creation(&mut connection, &context).expect("creation should execute");
        let first = &context.repositories[0];
        crate::git::remove_worktree(&first.plan.source_path, &first.plan.worktree_path)
            .expect("external partial removal should succeed");
        expire_operation(&mut connection, &context.operation_id);

        assert_eq!(
            recover_expired_operation(&mut connection, &context.workspace_id).unwrap(),
            RecoveryOutcome::RolledBack
        );
        assert!(!context.plan.workspace_path.as_path().exists());
        assert_eq!(
            crate::storage::find_operation(&mut connection, &context.operation_id)
                .unwrap()
                .state,
            OperationState::RolledBack
        );
        assert!(
            crate::storage::list_repo_worktrees(&mut connection, &context.workspace_id)
                .unwrap()
                .iter()
                .all(|repository| repository.state == RepoWorktreeState::Failed)
        );

        drop(connection);
        fs::remove_file(root.join("state.sqlite")).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn refuses_to_remove_a_replaced_worktree_during_recovery() {
        let (root, mut connection, context) = setup_context();
        execute_creation(&mut connection, &context).expect("creation should execute");
        let repository = &context.repositories[0];
        let worktree_path = repository.plan.worktree_path.clone();
        crate::git::remove_worktree(&repository.plan.source_path, &worktree_path)
            .expect("original worktree should be removed");
        fs::create_dir_all(&worktree_path).expect("replacement repository should be created");
        run_git(&worktree_path, &["init", "-q"]);
        run_git(
            &worktree_path,
            &["config", "user.email", "trees@example.invalid"],
        );
        run_git(&worktree_path, &["config", "user.name", "trees tests"]);
        fs::write(worktree_path.join("README"), "replacement\n")
            .expect("replacement file should be written");
        run_git(&worktree_path, &["add", "README"]);
        run_git(&worktree_path, &["commit", "-qm", "replacement"]);
        expire_operation(&mut connection, &context.operation_id);

        assert_eq!(
            recover_expired_operation(&mut connection, &context.workspace_id).unwrap(),
            RecoveryOutcome::Failed
        );
        assert!(worktree_path.join("README").exists());
        assert_eq!(
            crate::storage::find_operation(&mut connection, &context.operation_id)
                .unwrap()
                .state,
            OperationState::Failed
        );

        drop(connection);
        fs::remove_file(root.join("state.sqlite")).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn refuses_to_remove_workspace_when_source_identity_is_unavailable() {
        let (root, mut connection, context) = setup_context();
        execute_creation(&mut connection, &context).expect("creation should execute");
        let source = context.repositories[0]
            .plan
            .source_path
            .as_path()
            .to_owned();
        fs::remove_dir_all(source).expect("source repository should be unavailable");
        expire_operation(&mut connection, &context.operation_id);

        assert_eq!(
            recover_expired_operation(&mut connection, &context.workspace_id).unwrap(),
            RecoveryOutcome::Failed
        );
        assert!(context.plan.workspace_path.as_path().exists());
        assert_eq!(
            crate::storage::find_operation(&mut connection, &context.operation_id)
                .unwrap()
                .state,
            OperationState::Failed
        );

        drop(connection);
        fs::remove_file(root.join("state.sqlite")).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn distinguishes_absent_and_active_operations() {
        let database_path =
            std::env::temp_dir().join(format!("trees-recovery-{}.sqlite", uuid::Uuid::now_v7()));
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");
        assert_eq!(
            recover_expired_operation(&mut connection, &WorkspaceId::new()).unwrap(),
            RecoveryOutcome::NoRunningOperation
        );
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");

        let (root, mut connection, context) = setup_context();
        assert_eq!(
            recover_expired_operation(&mut connection, &context.workspace_id).unwrap(),
            RecoveryOutcome::LeaseActive
        );
        drop(connection);
        fs::remove_file(root.join("state.sqlite")).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }
}
