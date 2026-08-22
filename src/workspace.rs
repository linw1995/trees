use std::fmt;
use std::fs;
use std::path::PathBuf;

use diesel::sqlite::SqliteConnection;
use serde::Serialize;

use crate::domain::{
    CanonicalPath, JsonDocument, OperationState, RepoWorktreeId, RepoWorktreeState, Timestamp,
    WorkspaceId, WorkspaceState,
};
use crate::git::{self, GitError};
use crate::naming::{self, NamingError, WorktreePlan};
use crate::reconciliation::{self, ReconciliationError};
use crate::storage::{
    append_event, finalize_creation as finalize_persisted_creation, find_workspace_by_path,
    insert_repo_worktree, insert_workspace, persist_operation_intent,
    persist_operation_step_intent, record_operation_transition, record_repo_worktree_transition,
    record_workspace_transition, record_worktree_step_result, with_short_transaction, EventDraft,
    NewRepoWorktree, NewWorkspace, OperationIntent, TransitionMetadata,
};
use crate::validation::{self, ValidationError};

#[derive(Debug, Clone)]
pub struct CreateRequest {
    pub workspace_path: PathBuf,
    pub repositories: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct CreationPlan {
    pub workspace_path: CanonicalPath,
    pub repositories: Vec<RepositoryPlan>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct RepositoryPlan {
    pub source_path: CanonicalPath,
    pub repository_identity: CanonicalPath,
    pub worktree_path: PathBuf,
    pub head: String,
}

#[derive(Debug, Clone)]
pub struct TrackedRepository {
    pub id: RepoWorktreeId,
    pub plan: RepositoryPlan,
}

#[derive(Debug, Clone)]
pub struct CreationContext {
    pub workspace_id: WorkspaceId,
    pub operation_id: crate::domain::OperationId,
    pub plan: CreationPlan,
    pub repositories: Vec<TrackedRepository>,
}

pub fn prepare_create(request: &CreateRequest) -> Result<CreationPlan, WorkspaceError> {
    let input = validation::validate_create(&request.workspace_path, &request.repositories)?;
    let worktrees = naming::plan_worktrees(&input)?;
    let repositories = worktrees
        .into_iter()
        .map(repository_plan)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CreationPlan {
        workspace_path: input.workspace_path,
        repositories,
    })
}

fn repository_plan(plan: WorktreePlan) -> Result<RepositoryPlan, WorkspaceError> {
    let info = git::inspect_repository(&plan.repository)?;
    Ok(RepositoryPlan {
        source_path: plan.repository,
        repository_identity: info.common_dir,
        worktree_path: plan.worktree_path,
        head: info.head,
    })
}

pub fn initialize_creation(
    connection: &mut SqliteConnection,
    plan: CreationPlan,
) -> Result<CreationContext, WorkspaceError> {
    if find_workspace_by_path(connection, &plan.workspace_path)
        .map_err(WorkspaceError::Database)?
        .is_some()
    {
        return Err(WorkspaceError::AlreadyManaged(plan.workspace_path));
    }

    let workspace_id = WorkspaceId::new();
    let owner_id = format!("process:{}", std::process::id());
    let operation_intent = OperationIntent::new(
        workspace_id,
        "create",
        owner_id,
        Timestamp::after_seconds(300),
        "prepare worktrees",
        JsonDocument::from_serializable(&plan).map_err(WorkspaceError::Json)?,
    );
    let repositories = plan
        .repositories
        .iter()
        .map(|repository| TrackedRepository {
            id: RepoWorktreeId::new(),
            plan: repository.clone(),
        })
        .collect::<Vec<_>>();
    let now = Timestamp::now();

    with_short_transaction(connection, |connection| {
        insert_workspace(
            connection,
            &NewWorkspace {
                id: workspace_id,
                canonical_path: plan.workspace_path.clone(),
                state: WorkspaceState::Creating,
                created_at: now.clone(),
                updated_at: now.clone(),
                last_reconciled_at: None,
            },
        )?;
        for repository in &repositories {
            insert_repo_worktree(
                connection,
                &NewRepoWorktree {
                    id: repository.id,
                    workspace_id,
                    repository_identity: repository.plan.repository_identity.clone(),
                    source_path: repository.plan.source_path.clone(),
                    worktree_path: CanonicalPath::from_absolute(&repository.plan.worktree_path)
                        .map_err(|error| {
                            diesel::result::Error::QueryBuilderError(Box::new(error))
                        })?,
                    state: RepoWorktreeState::Pending,
                    last_head: Some(repository.plan.head.clone()),
                    last_observed_at: now.clone(),
                },
            )?;
        }
        persist_operation_intent(connection, &operation_intent)?;

        append_event(
            connection,
            &EventDraft {
                operation_id: operation_intent.id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace_id.to_string(),
                event_type: "workspace_created".to_owned(),
                source: "trees".to_owned(),
                occurred_at: now.clone(),
                previous_state: None,
                current_state: Some(WorkspaceState::Creating.to_string()),
                details_json: None,
                error_json: None,
            },
        )?;
        for repository in &repositories {
            append_event(
                connection,
                &EventDraft {
                    operation_id: operation_intent.id,
                    entity_type: "repo_worktree".to_owned(),
                    entity_id: repository.id.to_string(),
                    event_type: "worktree_planned".to_owned(),
                    source: "trees".to_owned(),
                    occurred_at: now.clone(),
                    previous_state: None,
                    current_state: Some(RepoWorktreeState::Pending.to_string()),
                    details_json: None,
                    error_json: None,
                },
            )?;
        }
        append_event(
            connection,
            &EventDraft {
                operation_id: operation_intent.id,
                entity_type: "operation".to_owned(),
                entity_id: operation_intent.id.to_string(),
                event_type: "operation_started".to_owned(),
                source: "trees".to_owned(),
                occurred_at: now,
                previous_state: None,
                current_state: Some("running".to_owned()),
                details_json: None,
                error_json: None,
            },
        )?;
        Ok::<(), diesel::result::Error>(())
    })
    .map_err(WorkspaceError::Database)?;

    Ok(CreationContext {
        workspace_id,
        operation_id: operation_intent.id,
        plan,
        repositories,
    })
}

pub fn execute_creation(
    connection: &mut SqliteConnection,
    context: &CreationContext,
) -> Result<(), WorkspaceError> {
    if let Err(source) = fs::create_dir(&context.plan.workspace_path) {
        let primary = WorkspaceError::Io {
            path: context.plan.workspace_path.clone().into_path_buf(),
            source,
        };
        return fail_creation(connection, context, &[], None, primary);
    }

    let mut completed = Vec::new();
    for repository in &context.repositories {
        if let Err(primary) = execute_repository_step(connection, context, repository) {
            return fail_creation(connection, context, &completed, Some(repository), primary);
        }
        completed.push(repository);
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct CreationResult {
    pub workspace_path: CanonicalPath,
    pub worktree_paths: Vec<PathBuf>,
}

pub fn create(request: CreateRequest) -> Result<CreationResult, WorkspaceError> {
    let plan = prepare_create(&request)?;
    let mut connection = crate::database::open_default().map_err(WorkspaceError::DatabaseOpen)?;
    create_with_connection(&mut connection, plan)
}

pub fn create_with_connection(
    connection: &mut SqliteConnection,
    plan: CreationPlan,
) -> Result<CreationResult, WorkspaceError> {
    let context = initialize_creation(connection, plan)?;
    reconciliation::reconcile_workspace(connection, &context.workspace_id, &context.operation_id)
        .map_err(WorkspaceError::Reconciliation)?;
    execute_creation(connection, &context)?;
    reconciliation::reconcile_workspace(connection, &context.workspace_id, &context.operation_id)
        .map_err(WorkspaceError::Reconciliation)?;
    finalize_persisted_creation(connection, &context.workspace_id, &context.operation_id)
        .map_err(WorkspaceError::Database)?;
    Ok(CreationResult {
        workspace_path: context.plan.workspace_path,
        worktree_paths: context
            .repositories
            .into_iter()
            .map(|repository| repository.plan.worktree_path)
            .collect(),
    })
}

fn execute_repository_step(
    connection: &mut SqliteConnection,
    context: &CreationContext,
    repository: &TrackedRepository,
) -> Result<(), WorkspaceError> {
    let intent_json =
        JsonDocument::from_serializable(&repository.plan).map_err(WorkspaceError::Json)?;
    persist_operation_step_intent(
        connection,
        &context.operation_id,
        format!("attach {}", repository.plan.source_path),
        intent_json,
    )
    .map_err(WorkspaceError::Database)?;
    git::add_detached_worktree(&repository.plan.source_path, &repository.plan.worktree_path)?;
    let worktree =
        git::find_worktree(&repository.plan.source_path, &repository.plan.worktree_path)?;
    record_worktree_step_result(
        connection,
        &repository.id,
        &context.operation_id,
        RepoWorktreeState::Attached,
        worktree.head,
        "worktree attached",
        TransitionMetadata::new("worktree_attached", "trees").with_details(
            JsonDocument::from_serializable(&repository.plan).map_err(WorkspaceError::Json)?,
        ),
    )
    .map_err(WorkspaceError::Database)?;
    reconciliation::reconcile_workspace(connection, &context.workspace_id, &context.operation_id)
        .map_err(WorkspaceError::Reconciliation)?;
    Ok(())
}

fn fail_creation(
    connection: &mut SqliteConnection,
    context: &CreationContext,
    completed: &[&TrackedRepository],
    failed: Option<&TrackedRepository>,
    primary: WorkspaceError,
) -> Result<(), WorkspaceError> {
    match rollback_creation(connection, context, completed, failed, &primary) {
        Ok(()) => Err(primary),
        Err(rollback) => Err(WorkspaceError::Rollback {
            primary: Box::new(primary),
            rollback: Box::new(rollback),
        }),
    }
}

fn rollback_creation(
    connection: &mut SqliteConnection,
    context: &CreationContext,
    completed: &[&TrackedRepository],
    failed: Option<&TrackedRepository>,
    primary: &WorkspaceError,
) -> Result<(), WorkspaceError> {
    let error_json = error_document(primary);
    let mut errors = Vec::new();
    let failed_id = failed.map(|repository| repository.id);
    let repositories = failed
        .into_iter()
        .chain(completed.iter().rev().copied())
        .collect::<Vec<_>>();

    for repository in repositories {
        match git::find_worktree(&repository.plan.source_path, &repository.plan.worktree_path) {
            Ok(_) => {
                if let Err(error) = git::remove_worktree(
                    &repository.plan.source_path,
                    &repository.plan.worktree_path,
                ) {
                    errors.push(error.to_string());
                }
            }
            Err(GitError::WorktreeNotFound(_)) => {}
            Err(_error) if failed_id == Some(repository.id) => {}
            Err(error) => errors.push(error.to_string()),
        }
        if let Err(error) = record_repo_worktree_transition(
            connection,
            &repository.id,
            &context.operation_id,
            RepoWorktreeState::Failed,
            None,
            TransitionMetadata::new("worktree_rollback", "trees").with_error(error_json.clone()),
        ) {
            errors.push(error.to_string());
        }
    }

    if context.plan.workspace_path.as_path().exists() {
        if let Err(error) = fs::remove_dir(&context.plan.workspace_path) {
            errors.push(error.to_string());
        }
    }

    let operation_state = if errors.is_empty() {
        OperationState::RolledBack
    } else {
        OperationState::Failed
    };
    let operation_event = if operation_state == OperationState::RolledBack {
        "operation_rolled_back"
    } else {
        "operation_rollback_failed"
    };
    if let Err(error) = record_operation_transition(
        connection,
        &context.operation_id,
        operation_state,
        "rollback complete",
        None,
        TransitionMetadata::new(operation_event, "trees").with_error(error_json.clone()),
    ) {
        errors.push(error.to_string());
    }
    if let Err(error) = record_workspace_transition(
        connection,
        &context.workspace_id,
        &context.operation_id,
        WorkspaceState::Failed,
        TransitionMetadata::new("workspace_creation_failed", "trees").with_error(error_json),
    ) {
        errors.push(error.to_string());
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(WorkspaceError::RollbackFailure { errors })
    }
}

fn error_document(error: &WorkspaceError) -> JsonDocument {
    JsonDocument::from_serializable(&serde_json::json!({
        "error": error.to_string(),
    }))
    .expect("JSON error document should serialize")
}

#[derive(Debug)]
pub enum WorkspaceError {
    Validation(ValidationError),
    Naming(NamingError),
    Git(GitError),
    Database(diesel::result::Error),
    DatabaseOpen(crate::database::DatabaseError),
    Reconciliation(ReconciliationError),
    Json(crate::domain::JsonDocumentError),
    AlreadyManaged(CanonicalPath),
    Rollback {
        primary: Box<WorkspaceError>,
        rollback: Box<WorkspaceError>,
    },
    RollbackFailure {
        errors: Vec<String>,
    },
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(formatter),
            Self::Naming(error) => error.fmt(formatter),
            Self::Git(error) => error.fmt(formatter),
            Self::Database(error) => write!(formatter, "database operation failed: {error}"),
            Self::DatabaseOpen(error) => {
                write!(formatter, "failed to open lifecycle database: {error}")
            }
            Self::Reconciliation(error) => write!(formatter, "reconciliation failed: {error}"),
            Self::Json(error) => error.fmt(formatter),
            Self::AlreadyManaged(path) => write!(formatter, "workspace is already managed: {path}"),
            Self::Rollback { primary, rollback } => {
                write!(
                    formatter,
                    "workspace creation failed: {primary}; rollback failed: {rollback}"
                )
            }
            Self::RollbackFailure { errors } => {
                write!(
                    formatter,
                    "workspace rollback failed: {}",
                    errors.join("; ")
                )
            }
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "workspace filesystem operation failed for {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for WorkspaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(error) => Some(error),
            Self::Naming(error) => Some(error),
            Self::Git(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::DatabaseOpen(error) => Some(error),
            Self::Reconciliation(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::AlreadyManaged(_) => None,
            Self::Rollback { primary, .. } => Some(primary),
            Self::RollbackFailure { .. } => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

impl From<ValidationError> for WorkspaceError {
    fn from(error: ValidationError) -> Self {
        Self::Validation(error)
    }
}

impl From<NamingError> for WorkspaceError {
    fn from(error: NamingError) -> Self {
        Self::Naming(error)
    }
}

impl From<GitError> for WorkspaceError {
    fn from(error: GitError) -> Self {
        Self::Git(error)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use super::*;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("trees-workspace-{}", uuid::Uuid::now_v7()))
    }

    fn run_git(path: &std::path::Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .expect("git should run");
        assert!(output.status.success());
    }

    fn repository(path: &std::path::Path) {
        fs::create_dir_all(path).expect("repository should be created");
        run_git(path, &["init", "-q"]);
        run_git(path, &["config", "user.email", "trees@example.invalid"]);
        run_git(path, &["config", "user.name", "trees tests"]);
        fs::write(path.join("README"), "test\n").expect("test file should be written");
        run_git(path, &["add", "README"]);
        run_git(path, &["commit", "-qm", "initial"]);
    }

    #[test]
    fn prepares_a_multi_repository_creation_plan() {
        let root = test_root();
        let first = root.join("first");
        let second = root.join("second");
        repository(&first);
        repository(&second);

        let plan = prepare_create(&CreateRequest {
            workspace_path: root.join("workspace"),
            repositories: vec![first, second],
        })
        .expect("creation plan should be prepared");

        assert_eq!(plan.repositories.len(), 2);
        assert!(plan
            .repositories
            .iter()
            .all(|repository| !repository.head.is_empty()));
        assert!(plan.repositories.iter().all(|repository| {
            repository
                .worktree_path
                .starts_with(plan.workspace_path.as_path())
        }));
        let database_path = root.join("state.sqlite");
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");
        let context =
            initialize_creation(&mut connection, plan).expect("creation should initialize");
        execute_creation(&mut connection, &context).expect("creation should execute");
        finalize_persisted_creation(
            &mut connection,
            &context.workspace_id,
            &context.operation_id,
        )
        .expect("creation should finalize");
        assert!(context.plan.workspace_path.as_path().exists());
        assert!(context
            .repositories
            .iter()
            .all(|repository| repository.plan.worktree_path.exists()));
        let workspace =
            crate::storage::find_workspace_by_path(&mut connection, &context.plan.workspace_path)
                .unwrap()
                .unwrap();
        assert_eq!(workspace.state, WorkspaceState::Ready);
        assert_eq!(
            crate::storage::list_repo_worktrees(&mut connection, &context.workspace_id)
                .unwrap()
                .iter()
                .filter(|worktree| worktree.state == RepoWorktreeState::Attached)
                .count(),
            2
        );
        assert_eq!(
            crate::storage::find_operation(&mut connection, &context.operation_id)
                .unwrap()
                .id,
            context.operation_id
        );
        assert_eq!(
            crate::storage::find_operation(&mut connection, &context.operation_id)
                .unwrap()
                .state,
            OperationState::Succeeded
        );
        assert_eq!(
            crate::storage::list_events_for_operation(&mut connection, &context.operation_id)
                .unwrap()
                .len(),
            11
        );
        for repository in &context.repositories {
            crate::git::remove_worktree(
                &repository.plan.source_path,
                &repository.plan.worktree_path,
            )
            .expect("created worktree should be removable");
        }
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn rolls_back_worktrees_when_a_later_repository_fails() {
        let root = test_root();
        let first = root.join("first");
        let second = root.join("second");
        repository(&first);
        repository(&second);
        let second_git = second.join(".git");
        let plan = prepare_create(&CreateRequest {
            workspace_path: root.join("workspace"),
            repositories: vec![first, second],
        })
        .expect("creation plan should be prepared");
        let database_path = root.join("state.sqlite");
        let mut connection =
            crate::database::connect(&database_path).expect("database should open");
        let context =
            initialize_creation(&mut connection, plan).expect("creation should initialize");
        fs::remove_dir_all(second_git).expect("second repository metadata should be removed");

        assert!(execute_creation(&mut connection, &context).is_err());
        assert!(!context.plan.workspace_path.as_path().exists());
        assert!(context
            .repositories
            .iter()
            .all(|repository| !repository.plan.worktree_path.exists()));
        assert_eq!(
            crate::storage::find_workspace_by_path(&mut connection, &context.plan.workspace_path)
                .unwrap()
                .unwrap()
                .state,
            WorkspaceState::Failed
        );
        assert_eq!(
            crate::storage::find_operation(&mut connection, &context.operation_id)
                .unwrap()
                .state,
            OperationState::RolledBack
        );
        assert_eq!(
            crate::storage::list_repo_worktrees(&mut connection, &context.workspace_id)
                .unwrap()
                .iter()
                .filter(|worktree| worktree.state == RepoWorktreeState::Failed)
                .count(),
            2
        );
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn formats_workspace_errors_and_sources() {
        let path = CanonicalPath::resolve(".").expect("workspace path should resolve");
        let primary = WorkspaceError::AlreadyManaged(path.clone());
        let errors = [
            WorkspaceError::Validation(ValidationError::NoRepositories),
            WorkspaceError::Naming(NamingError::MissingRepositoryName(path.clone())),
            WorkspaceError::Git(GitError::WorktreeNotFound(path.as_path().to_owned())),
            WorkspaceError::Database(diesel::result::Error::NotFound),
            WorkspaceError::DatabaseOpen(crate::database::DatabaseError::Path(
                crate::paths::PathError::HomeDirectoryUnavailable,
            )),
            WorkspaceError::Json(JsonDocument::parse("not json").unwrap_err()),
            primary,
            WorkspaceError::Rollback {
                primary: Box::new(WorkspaceError::AlreadyManaged(path.clone())),
                rollback: Box::new(WorkspaceError::RollbackFailure {
                    errors: vec!["rollback error".to_owned()],
                }),
            },
            WorkspaceError::RollbackFailure {
                errors: vec!["rollback error".to_owned()],
            },
            WorkspaceError::Io {
                path: path.into_path_buf(),
                source: std::io::Error::other("io error"),
            },
        ];

        for error in errors {
            assert!(!error.to_string().is_empty());
            let _ = std::error::Error::source(&error);
        }
    }
}
