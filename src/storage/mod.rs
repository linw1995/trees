pub mod models;
pub mod repository;
pub mod transaction;

pub use models::{
    EventRow, NewEvent, NewOperation, NewRepoWorktree, NewWorkspace, OperationIntent, OperationRow,
    RepoWorktreeRow, WorkspaceRow,
};
pub use repository::{
    append_event, begin_operation, claim_expired_operation, finalize_creation, find_operation,
    find_running_operation, find_workspace, find_workspace_by_path, insert_event, insert_operation,
    insert_repo_worktree, insert_workspace, list_events_for_operation, list_repo_worktrees,
    persist_operation_intent, persist_operation_step_intent, record_operation_transition,
    record_repo_worktree_transition, record_workspace_transition, record_worktree_step_result,
    update_workspace_observation, EventDraft, OperationIntentError, TransitionMetadata,
};
pub use transaction::with_short_transaction;
