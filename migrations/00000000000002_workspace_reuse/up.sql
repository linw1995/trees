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
    ),
    UNIQUE (repository_ids)
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

ALTER TABLE operations
    DROP COLUMN last_heartbeat_at;

PRAGMA foreign_keys = ON;
