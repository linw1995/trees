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

CREATE TABLE workspace_pool_map (
    workspace_id TEXT NOT NULL PRIMARY KEY,
    pool_id TEXT NOT NULL
);

WITH workspace_sets AS (
    SELECT
        workspace.id AS workspace_id,
        workspace.pool_key AS hash_key,
        (
            SELECT json_group_array(repository_id)
            FROM (
                SELECT DISTINCT origin.id AS repository_id
                FROM repo_worktrees AS repository
                JOIN origin_repositories AS origin
                  ON origin.repository_identity = repository.repository_identity
                WHERE repository.workspace_id = workspace.id
                ORDER BY origin.id
            )
        ) AS repository_ids
    FROM workspaces AS workspace
    WHERE workspace.management_mode = 'automatic'
      AND workspace.pool_key IS NOT NULL
), pool_ids AS (
    SELECT
        MIN(workspace_id) AS pool_id,
        repository_ids
    FROM workspace_sets
    GROUP BY repository_ids
)
INSERT INTO workspace_pools (id, hash_key, repository_ids)
SELECT
    pool_ids.pool_id,
    MIN(workspace_sets.hash_key),
    pool_ids.repository_ids
FROM pool_ids
JOIN workspace_sets
  ON workspace_sets.repository_ids = pool_ids.repository_ids
GROUP BY pool_ids.pool_id, pool_ids.repository_ids;

WITH workspace_sets AS (
    SELECT
        workspace.id AS workspace_id,
        (
            SELECT json_group_array(repository_id)
            FROM (
                SELECT DISTINCT origin.id AS repository_id
                FROM repo_worktrees AS repository
                JOIN origin_repositories AS origin
                  ON origin.repository_identity = repository.repository_identity
                WHERE repository.workspace_id = workspace.id
                ORDER BY origin.id
            )
        ) AS repository_ids
    FROM workspaces AS workspace
    WHERE workspace.management_mode = 'automatic'
      AND workspace.pool_key IS NOT NULL
), pool_ids AS (
    SELECT
        MIN(workspace_id) AS pool_id,
        repository_ids
    FROM workspace_sets
    GROUP BY repository_ids
)
INSERT INTO workspace_pool_map (workspace_id, pool_id)
SELECT workspace_sets.workspace_id, pool_ids.pool_id
FROM workspace_sets
JOIN pool_ids
  ON pool_ids.repository_ids = workspace_sets.repository_ids;

CREATE TABLE workspace_pool_repositories (
    pool_id TEXT NOT NULL REFERENCES workspace_pools(id),
    repository_id TEXT NOT NULL REFERENCES origin_repositories(id),
    PRIMARY KEY (pool_id, repository_id)
);

CREATE INDEX workspace_pool_repositories_identity_idx
    ON workspace_pool_repositories (repository_id);

INSERT INTO workspace_pool_repositories (pool_id, repository_id)
SELECT DISTINCT map.pool_id, origin.id
FROM workspace_pool_map AS map
JOIN repo_worktrees AS repository
  ON repository.workspace_id = map.workspace_id
JOIN origin_repositories AS origin
  ON origin.repository_identity = repository.repository_identity;

CREATE TABLE workspaces_v3 (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    canonical_path TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK (state IN ('creating', 'ready', 'degraded', 'failed', 'reclaimed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_reconciled_at TEXT,
    management_mode TEXT NOT NULL DEFAULT 'manual' CHECK (management_mode IN ('automatic', 'manual')),
    pool_id TEXT REFERENCES workspace_pools(id),
    last_released_at TEXT,
    reclaimed_at TEXT,
    CHECK (
        management_mode = 'manual'
        OR pool_id IS NOT NULL
    )
);

INSERT INTO workspaces_v3 (
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
    workspace.id,
    workspace.canonical_path,
    workspace.state,
    workspace.created_at,
    workspace.updated_at,
    workspace.last_reconciled_at,
    workspace.management_mode,
    map.pool_id,
    workspace.last_checked_in_at,
    workspace.reclaimed_at
FROM workspaces AS workspace
LEFT JOIN workspace_pool_map AS map
  ON map.workspace_id = workspace.id;

CREATE TABLE repo_worktrees_v3 (
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

INSERT INTO repo_worktrees_v3 (
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
DROP TABLE workspace_pool_map;

ALTER TABLE workspaces_v3 RENAME TO workspaces;
ALTER TABLE repo_worktrees_v3 RENAME TO repo_worktrees;

CREATE INDEX workspaces_pool_lookup_idx
    ON workspaces (management_mode, pool_id, last_released_at, created_at);

CREATE TABLE workspace_claims (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL UNIQUE REFERENCES workspaces(id),
    claimed_at TEXT NOT NULL
);

INSERT INTO workspace_claims (
    id,
    workspace_id,
    claimed_at
)
SELECT
    id,
    workspace_id,
    checked_out_at
FROM workspace_leases;

DROP TABLE workspace_leases;

ALTER TABLE operations
    DROP COLUMN last_heartbeat_at;

PRAGMA foreign_keys = ON;
