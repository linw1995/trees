use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error, QueryResult};
use diesel::sqlite::SqliteConnection;
use std::fmt;

use crate::domain::{
    CanonicalPath, JsonDocument, OperationId, OperationState, RepoWorktreeId, RepoWorktreeState,
    Timestamp, WorkspaceId, WorkspaceState,
};
use crate::schema::{lifecycle_events, operations, repo_worktrees, workspaces};

use super::models::{
    EventRow, NewEvent, NewOperation, NewRepoWorktree, NewWorkspace, OperationIntent, OperationRow,
    RepoWorktreeRow, WorkspaceRow,
};
use super::transaction::with_short_transaction;

#[derive(Debug, Clone)]
pub struct TransitionMetadata {
    pub event_type: String,
    pub source: String,
    pub details_json: Option<JsonDocument>,
    pub error_json: Option<JsonDocument>,
}

impl TransitionMetadata {
    pub fn new(event_type: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            source: source.into(),
            details_json: None,
            error_json: None,
        }
    }

    pub fn with_details(mut self, details_json: JsonDocument) -> Self {
        self.details_json = Some(details_json);
        self
    }

    pub fn with_error(mut self, error_json: JsonDocument) -> Self {
        self.error_json = Some(error_json);
        self
    }
}

#[derive(Debug, Clone)]
pub struct EventDraft {
    pub operation_id: OperationId,
    pub entity_type: String,
    pub entity_id: String,
    pub event_type: String,
    pub source: String,
    pub occurred_at: Timestamp,
    pub previous_state: Option<String>,
    pub current_state: Option<String>,
    pub details_json: Option<JsonDocument>,
    pub error_json: Option<JsonDocument>,
}

pub fn insert_workspace(
    connection: &mut SqliteConnection,
    value: &NewWorkspace,
) -> QueryResult<WorkspaceRow> {
    diesel::insert_into(workspaces::table)
        .values(value)
        .execute(connection)?;
    workspaces::table
        .find(&value.id)
        .select(WorkspaceRow::as_select())
        .first(connection)
}

pub fn find_workspace_by_path(
    connection: &mut SqliteConnection,
    path: &CanonicalPath,
) -> QueryResult<Option<WorkspaceRow>> {
    workspaces::table
        .filter(workspaces::canonical_path.eq(path))
        .select(WorkspaceRow::as_select())
        .first(connection)
        .optional()
}

pub fn insert_repo_worktree(
    connection: &mut SqliteConnection,
    value: &NewRepoWorktree,
) -> QueryResult<RepoWorktreeRow> {
    diesel::insert_into(repo_worktrees::table)
        .values(value)
        .execute(connection)?;
    repo_worktrees::table
        .find(&value.id)
        .select(RepoWorktreeRow::as_select())
        .first(connection)
}

pub fn list_repo_worktrees(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
) -> QueryResult<Vec<RepoWorktreeRow>> {
    repo_worktrees::table
        .filter(repo_worktrees::workspace_id.eq(workspace_id))
        .order(repo_worktrees::worktree_path.asc())
        .select(RepoWorktreeRow::as_select())
        .load(connection)
}

pub fn insert_operation(
    connection: &mut SqliteConnection,
    value: &NewOperation,
) -> QueryResult<OperationRow> {
    diesel::insert_into(operations::table)
        .values(value)
        .execute(connection)?;
    operations::table
        .find(&value.id)
        .select(OperationRow::as_select())
        .first(connection)
}

pub fn persist_operation_intent(
    connection: &mut SqliteConnection,
    intent: &OperationIntent,
) -> QueryResult<OperationRow> {
    insert_operation(
        connection,
        &NewOperation {
            id: intent.id,
            workspace_id: intent.workspace_id,
            kind: intent.kind.clone(),
            state: OperationState::Running,
            owner_id: intent.owner_id.clone(),
            lease_expires_at: intent.lease_expires_at.clone(),
            last_heartbeat_at: intent.started_at.clone(),
            started_at: intent.started_at.clone(),
            finished_at: None,
            pending_step: intent.pending_step.clone(),
            intent_json: intent.intent_json.clone(),
            error_json: None,
        },
    )
}

