PRAGMA foreign_keys = OFF;

CREATE TABLE operations_v1 (
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

WITH latest_operation_events AS (
    SELECT
        event.operation_id,
        event.current_state,
        event.occurred_at,
        event.details_json,
        event.error_json
    FROM lifecycle_events AS event
    WHERE event.entity_type = 'operation'
      AND NOT EXISTS (
          SELECT 1
          FROM lifecycle_events AS newer
          WHERE newer.entity_type = 'operation'
            AND newer.operation_id = event.operation_id
            AND (
                newer.occurred_at > event.occurred_at
                OR (
                    newer.occurred_at = event.occurred_at
                    AND newer.event_id > event.event_id
                )
            )
      )
)
INSERT INTO operations_v1 (
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
    operation.id,
    operation.workspace_id,
    operation.kind,
    COALESCE(latest.current_state, 'failed'),
    COALESCE(lease.id, 'migration'),
    COALESCE(lease.lease_expires_at, latest.occurred_at, operation.started_at),
    COALESCE(latest.occurred_at, operation.started_at),
    operation.started_at,
    CASE
        WHEN latest.current_state IN ('succeeded', 'failed', 'rolled_back')
            THEN latest.occurred_at
        ELSE NULL
    END,
    COALESCE(json_extract(latest.details_json, '$.pending_step'), 'downgraded'),
    operation.intent_json,
    latest.error_json
FROM operations AS operation
LEFT JOIN operation_leases AS lease
  ON lease.operation_id = operation.id
LEFT JOIN latest_operation_events AS latest
  ON latest.operation_id = operation.id;

DROP TABLE operation_leases;
DROP TABLE operations;
ALTER TABLE operations_v1 RENAME TO operations;

CREATE TABLE workspaces_v1 (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    canonical_path TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK (state IN ('creating', 'ready', 'degraded', 'failed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_reconciled_at TEXT
);

CREATE TABLE repo_worktrees_v1 (
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

INSERT INTO workspaces_v1 (
    id,
    canonical_path,
    state,
    created_at,
    updated_at,
    last_reconciled_at
)
SELECT
    id,
    canonical_path,
    CASE state WHEN 'reclaimed' THEN 'failed' ELSE state END,
    created_at,
    updated_at,
    last_reconciled_at
FROM workspaces;

INSERT INTO repo_worktrees_v1 (
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
    repository.id,
    repository.workspace_id,
    origin.repository_identity,
    origin.source_path,
    repository.worktree_path,
    CASE
        WHEN repository.state IN ('dirty', 'reclaimed') THEN 'failed'
        ELSE repository.state
    END,
    repository.last_head,
    repository.last_observed_at
FROM repo_worktrees AS repository
JOIN origin_repositories AS origin
  ON origin.id = repository.origin_repository_id;

DROP TABLE repo_worktrees;
DROP TABLE workspaces;
DROP TABLE workspace_claims;
DROP TABLE workspace_pool_repositories;
DROP TABLE workspace_pools;
DROP TABLE origin_repositories;

ALTER TABLE workspaces_v1 RENAME TO workspaces;
ALTER TABLE repo_worktrees_v1 RENAME TO repo_worktrees;

PRAGMA foreign_keys = ON;
