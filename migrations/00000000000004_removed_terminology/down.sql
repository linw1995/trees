PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;

CREATE TABLE workspaces_renamed (
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

CREATE TABLE repo_worktrees_renamed (
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

INSERT INTO workspaces_renamed
SELECT id, canonical_path, CASE state WHEN 'removed' THEN 'reclaimed' ELSE state END,
       created_at, updated_at, last_reconciled_at, management_mode, pool_id,
       last_released_at, removed_at
FROM workspaces;

INSERT INTO repo_worktrees_renamed
SELECT id, workspace_id, origin_repository_id, worktree_path,
       CASE state WHEN 'removed' THEN 'reclaimed' ELSE state END,
       last_head, last_observed_at
FROM repo_worktrees;

DROP TABLE repo_worktrees;
DROP TABLE workspaces;
ALTER TABLE workspaces_renamed RENAME TO workspaces;
ALTER TABLE repo_worktrees_renamed RENAME TO repo_worktrees;
CREATE INDEX workspaces_pool_lookup_idx
    ON workspaces (management_mode, pool_id, last_released_at, created_at);

COMMIT;
PRAGMA foreign_keys = ON;
