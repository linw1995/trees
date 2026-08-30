PRAGMA foreign_keys = OFF;

ALTER TABLE lifecycle_events RENAME TO lifecycle_events_v1;
ALTER TABLE operations RENAME TO operations_v1;
ALTER TABLE repo_worktrees RENAME TO repo_worktrees_v1;
ALTER TABLE workspaces RENAME TO workspaces_v1;

DROP INDEX IF EXISTS operations_workspace_state_idx;
DROP INDEX IF EXISTS operations_lease_expiry_idx;
DROP INDEX IF EXISTS operations_one_running_per_workspace_idx;
DROP INDEX IF EXISTS lifecycle_events_operation_time_idx;
DROP INDEX IF EXISTS lifecycle_events_entity_time_idx;
DROP INDEX IF EXISTS lifecycle_events_time_idx;
DROP TRIGGER IF EXISTS lifecycle_events_immutable_delete;
DROP TRIGGER IF EXISTS lifecycle_events_immutable_update;

CREATE TABLE workspaces (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    canonical_path TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK (state IN ('creating', 'ready', 'degraded', 'failed', 'reclaimed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_reconciled_at TEXT,
    management_mode TEXT NOT NULL DEFAULT 'manual' CHECK (management_mode IN ('automatic', 'manual')),
    pool_key TEXT,
    workspace_root TEXT,
    last_checked_in_at TEXT,
    reclaimed_at TEXT,
    CHECK (
        management_mode = 'manual'
        OR (pool_key IS NOT NULL AND workspace_root IS NOT NULL)
    )
);

CREATE TABLE repo_worktrees (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    repository_identity TEXT NOT NULL,
    source_path TEXT NOT NULL,
    worktree_path TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'attached', 'dirty', 'missing', 'diverged', 'failed', 'reclaimed')),
    last_head TEXT,
    last_observed_at TEXT NOT NULL,
    UNIQUE (workspace_id, repository_identity),
    UNIQUE (workspace_id, worktree_path)
);

CREATE TABLE operations (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    kind TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('running', 'succeeded', 'failed', 'rolled_back')),
    owner_id TEXT NOT NULL,
    lease_expires_at TEXT NOT NULL,
    last_heartbeat_at TEXT NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    pending_step TEXT NOT NULL,
    intent_json TEXT NOT NULL CHECK (json_valid(intent_json)),
    error_json TEXT CHECK (error_json IS NULL OR json_valid(error_json))
);

CREATE TABLE lifecycle_events (
    event_id TEXT NOT NULL PRIMARY KEY CHECK (length(event_id) = 36),
    operation_id TEXT NOT NULL REFERENCES operations(id),
    entity_type TEXT NOT NULL CHECK (entity_type IN ('workspace', 'repo_worktree', 'operation')),
    entity_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    source TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    previous_state TEXT,
    current_state TEXT,
    details_json TEXT CHECK (details_json IS NULL OR json_valid(details_json)),
    error_json TEXT CHECK (error_json IS NULL OR json_valid(error_json))
);

CREATE TABLE workspace_leases (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL UNIQUE REFERENCES workspaces(id),
    owner_id TEXT NOT NULL,
    checked_out_at TEXT NOT NULL,
    lease_expires_at TEXT NOT NULL,
    last_heartbeat_at TEXT NOT NULL
);

INSERT INTO workspaces (
    id,
    canonical_path,
    state,
    created_at,
    updated_at,
    last_reconciled_at,
    management_mode,
    pool_key,
    workspace_root,
    last_checked_in_at,
    reclaimed_at
)
SELECT
    id,
    canonical_path,
    state,
    created_at,
    updated_at,
    last_reconciled_at,
    'manual',
    NULL,
    NULL,
    NULL,
    NULL
FROM workspaces_v1;

INSERT INTO repo_worktrees (
    id,
    workspace_id,
    repository_identity,
    source_path,
    worktree_path,
    state,
    last_head,
    last_observed_at
)
SELECT
    id,
    workspace_id,
    repository_identity,
    source_path,
    worktree_path,
    state,
    last_head,
    last_observed_at
FROM repo_worktrees_v1;

INSERT INTO operations (
    id,
    workspace_id,
    kind,
    state,
    owner_id,
    lease_expires_at,
    last_heartbeat_at,
    started_at,
    finished_at,
    pending_step,
    intent_json,
    error_json
)
SELECT
    id,
    workspace_id,
    kind,
    state,
    owner_id,
    lease_expires_at,
    last_heartbeat_at,
    started_at,
    finished_at,
    pending_step,
    intent_json,
    error_json
FROM operations_v1;

INSERT INTO lifecycle_events (
    event_id,
    operation_id,
    entity_type,
    entity_id,
    event_type,
    source,
    occurred_at,
    previous_state,
    current_state,
    details_json,
    error_json
)
SELECT
    event_id,
    operation_id,
    entity_type,
    entity_id,
    event_type,
    source,
    occurred_at,
    previous_state,
    current_state,
    details_json,
    error_json
FROM lifecycle_events_v1;

DROP TABLE lifecycle_events_v1;
DROP TABLE operations_v1;
DROP TABLE repo_worktrees_v1;
DROP TABLE workspaces_v1;

CREATE INDEX operations_workspace_state_idx
    ON operations (workspace_id, state);

CREATE INDEX operations_lease_expiry_idx
    ON operations (lease_expires_at);

CREATE UNIQUE INDEX operations_one_running_per_workspace_idx
    ON operations (workspace_id)
    WHERE state = 'running';

CREATE INDEX lifecycle_events_operation_time_idx
    ON lifecycle_events (operation_id, occurred_at);

CREATE INDEX lifecycle_events_entity_time_idx
    ON lifecycle_events (entity_type, entity_id, occurred_at);

CREATE INDEX lifecycle_events_time_idx
    ON lifecycle_events (occurred_at);

CREATE INDEX workspaces_pool_lookup_idx
    ON workspaces (management_mode, workspace_root, pool_key, last_checked_in_at, created_at);

CREATE INDEX workspace_leases_expiry_idx
    ON workspace_leases (lease_expires_at);

CREATE TRIGGER lifecycle_events_immutable_update
BEFORE UPDATE ON lifecycle_events
BEGIN
    SELECT RAISE(ABORT, 'lifecycle_events are immutable');
END;

CREATE TRIGGER lifecycle_events_immutable_delete
BEFORE DELETE ON lifecycle_events
BEGIN
    SELECT RAISE(ABORT, 'lifecycle_events are immutable');
END;

PRAGMA foreign_keys = ON;