pub fn begin_operation(
    connection: &mut SqliteConnection,
    intent: &OperationIntent,
) -> Result<OperationRow, OperationIntentError> {
    persist_operation_intent(connection, intent).map_err(|error| match error {
        Error::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
            OperationIntentError::WorkspaceBusy(intent.workspace_id)
        }
        error => OperationIntentError::Database(error),
    })
}

#[derive(Debug)]
pub enum OperationIntentError {
    WorkspaceBusy(WorkspaceId),
    Database(diesel::result::Error),
}

impl fmt::Display for OperationIntentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkspaceBusy(workspace_id) => {
                write!(
                    formatter,
                    "workspace already has a running operation: {workspace_id}"
                )
            }
            Self::Database(error) => {
                write!(formatter, "failed to persist operation intent: {error}")
            }
        }
    }
}

impl std::error::Error for OperationIntentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::WorkspaceBusy(_) => None,
            Self::Database(error) => Some(error),
        }
    }
}

pub fn find_operation(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
) -> QueryResult<OperationRow> {
    operations::table
        .find(operation_id)
        .select(OperationRow::as_select())
        .first(connection)
}

pub fn find_running_operation(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
) -> QueryResult<Option<OperationRow>> {
    operations::table
        .filter(operations::workspace_id.eq(workspace_id))
        .filter(operations::state.eq(OperationState::Running))
        .select(OperationRow::as_select())
        .first(connection)
        .optional()
}

pub fn insert_event(connection: &mut SqliteConnection, value: &NewEvent) -> QueryResult<EventRow> {
    diesel::insert_into(lifecycle_events::table)
        .values(value)
        .execute(connection)?;
    lifecycle_events::table
        .find(&value.event_id)
        .select(EventRow::as_select())
        .first(connection)
}

pub fn append_event(
    connection: &mut SqliteConnection,
    draft: &EventDraft,
) -> QueryResult<EventRow> {
    insert_event(
        connection,
        &NewEvent {
            event_id: crate::domain::EventId::new(),
            operation_id: draft.operation_id,
            entity_type: draft.entity_type.clone(),
            entity_id: draft.entity_id.clone(),
            event_type: draft.event_type.clone(),
            source: draft.source.clone(),
            occurred_at: draft.occurred_at.clone(),
            previous_state: draft.previous_state.clone(),
            current_state: draft.current_state.clone(),
            details_json: draft.details_json.clone(),
            error_json: draft.error_json.clone(),
        },
    )
}

pub fn record_workspace_transition(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    operation_id: &OperationId,
    state: WorkspaceState,
    metadata: TransitionMetadata,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        let previous_state = workspaces::table
            .find(workspace_id)
            .select(workspaces::state)
            .first::<WorkspaceState>(connection)?;
        let occurred_at = Timestamp::now();

        diesel::update(workspaces::table.find(workspace_id))
            .set((
                workspaces::state.eq(state),
                workspaces::updated_at.eq(&occurred_at),
                workspaces::last_reconciled_at.eq(&occurred_at),
            ))
            .execute(connection)?;
        append_event(
            connection,
            &EventDraft {
                operation_id: *operation_id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace_id.to_string(),
                event_type: metadata.event_type,
                source: metadata.source,
                occurred_at,
                previous_state: Some(previous_state.to_string()),
                current_state: Some(state.to_string()),
                details_json: metadata.details_json,
                error_json: metadata.error_json,
            },
        )?;
        Ok(())
    })
}

pub fn record_repo_worktree_transition(
    connection: &mut SqliteConnection,
    worktree_id: &RepoWorktreeId,
    operation_id: &OperationId,
    state: RepoWorktreeState,
    last_head: Option<String>,
    metadata: TransitionMetadata,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        let previous_state = repo_worktrees::table
            .find(worktree_id)
            .select(repo_worktrees::state)
            .first::<RepoWorktreeState>(connection)?;
        let occurred_at = Timestamp::now();

        diesel::update(repo_worktrees::table.find(worktree_id))
            .set((
                repo_worktrees::state.eq(state),
                repo_worktrees::last_head.eq(last_head),
                repo_worktrees::last_observed_at.eq(&occurred_at),
            ))
            .execute(connection)?;
        append_event(
            connection,
            &EventDraft {
                operation_id: *operation_id,
                entity_type: "repo_worktree".to_owned(),
                entity_id: worktree_id.to_string(),
                event_type: metadata.event_type,
                source: metadata.source,
                occurred_at,
                previous_state: Some(previous_state.to_string()),
                current_state: Some(state.to_string()),
                details_json: metadata.details_json,
                error_json: metadata.error_json,
            },
        )?;
        Ok(())
    })
}

