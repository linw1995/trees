PRAGMA foreign_keys = OFF;

CREATE TABLE origin_repositories (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    repository_identity TEXT NOT NULL UNIQUE,
    source_path TEXT NOT NULL
);

INSERT INTO origin_repositories (id, repository_identity, source_path)
SELECT MIN(id), repository_identity, MIN(source_path)
FROM repo_worktrees
GROUP BY repository_identity;

CREATE TABLE workspace_pools (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    hash_key TEXT NOT NULL,
    repository_ids TEXT NOT NULL CHECK (
        json_valid(repository_ids)
        AND json_type(repository_ids) = 'array'
    )
);

CREATE INDEX workspace_pools_hash_idx
    ON workspace_pools (hash_key);

CREATE TABLE workspace_pool_repositories (
    pool_id TEXT NOT NULL REFERENCES workspace_pools(id),
    repository_id TEXT NOT NULL REFERENCES origin_repositories(id),
    PRIMARY KEY (pool_id, repository_id)
);

CREATE INDEX workspace_pool_repositories_identity_idx
    ON workspace_pool_repositories (repository_id);

CREATE TABLE workspaces_v2 (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    canonical_path TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK (
        state IN ('creating', 'ready', 'degraded', 'failed', 'reclaimed')
    ),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_reconciled_at TEXT,
    management_mode TEXT NOT NULL DEFAULT 'manual' CHECK (
        management_mode IN ('automatic', 'manual')
    ),
    pool_id TEXT REFERENCES workspace_pools(id),
    last_released_at TEXT,
    reclaimed_at TEXT,
    CHECK (
        management_mode = 'manual'
        OR pool_id IS NOT NULL
    )
);

INSERT INTO workspaces_v2 (
    id,
    canonical_path,
    state,
    created_at,
    updated_at,
    last_reconciled_at,
    management_mode,
    pool_id,
    last_released_at,
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
    NULL
FROM workspaces;

CREATE TABLE repo_worktrees_v2 (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    origin_repository_id TEXT NOT NULL REFERENCES origin_repositories(id),
    worktree_path TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN ('pending', 'attached', 'dirty', 'missing', 'diverged', 'failed', 'reclaimed')
    ),
    last_head TEXT,
    last_observed_at TEXT NOT NULL,
    UNIQUE (workspace_id, origin_repository_id),
    UNIQUE (workspace_id, worktree_path)
);

INSERT INTO repo_worktrees_v2 (
    id,
    workspace_id,
    origin_repository_id,
    worktree_path,
    state,
    last_head,
    last_observed_at
)
SELECT
    repository.id,
    repository.workspace_id,
    origin.id,
    repository.worktree_path,
    repository.state,
    repository.last_head,
    repository.last_observed_at
FROM repo_worktrees AS repository
JOIN origin_repositories AS origin
  ON origin.repository_identity = repository.repository_identity;

DROP TABLE repo_worktrees;
DROP TABLE workspaces;

ALTER TABLE workspaces_v2 RENAME TO workspaces;
ALTER TABLE repo_worktrees_v2 RENAME TO repo_worktrees;

CREATE INDEX workspaces_pool_lookup_idx
    ON workspaces (management_mode, pool_id, last_released_at, created_at);

CREATE TABLE workspace_claims (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL UNIQUE REFERENCES workspaces(id),
    claimed_at TEXT NOT NULL
);

CREATE TABLE operations_legacy AS
SELECT
    id,
    workspace_id,
    kind,
    state,
    lease_expires_at,
    started_at,
    finished_at,
    pending_step,
    intent_json,
    error_json
FROM operations;

CREATE TABLE operations_v2 (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    kind TEXT NOT NULL,
    started_at TEXT NOT NULL,
    intent_json TEXT NOT NULL CHECK (json_valid(intent_json))
);

INSERT INTO operations_v2 (
    id,
    workspace_id,
    kind,
    started_at,
    intent_json
)
SELECT
    id,
    workspace_id,
    kind,
    started_at,
    intent_json
FROM operations_legacy;

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
    legacy.id,
    legacy.id,
    'operation',
    legacy.id,
    CASE
        WHEN legacy.state = 'running' THEN 'operation_started'
        ELSE 'operation_migrated'
    END,
    'migration',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    NULL,
    legacy.state,
    json_object('pending_step', legacy.pending_step),
    legacy.error_json
FROM operations_legacy AS legacy
WHERE NOT EXISTS (
    SELECT 1
    FROM lifecycle_events AS event
    WHERE event.operation_id = legacy.id
      AND event.entity_type = 'operation'
)
  AND NOT EXISTS (
    SELECT 1
    FROM lifecycle_events AS event
    WHERE event.event_id = legacy.id
);

DROP TABLE operations;
ALTER TABLE operations_v2 RENAME TO operations;

CREATE TABLE operation_leases (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    operation_id TEXT NOT NULL UNIQUE REFERENCES operations(id),
    workspace_id TEXT NOT NULL UNIQUE REFERENCES workspaces(id),
    lease_expires_at TEXT NOT NULL
);

INSERT INTO operation_leases (
    id,
    operation_id,
    workspace_id,
    lease_expires_at
)
SELECT
    id,
    id,
    workspace_id,
    lease_expires_at
FROM operations_legacy
WHERE state = 'running';

DROP TABLE operations_legacy;

CREATE INDEX operation_leases_expiry_idx
    ON operation_leases (lease_expires_at);

CREATE TRIGGER operations_immutable_update
BEFORE UPDATE ON operations
BEGIN
    SELECT RAISE(ABORT, 'operations are immutable');
END;

CREATE TRIGGER operations_immutable_delete
BEFORE DELETE ON operations
BEGIN
    SELECT RAISE(ABORT, 'operations are immutable');
END;

PRAGMA foreign_keys = ON;
