PRAGMA foreign_keys = OFF;

CREATE TABLE workspace_claims (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL UNIQUE REFERENCES workspaces(id),
    owner_id TEXT NOT NULL,
    claimed_at TEXT NOT NULL
);

INSERT INTO workspace_claims (
    id,
    workspace_id,
    owner_id,
    claimed_at
)
SELECT
    id,
    workspace_id,
    owner_id,
    checked_out_at
FROM workspace_leases;

DROP TABLE workspace_leases;

CREATE INDEX workspace_claims_workspace_idx
    ON workspace_claims (workspace_id);

PRAGMA foreign_keys = ON;
