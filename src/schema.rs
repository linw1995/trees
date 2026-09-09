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
    workspace_pools (id) {
        id -> Text,
        hash_key -> Text,
        repository_ids -> Text,
    }
}

diesel::table! {
    workspace_claims (id) {
        id -> Text,
        workspace_id -> Text,
        claimed_at -> Text,
    }
}

diesel::table! {
    workspace_pool_repositories (pool_id, repository_id) {
        pool_id -> Text,
        repository_id -> Text,
    }
}

diesel::table! {
    origin_repositories (id) {
        id -> Text,
        repository_identity -> Text,
        source_path -> Text,
    }
}

diesel::table! {
    operations (id) {
        id -> Text,
        workspace_id -> Text,
        kind -> Text,
        started_at -> Text,
        intent_json -> Text,
    }
}

diesel::table! {
    operation_leases (id) {
        id -> Text,
        operation_id -> Text,
        workspace_id -> Text,
        lease_expires_at -> Text,
    }
}

diesel::table! {
    repo_worktrees (id) {
        id -> Text,
        workspace_id -> Text,
        origin_repository_id -> Text,
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
        pool_id -> Nullable<Text>,
        last_released_at -> Nullable<Text>,
        reclaimed_at -> Nullable<Text>,
    }
}

diesel::joinable!(lifecycle_events -> operations (operation_id));
diesel::joinable!(operation_leases -> operations (operation_id));
diesel::joinable!(operation_leases -> workspaces (workspace_id));
diesel::joinable!(operations -> workspaces (workspace_id));
diesel::joinable!(repo_worktrees -> workspaces (workspace_id));
diesel::joinable!(repo_worktrees -> origin_repositories (origin_repository_id));
diesel::joinable!(workspace_claims -> workspaces (workspace_id));
diesel::joinable!(workspace_pool_repositories -> workspace_pools (pool_id));
diesel::joinable!(workspace_pool_repositories -> origin_repositories (repository_id));
diesel::joinable!(workspaces -> workspace_pools (pool_id));

diesel::allow_tables_to_appear_in_same_query!(
    lifecycle_events,
    origin_repositories,
    operations,
    operation_leases,
    repo_worktrees,
    workspace_claims,
    workspace_pool_repositories,
    workspace_pools,
    workspaces,
);

diesel::table! {
    pending_origin_clones (id) {
        id -> Text,
        remote_url -> Text,
        managed_root -> Text,
        source_path -> Text,
        ownership_token -> Text,
    }
}
