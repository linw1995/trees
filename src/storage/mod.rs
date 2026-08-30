pub mod models;
pub mod repository;
pub mod transaction;

pub use models::{
    EventRow, NewEvent, NewManagedWorkspace, NewOperation, NewRepoWorktree, NewWorkspace,
    NewWorkspaceLease, OperationIntent, OperationRow, RepoWorktreeRow, WorkspaceLeaseRow,
    WorkspaceRow,
};
pub use repository::{
    append_event, begin_operation, claim_expired_operation, finalize_automatic_creation,
    finalize_creation, find_operation, find_running_operation, find_workspace,
    find_workspace_by_path, find_workspace_lease, find_workspace_lease_by_id, insert_event,
    insert_managed_workspace, insert_operation, insert_repo_worktree, insert_workspace,
    insert_workspace_lease, list_automatic_workspace_candidates, list_events_for_operation,
    list_repo_worktrees, persist_operation_intent, persist_operation_step_intent,
    record_operation_transition, record_repo_worktree_transition, record_workspace_checkin,
    record_workspace_checkin_rejection, record_workspace_checkout,
    record_workspace_checkout_failure, record_workspace_lease_expiration_failure,
    record_workspace_lease_reclaim, record_workspace_lease_renewal, record_workspace_reclaimed,
    record_workspace_transition, record_worktree_step_result, release_workspace_lease,
    renew_operation_lease, renew_workspace_lease, update_workspace_observation, EventDraft,
    OperationIntentError, TransitionMetadata,
};
pub use transaction::with_short_transaction;
