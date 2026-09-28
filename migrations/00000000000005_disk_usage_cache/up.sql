ALTER TABLE workspaces
ADD COLUMN disk_usage_json TEXT
CHECK (disk_usage_json IS NULL OR json_valid(disk_usage_json));

ALTER TABLE repo_worktrees
ADD COLUMN disk_usage_json TEXT
CHECK (disk_usage_json IS NULL OR json_valid(disk_usage_json));

ALTER TABLE origin_repositories
ADD COLUMN disk_usage_json TEXT
CHECK (disk_usage_json IS NULL OR json_valid(disk_usage_json));

CREATE TRIGGER workspace_disk_usage_path_changed
AFTER UPDATE OF canonical_path ON workspaces
WHEN NEW.canonical_path != OLD.canonical_path
BEGIN
    UPDATE workspaces SET disk_usage_json = NULL WHERE id = NEW.id;
END;

CREATE TRIGGER worktree_disk_usage_path_changed
AFTER UPDATE OF worktree_path ON repo_worktrees
WHEN NEW.worktree_path != OLD.worktree_path
BEGIN
    UPDATE repo_worktrees SET disk_usage_json = NULL WHERE id = NEW.id;
END;

CREATE TRIGGER origin_disk_usage_path_changed
AFTER UPDATE OF source_path ON origin_repositories
WHEN NEW.source_path != OLD.source_path
BEGIN
    UPDATE origin_repositories SET disk_usage_json = NULL WHERE id = NEW.id;
END;

CREATE TRIGGER workspace_disk_usage_removed
AFTER UPDATE OF state ON workspaces
WHEN NEW.state = 'removed' AND OLD.state != 'removed'
BEGIN
    UPDATE workspaces SET disk_usage_json = NULL WHERE id = NEW.id;
    UPDATE repo_worktrees SET disk_usage_json = NULL WHERE workspace_id = NEW.id;
END;

CREATE TRIGGER worktree_disk_usage_removed
AFTER UPDATE OF state ON repo_worktrees
WHEN NEW.state = 'removed' AND OLD.state != 'removed'
BEGIN
    UPDATE repo_worktrees SET disk_usage_json = NULL WHERE id = NEW.id;
    UPDATE workspaces SET disk_usage_json = NULL WHERE id = NEW.workspace_id;
END;
