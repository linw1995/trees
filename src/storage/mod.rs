pub mod models;
pub mod repository;
pub mod transaction;

pub use models::{
    EventRow, LeasedOperation, NewEvent, NewManagedWorkspace, NewOperation, NewOperationLease,
    NewOriginRepository, NewRepoWorktree, NewWorkspace, NewWorkspaceClaim, NewWorkspacePool,
    NewWorkspacePoolRepository, OperationIntent, OperationLeaseRow, OperationRow,
    OriginRepositoryRow, PoolOriginRepository, RepoWorktreeRow, RunningOperation,
    WorkspaceClaimRow, WorkspacePoolRepositoryRow, WorkspacePoolRow, WorkspaceRow,
};
pub use repository::{
    append_event, begin_operation, claim_expired_operation, ensure_origin_repository,
    ensure_workspace_pool, finalize_automatic_creation, finalize_creation,
    finalize_recovered_creation, find_latest_operation_event, find_operation, find_operation_lease,
    find_origin_repository_by_identity, find_running_operation, find_workspace,
    find_workspace_by_path, find_workspace_claim, find_workspace_claim_by_id, find_workspace_pool,
    find_workspace_pool_by_id, insert_event, insert_managed_workspace, insert_operation,
    insert_origin_repository, insert_repo_worktree, insert_workspace, insert_workspace_claim,
    insert_workspace_pool, insert_workspace_pool_repositories, list_automatic_workspace_candidates,
    list_automatic_workspaces, list_current_automatic_workspaces, list_events_for_operation,
    list_leased_operations, list_operation_events, list_operation_leases_for_workspaces,
    list_pool_origin_repositories, list_repo_worktrees, list_repo_worktrees_for_workspaces,
    list_workspace_claims, list_workspace_pool_repositories, list_workspaces, operation_state,
    persist_operation_intent, persist_operation_step_intent, record_operation_transition,
    record_repo_worktree_transition, record_workspace_acquire, record_workspace_acquire_failure,
    record_workspace_gc_failure, record_workspace_gc_skipped, record_workspace_reclaimed,
    record_workspace_release, record_workspace_release_rejection, record_workspace_transition,
    record_worktree_step_result, release_workspace_claim, renew_operation_lease,
    try_begin_operation, update_workspace_observation, EventDraft, OperationIntentError,
    TransitionMetadata,
};
pub use transaction::{
    with_immediate_transaction, with_retrying_short_transaction, with_short_transaction,
    with_trying_short_transaction,
};
