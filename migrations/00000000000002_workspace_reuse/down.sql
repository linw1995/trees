PRAGMA foreign_keys = OFF;

DROP TABLE IF EXISTS workspace_leases;

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
    legacy.id,
    legacy.workspace_id,
    origin.repository_identity,
    origin.source_path,
    legacy.worktree_path,
    CASE
        WHEN legacy.state IN ('dirty', 'reclaimed') THEN 'failed'
        ELSE legacy.state
    END,
    legacy.last_head,
    legacy.last_observed_at
FROM repo_worktrees AS legacy
JOIN origin_repositories AS origin
    ON origin.id = legacy.origin_repository_id;

DROP TABLE workspace_pool_repositories;
DROP TABLE repo_worktrees;
DROP TABLE workspaces;
DROP TABLE workspace_pools;
DROP TABLE origin_repositories;

ALTER TABLE workspaces_v1 RENAME TO workspaces;
ALTER TABLE repo_worktrees_v1 RENAME TO repo_worktrees;

PRAGMA foreign_keys = ON;
