// @generated automatically by Diesel CLI.

diesel::table! {
    lifecycle_events (event_id) {
        event_id -> Text,
        operation_id -> Text,
        entity_type -> Text,
        entity_id -> Text,
        event_type -> Text,
        source -> Text,
        occurred_at -> Text,
        previous_state -> Nullable<Text>,
        current_state -> Nullable<Text>,
        details_json -> Nullable<Text>,
        error_json -> Nullable<Text>,
    }
}

diesel::table! {
    workspace_leases (id) {
        id -> Text,
        workspace_id -> Text,
        owner_id -> Text,
        checked_out_at -> Text,
        lease_expires_at -> Text,
        last_heartbeat_at -> Text,
    }
}

diesel::table! {
    operations (id) {
        id -> Text,
        workspace_id -> Text,
        kind -> Text,
        state -> Text,
        owner_id -> Text,
        lease_expires_at -> Text,
        last_heartbeat_at -> Text,
        started_at -> Text,
        finished_at -> Nullable<Text>,
        pending_step -> Text,
        intent_json -> Text,
        error_json -> Nullable<Text>,
    }
}

diesel::table! {
    repo_worktrees (id) {
        id -> Text,
        workspace_id -> Text,
        repository_identity -> Text,
        source_path -> Text,
        worktree_path -> Text,
        state -> Text,
        last_head -> Nullable<Text>,
        last_observed_at -> Text,
    }
}

diesel::table! {
    workspaces (id) {
        id -> Text,
        canonical_path -> Text,
        state -> Text,
        created_at -> Text,
        updated_at -> Text,
        last_reconciled_at -> Nullable<Text>,
        management_mode -> Text,
        pool_key -> Nullable<Text>,
        workspace_root -> Nullable<Text>,
        last_checked_in_at -> Nullable<Text>,
        reclaimed_at -> Nullable<Text>,
    }
}

diesel::joinable!(lifecycle_events -> operations (operation_id));
diesel::joinable!(operations -> workspaces (workspace_id));
diesel::joinable!(repo_worktrees -> workspaces (workspace_id));
diesel::joinable!(workspace_leases -> workspaces (workspace_id));

diesel::allow_tables_to_appear_in_same_query!(
    lifecycle_events,
    operations,
    repo_worktrees,
    workspace_leases,
    workspaces,
);