pub fn record_operation_transition(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
    state: OperationState,
    pending_step: impl Into<String>,
    finished_at: Option<Timestamp>,
    metadata: TransitionMetadata,
) -> QueryResult<()> {
    let pending_step = pending_step.into();
    with_short_transaction(connection, |connection| {
        let operation = operations::table
            .find(operation_id)
            .select(OperationRow::as_select())
            .first(connection)?;
        let occurred_at = Timestamp::now();
        let finished_at = finished_at.or_else(|| {
            matches!(
                state,
                OperationState::Succeeded | OperationState::Failed | OperationState::RolledBack
            )
            .then_some(occurred_at.clone())
        });

        diesel::update(operations::table.find(operation_id))
            .set((
                operations::state.eq(state),
                operations::pending_step.eq(pending_step),
                operations::last_heartbeat_at.eq(&occurred_at),
                operations::finished_at.eq(finished_at),
                operations::error_json.eq(metadata.error_json.clone()),
            ))
            .execute(connection)?;
        append_event(
            connection,
            &EventDraft {
                operation_id: *operation_id,
                entity_type: "operation".to_owned(),
                entity_id: operation.id.to_string(),
                event_type: metadata.event_type,
                source: metadata.source,
                occurred_at,
                previous_state: Some(operation.state.to_string()),
                current_state: Some(state.to_string()),
                details_json: metadata.details_json,
                error_json: metadata.error_json,
            },
        )?;
        Ok(())
    })
}

pub fn persist_operation_step_intent(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
    pending_step: impl Into<String>,
    intent_json: JsonDocument,
) -> QueryResult<()> {
    let pending_step = pending_step.into();
    with_short_transaction(connection, |connection| {
        let updated = diesel::update(operations::table.find(operation_id))
            .set((
                operations::pending_step.eq(pending_step),
                operations::intent_json.eq(intent_json),
                operations::last_heartbeat_at.eq(Timestamp::now()),
                operations::lease_expires_at.eq(Timestamp::after_seconds(300)),
            ))
            .execute(connection)?;
        if updated == 1 {
            Ok(())
        } else {
            Err(diesel::result::Error::NotFound)
        }
    })
}

pub fn record_worktree_step_result(
    connection: &mut SqliteConnection,
    worktree_id: &RepoWorktreeId,
    operation_id: &OperationId,
    state: RepoWorktreeState,
    last_head: Option<String>,
    pending_step: impl Into<String>,
    metadata: TransitionMetadata,
) -> QueryResult<()> {
    let pending_step = pending_step.into();
    with_short_transaction(connection, |connection| {
        let previous_state = repo_worktrees::table
            .find(worktree_id)
            .select(repo_worktrees::state)
            .first::<RepoWorktreeState>(connection)?;
        let operation_event_type = format!("operation_step_{}", metadata.event_type);
        let occurred_at = Timestamp::now();

        diesel::update(repo_worktrees::table.find(worktree_id))
            .set((
                repo_worktrees::state.eq(state),
                repo_worktrees::last_head.eq(last_head),
                repo_worktrees::last_observed_at.eq(&occurred_at),
            ))
            .execute(connection)?;
        diesel::update(operations::table.find(operation_id))
            .set((
                operations::pending_step.eq(pending_step),
                operations::last_heartbeat_at.eq(&occurred_at),
                operations::lease_expires_at.eq(Timestamp::after_seconds(300)),
                operations::error_json.eq(metadata.error_json.clone()),
            ))
            .execute(connection)?;
        append_event(
            connection,
            &EventDraft {
                operation_id: *operation_id,
                entity_type: "repo_worktree".to_owned(),
                entity_id: worktree_id.to_string(),
                event_type: metadata.event_type,
                source: metadata.source.clone(),
                occurred_at: occurred_at.clone(),
                previous_state: Some(previous_state.to_string()),
                current_state: Some(state.to_string()),
                details_json: metadata.details_json.clone(),
                error_json: metadata.error_json.clone(),
            },
        )?;
        append_event(
            connection,
            &EventDraft {
                operation_id: *operation_id,
                entity_type: "operation".to_owned(),
                entity_id: operation_id.to_string(),
                event_type: operation_event_type,
                source: metadata.source,
                occurred_at,
                previous_state: Some(OperationState::Running.to_string()),
                current_state: Some(OperationState::Running.to_string()),
                details_json: metadata.details_json,
                error_json: metadata.error_json,
            },
        )?;
        Ok(())
    })
}

