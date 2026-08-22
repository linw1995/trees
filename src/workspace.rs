use std::fmt;
use std::path::PathBuf;

use diesel::sqlite::SqliteConnection;
use serde::Serialize;

use crate::domain::{
    CanonicalPath, JsonDocument, RepoWorktreeId, RepoWorktreeState, Timestamp, WorkspaceId,
    WorkspaceState,
};
use crate::git::{self, GitError};
use crate::naming::{self, NamingError, WorktreePlan};
use crate::storage::{
    append_event, find_workspace_by_path, insert_repo_worktree, insert_workspace,
    persist_operation_intent, with_short_transaction, EventDraft, NewRepoWorktree, NewWorkspace,
    OperationIntent,
};
use crate::validation::{self, ValidationError};

#[derive(Debug, Clone)]
pub struct CreateRequest {
    pub workspace_path: PathBuf,
    pub repositories: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreationPlan {
    pub workspace_path: CanonicalPath,
    pub repositories: Vec<RepositoryPlan>,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug)]
pub enum WorkspaceError {
    Validation(ValidationError),
    Naming(NamingError),
    Git(GitError),
    Database(diesel::result::Error),
    Json(crate::domain::JsonDocumentError),
    AlreadyManaged(CanonicalPath),
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(formatter),
            Self::Naming(error) => error.fmt(formatter),
            Self::Git(error) => error.fmt(formatter),
            Self::Database(error) => write!(formatter, "database operation failed: {error}"),
            Self::Json(error) => error.fmt(formatter),
            Self::AlreadyManaged(path) => write!(formatter, "workspace is already managed: {path}"),
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
            Self::Json(error) => Some(error),
            Self::AlreadyManaged(_) => None,
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
        let workspace =
            crate::storage::find_workspace_by_path(&mut connection, &context.plan.workspace_path)
                .unwrap()
                .unwrap();
        assert_eq!(workspace.state, WorkspaceState::Creating);
        assert_eq!(
            crate::storage::list_repo_worktrees(&mut connection, &context.workspace_id)
                .unwrap()
                .iter()
                .filter(|worktree| worktree.state == RepoWorktreeState::Pending)
                .count(),
            2
        );
        assert_eq!(
            crate::storage::find_running_operation(&mut connection, &context.workspace_id)
                .unwrap()
                .unwrap()
                .id,
            context.operation_id
        );
        assert_eq!(
            crate::storage::list_events_for_operation(&mut connection, &context.operation_id)
                .unwrap()
                .len(),
            4
        );
        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("test root should be removable");
    }
}
