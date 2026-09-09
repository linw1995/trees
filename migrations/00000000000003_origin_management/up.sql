ALTER TABLE origin_repositories ADD COLUMN registered BOOLEAN NOT NULL DEFAULT 1 CHECK (registered IN (0, 1));
ALTER TABLE origin_repositories ADD COLUMN management_mode TEXT NOT NULL DEFAULT 'manual' CHECK (management_mode IN ('manual', 'automatic'));
ALTER TABLE origin_repositories ADD COLUMN managed_root TEXT;
ALTER TABLE origin_repositories ADD COLUMN remote_url TEXT CHECK (
    (management_mode = 'manual' AND managed_root IS NULL AND remote_url IS NULL)
    OR (management_mode = 'automatic' AND managed_root IS NOT NULL AND remote_url IS NOT NULL)
);
CREATE UNIQUE INDEX origin_remote_url ON origin_repositories(remote_url);
CREATE TABLE pending_origin_clones (
    id TEXT PRIMARY KEY NOT NULL,
    remote_url TEXT NOT NULL UNIQUE,
    managed_root TEXT NOT NULL,
    source_path TEXT NOT NULL,
    ownership_token TEXT NOT NULL
);