pub fn finalize_creation(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    operation_id: &OperationId,
) -> QueryResult<()> {
    with_short_transaction(connection, |connection| {
        let operation = operations::table
            .find(operation_id)
            .select(OperationRow::as_select())
            .first(connection)?;
        let workspace = workspaces::table
            .find(workspace_id)
            .select(WorkspaceRow::as_select())
            .first(connection)?;
        let occurred_at = Timestamp::now();

        diesel::update(operations::table.find(operation_id))
            .set((
                operations::state.eq(OperationState::Succeeded),
                operations::pending_step.eq("complete"),
                operations::last_heartbeat_at.eq(&occurred_at),
                operations::lease_expires_at.eq(&occurred_at),
                operations::finished_at.eq(&occurred_at),
                operations::error_json.eq::<Option<JsonDocument>>(None),
            ))
            .execute(connection)?;
        diesel::update(workspaces::table.find(workspace_id))
            .set((
                workspaces::state.eq(WorkspaceState::Ready),
                workspaces::updated_at.eq(&occurred_at),
                workspaces::last_reconciled_at.eq(&occurred_at),
            ))
            .execute(connection)?;
        append_event(
            connection,
            &EventDraft {
                operation_id: *operation_id,
                entity_type: "operation".to_owned(),
                entity_id: operation.id.to_string(),
                event_type: "operation_succeeded".to_owned(),
                source: "trees".to_owned(),
                occurred_at: occurred_at.clone(),
                previous_state: Some(operation.state.to_string()),
                current_state: Some(OperationState::Succeeded.to_string()),
                details_json: None,
                error_json: None,
            },
        )?;
        append_event(
            connection,
            &EventDraft {
                operation_id: *operation_id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace.id.to_string(),
                event_type: "workspace_ready".to_owned(),
                source: "trees".to_owned(),
                occurred_at,
                previous_state: Some(workspace.state.to_string()),
                current_state: Some(WorkspaceState::Ready.to_string()),
                details_json: None,
                error_json: None,
            },
        )?;
        Ok(())
    })
}

pub fn list_events_for_operation(
    connection: &mut SqliteConnection,
    operation_id: &OperationId,
) -> QueryResult<Vec<EventRow>> {
    lifecycle_events::table
        .filter(lifecycle_events::operation_id.eq(operation_id))
        .order((
            lifecycle_events::occurred_at.asc(),
            lifecycle_events::event_id.asc(),
        ))
        .select(EventRow::as_select())
        .load(connection)
}

