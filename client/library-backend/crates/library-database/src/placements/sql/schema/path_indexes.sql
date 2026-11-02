-- Exact path names and subtree walks including deleted folders share this
-- parent index. Portable sibling identity remains enforced separately.
CREATE INDEX IF NOT EXISTS idx_dir_parent_name ON dir(parent_id,name);

-- Trash traversal follows the retained parent when display projection has
-- moved a deleted folder elsewhere.
CREATE INDEX IF NOT EXISTS idx_dir_trashed_parent
ON dir(COALESCE(canonical_parent_id,parent_id)) WHERE deleted_at IS NOT NULL;
