CREATE TABLE pending_origin_clones (
    id TEXT PRIMARY KEY NOT NULL,
    remote_url TEXT NOT NULL UNIQUE,
    managed_root TEXT NOT NULL,
    source_path TEXT NOT NULL,
    ownership_token TEXT NOT NULL
);
