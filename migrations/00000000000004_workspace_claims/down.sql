PRAGMA foreign_keys = OFF;

ALTER TABLE workspaces
    RENAME COLUMN last_released_at TO last_checked_in_at;

ALTER TABLE workspaces
    RENAME COLUMN pool_id TO pool_key;

ALTER TABLE operations
    ADD COLUMN last_heartbeat_at TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z';

UPDATE operations
SET last_heartbeat_at = started_at;

CREATE TABLE workspace_leases (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL UNIQUE REFERENCES workspaces(id),
    owner_id TEXT NOT NULL,
    checked_out_at TEXT NOT NULL,
    lease_expires_at TEXT NOT NULL,
    last_heartbeat_at TEXT NOT NULL
);

INSERT INTO workspace_leases (
    id,
    workspace_id,
    owner_id,
    checked_out_at,
    lease_expires_at,
    last_heartbeat_at
)
SELECT
    id,
    workspace_id,
    -- Workspace claims do not retain an owner; use a migration marker when
    -- reconstructing the legacy lease shape.
    'migration',
    claimed_at,
    '9999-12-31T23:59:59Z',
    claimed_at
FROM workspace_claims;

DROP TABLE workspace_claims;

CREATE INDEX workspace_leases_expiry_idx
    ON workspace_leases (lease_expires_at);

PRAGMA foreign_keys = ON;
