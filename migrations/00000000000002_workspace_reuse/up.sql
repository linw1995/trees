PRAGMA foreign_keys = OFF;

CREATE TABLE origin_repositories (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    repository_identity TEXT NOT NULL UNIQUE,
    source_path TEXT NOT NULL
);

CREATE TABLE workspace_pools (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    hash_key TEXT NOT NULL,
    repositories_json TEXT NOT NULL CHECK (
        json_valid(repositories_json)
        AND json_type(repositories_json) = 'array'
    ),
    UNIQUE (repositories_json)
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

INSERT INTO origin_repositories (id, repository_identity, source_path)
SELECT MIN(id), repository_identity, MIN(source_path)
FROM repo_worktrees
GROUP BY repository_identity;

CREATE TABLE workspaces_v2 (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    canonical_path TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK (state IN ('creating', 'ready', 'degraded', 'failed', 'reclaimed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_reconciled_at TEXT,
    management_mode TEXT NOT NULL DEFAULT 'manual' CHECK (management_mode IN ('automatic', 'manual')),
    pool_key TEXT REFERENCES workspace_pools(id),
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
    origin_repository_id TEXT NOT NULL REFERENCES origin_repositories(id),
    worktree_path TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'attached', 'dirty', 'missing', 'diverged', 'failed', 'reclaimed')),
    last_head TEXT,
    last_observed_at TEXT NOT NULL,
    UNIQUE (workspace_id, origin_repository_id),
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
    origin_repository_id,
    worktree_path,
    state,
    last_head,
    last_observed_at
)
SELECT
    legacy.id,
    legacy.workspace_id,
    origin.id,
    legacy.worktree_path,
    legacy.state,
    legacy.last_head,
    legacy.last_observed_at
FROM repo_worktrees AS legacy
JOIN origin_repositories AS origin
    ON origin.repository_identity = legacy.repository_identity;

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
