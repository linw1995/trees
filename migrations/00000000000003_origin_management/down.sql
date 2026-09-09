DROP TABLE pending_origin_clones;
DROP INDEX origin_remote_url;
ALTER TABLE origin_repositories DROP COLUMN remote_url;
ALTER TABLE origin_repositories DROP COLUMN managed_root;
ALTER TABLE origin_repositories DROP COLUMN management_mode;
ALTER TABLE origin_repositories DROP COLUMN registered;
