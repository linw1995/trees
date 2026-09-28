DROP TRIGGER IF EXISTS worktree_disk_usage_removed;
DROP TRIGGER IF EXISTS workspace_disk_usage_removed;
DROP TRIGGER IF EXISTS origin_disk_usage_path_changed;
DROP TRIGGER IF EXISTS worktree_disk_usage_path_changed;
DROP TRIGGER IF EXISTS workspace_disk_usage_path_changed;

ALTER TABLE origin_repositories DROP COLUMN disk_usage_json;
ALTER TABLE repo_worktrees DROP COLUMN disk_usage_json;
ALTER TABLE workspaces DROP COLUMN disk_usage_json;