pub fn update_workspace_observation(
    connection: &mut SqliteConnection,
    workspace_id: &WorkspaceId,
    state: crate::domain::WorkspaceState,
    updated_at: &Timestamp,
    last_reconciled_at: &Timestamp,
) -> QueryResult<usize> {
    diesel::update(workspaces::table.find(workspace_id))
        .set((
            workspaces::state.eq(state),
            workspaces::updated_at.eq(updated_at),
            workspaces::last_reconciled_at.eq(last_reconciled_at),
        ))
        .execute(connection)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::database;
    use crate::domain::{
        CanonicalPath, EventId, JsonDocument, OperationId, OperationState, RepoWorktreeId,
        RepoWorktreeState, Timestamp, WorkspaceId, WorkspaceState,
    };

    #[test]
    fn repositories_round_trip_typed_rows() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let workspace_id = WorkspaceId::new();
        let now = Timestamp::now();
        let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");

        let workspace = insert_workspace(
            &mut connection,
            &NewWorkspace {
                id: workspace_id,
                canonical_path: workspace_path.clone(),
                state: WorkspaceState::Creating,
                created_at: now.clone(),
                updated_at: now.clone(),
                last_reconciled_at: None,
            },
        )
        .expect("workspace should be inserted");
        assert_eq!(workspace.id, workspace_id);
        assert_eq!(
            find_workspace_by_path(&mut connection, &workspace_path)
                .unwrap()
                .unwrap()
                .id,
            workspace_id
        );

        let worktree = insert_repo_worktree(
            &mut connection,
            &NewRepoWorktree {
                id: RepoWorktreeId::new(),
                workspace_id,
                repository_identity: workspace_path.clone(),
                source_path: workspace_path.clone(),
                worktree_path: CanonicalPath::resolve("/tmp")
                    .expect("temporary path should resolve"),
                state: RepoWorktreeState::Pending,
                last_head: None,
                last_observed_at: now.clone(),
            },
        )
        .expect("repo worktree should be inserted");
        assert_eq!(
            list_repo_worktrees(&mut connection, &workspace_id)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(worktree.workspace_id, workspace_id);

        let operation_id = OperationId::new();
        let operation = insert_operation(
            &mut connection,
            &NewOperation {
                id: operation_id,
                workspace_id,
                kind: "create".to_owned(),
                state: OperationState::Running,
                owner_id: "test-owner".to_owned(),
                lease_expires_at: now.clone(),
                last_heartbeat_at: now.clone(),
                started_at: now.clone(),
                finished_at: None,
                pending_step: "attach".to_owned(),
                intent_json: JsonDocument::parse(r#"{"workspace":"test"}"#).unwrap(),
                error_json: None,
            },
        )
        .expect("operation should be inserted");
        assert_eq!(
            find_running_operation(&mut connection, &workspace_id)
                .unwrap()
                .unwrap()
                .id,
            operation_id
        );

        let event = insert_event(
            &mut connection,
            &NewEvent {
                event_id: EventId::new(),
                operation_id,
                entity_type: "workspace".to_owned(),
                entity_id: workspace_id.to_string(),
                event_type: "created".to_owned(),
                source: "trees".to_owned(),
                occurred_at: now,
                previous_state: None,
                current_state: Some("creating".to_owned()),
                details_json: None,
                error_json: None,
            },
        )
        .expect("event should be inserted");
        assert_eq!(
            list_events_for_operation(&mut connection, &operation.id)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(event.operation_id, operation_id);

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn operation_intent_is_persisted_before_any_workflow_step() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let workspace_id = WorkspaceId::new();
        let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
        let now = Timestamp::now();

        insert_workspace(
            &mut connection,
            &NewWorkspace {
                id: workspace_id,
                canonical_path: workspace_path,
                state: WorkspaceState::Creating,
                created_at: now.clone(),
                updated_at: now,
                last_reconciled_at: None,
            },
        )
        .expect("workspace should be inserted");

        let intent = OperationIntent::new(
            workspace_id,
            "create",
            "test-owner",
            Timestamp::now(),
            "attach repo",
            JsonDocument::parse(r#"{"target":"repo"}"#).unwrap(),
        );
        let operation = persist_operation_intent(&mut connection, &intent)
            .expect("operation intent should be persisted");

        assert_eq!(operation.id, intent.id);
        assert_eq!(operation.state, OperationState::Running);
        assert_eq!(operation.owner_id, "test-owner");
        assert_eq!(operation.pending_step, "attach repo");

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn running_operations_are_exclusive_per_workspace() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let first_workspace_id = WorkspaceId::new();
        let second_workspace_id = WorkspaceId::new();
        let now = Timestamp::now();
        let workspace_paths = [
            (
                first_workspace_id,
                CanonicalPath::resolve(".").expect("workspace path should resolve"),
            ),
            (
                second_workspace_id,
                CanonicalPath::resolve("/tmp").expect("temporary path should resolve"),
            ),
        ];

        for (workspace_id, workspace_path) in workspace_paths {
            insert_workspace(
                &mut connection,
                &NewWorkspace {
                    id: workspace_id,
                    canonical_path: workspace_path,
                    state: WorkspaceState::Creating,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    last_reconciled_at: None,
                },
            )
            .expect("workspace should be inserted");
        }

        let intent = |workspace_id| {
            OperationIntent::new(
                workspace_id,
                "create",
                "test-owner",
                Timestamp::now(),
                "attach repo",
                JsonDocument::parse(r#"{"target":"repo"}"#).unwrap(),
            )
        };

        begin_operation(&mut connection, &intent(first_workspace_id))
            .expect("first operation should start");
        assert!(matches!(
            begin_operation(&mut connection, &intent(first_workspace_id)),
            Err(OperationIntentError::WorkspaceBusy(id)) if id == first_workspace_id
        ));
        begin_operation(&mut connection, &intent(second_workspace_id))
            .expect("different workspace should start concurrently");

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }

    #[test]
    fn transitions_commit_snapshots_and_events_together() {
        let database_path =
            std::env::temp_dir().join(format!("trees-{}.sqlite", WorkspaceId::new()));
        let mut connection = database::connect(&database_path).expect("database should open");
        let workspace_id = WorkspaceId::new();
        let workspace_path = CanonicalPath::resolve(".").expect("workspace path should resolve");
        let now = Timestamp::now();

        insert_workspace(
            &mut connection,
            &NewWorkspace {
                id: workspace_id,
                canonical_path: workspace_path.clone(),
                state: WorkspaceState::Creating,
                created_at: now.clone(),
                updated_at: now.clone(),
                last_reconciled_at: None,
            },
        )
        .expect("workspace should be inserted");
        let operation = begin_operation(
            &mut connection,
            &OperationIntent::new(
                workspace_id,
                "create",
                "test-owner",
                Timestamp::now(),
                "attach repo",
                JsonDocument::parse(r#"{"target":"repo"}"#).unwrap(),
            ),
        )
        .expect("operation should start");
        let worktree = insert_repo_worktree(
            &mut connection,
            &NewRepoWorktree {
                id: RepoWorktreeId::new(),
                workspace_id,
                repository_identity: workspace_path.clone(),
                source_path: workspace_path,
                worktree_path: CanonicalPath::resolve("/tmp")
                    .expect("temporary path should resolve"),
                state: RepoWorktreeState::Pending,
                last_head: None,
                last_observed_at: now,
            },
        )
        .expect("worktree should be inserted");

        record_workspace_transition(
            &mut connection,
            &workspace_id,
            &operation.id,
            WorkspaceState::Ready,
            TransitionMetadata::new("workspace_ready", "trees")
                .with_details(JsonDocument::parse(r#"{"repositories":1}"#).unwrap()),
        )
        .expect("workspace transition should be recorded");
        record_repo_worktree_transition(
            &mut connection,
            &worktree.id,
            &operation.id,
            RepoWorktreeState::Attached,
            Some("abc123".to_owned()),
            TransitionMetadata::new("worktree_attached", "trees"),
        )
        .expect("worktree transition should be recorded");
        record_operation_transition(
            &mut connection,
            &operation.id,
            OperationState::Succeeded,
            "complete",
            None,
            TransitionMetadata::new("operation_succeeded", "trees"),
        )
        .expect("operation transition should be recorded");

        let workspace = find_workspace_by_path(
            &mut connection,
            &CanonicalPath::resolve(".").expect("workspace path should resolve"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(workspace.state, WorkspaceState::Ready);
        assert_eq!(
            list_repo_worktrees(&mut connection, &workspace_id).unwrap()[0].state,
            RepoWorktreeState::Attached
        );
        assert_eq!(
            find_operation(&mut connection, &operation.id)
                .unwrap()
                .state,
            OperationState::Succeeded
        );
        assert_eq!(
            list_events_for_operation(&mut connection, &operation.id)
                .unwrap()
                .len(),
            3
        );

        drop(connection);
        fs::remove_file(database_path).expect("temporary database should be removable");
    }
}
