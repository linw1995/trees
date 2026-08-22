CREATE TABLE workspaces (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    canonical_path TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK (state IN ('creating', 'ready', 'degraded', 'failed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_reconciled_at TEXT
);

CREATE TABLE repo_worktrees (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    repository_identity TEXT NOT NULL,
    source_path TEXT NOT NULL,
    worktree_path TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'attached', 'missing', 'diverged', 'failed')),
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
