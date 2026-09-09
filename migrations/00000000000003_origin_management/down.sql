CREATE TEMP TABLE origin_rollback_guard (pending_count INTEGER CHECK (pending_count = 0));
INSERT INTO origin_rollback_guard SELECT COUNT(*) FROM pending_origin_clones;
DROP TABLE origin_rollback_guard;
DROP TABLE pending_origin_clones;
