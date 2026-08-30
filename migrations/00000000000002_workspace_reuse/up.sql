PRAGMA foreign_keys = OFF;

CREATE TABLE workspaces_v2 (
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

CREATE TABLE repo_worktrees_v2 (
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

INSERT INTO workspaces_v2 (
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
FROM workspaces;

INSERT INTO repo_worktrees_v2 (
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
FROM repo_worktrees;

DROP TABLE repo_worktrees;
DROP TABLE workspaces;

ALTER TABLE workspaces_v2 RENAME TO workspaces;
ALTER TABLE repo_worktrees_v2 RENAME TO repo_worktrees;

CREATE TABLE workspace_leases (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL UNIQUE REFERENCES workspaces(id),
    owner_id TEXT NOT NULL,
    checked_out_at TEXT NOT NULL,
    lease_expires_at TEXT NOT NULL,
    last_heartbeat_at TEXT NOT NULL
);

CREATE INDEX workspaces_pool_lookup_idx
    ON workspaces (management_mode, workspace_root, pool_key, last_checked_in_at, created_at);

CREATE INDEX workspace_leases_expiry_idx
    ON workspace_leases (lease_expires_at);

PRAGMA foreign_keys = ON;
