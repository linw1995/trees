PRAGMA foreign_keys = OFF;

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

ALTER TABLE workspaces
    RENAME COLUMN last_checked_in_at TO last_released_at;

ALTER TABLE workspaces
    RENAME COLUMN pool_key TO pool_id;

ALTER TABLE operations
    DROP COLUMN last_heartbeat_at;

PRAGMA foreign_keys = ON;
