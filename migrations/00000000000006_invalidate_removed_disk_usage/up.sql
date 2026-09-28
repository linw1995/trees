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
