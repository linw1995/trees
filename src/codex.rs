use std::collections::BTreeMap;

use crate::domain::WorkspaceId;

pub mod app_server;
pub mod project;
pub mod project_sync;
pub mod thread;

pub const WORKSPACE_METADATA_KEY: &str = "treesWorkspaceId";

pub fn project_idempotency_key(workspace_id: &WorkspaceId) -> String {
    format!("trees:workspace:{workspace_id}")
}

pub fn project_metadata(workspace_id: &WorkspaceId) -> BTreeMap<String, String> {
    BTreeMap::from([(WORKSPACE_METADATA_KEY.to_owned(), workspace_id.to_string())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_stable_project_identity_from_workspace_id() {
        let workspace_id = WorkspaceId::new();

        assert_eq!(
            project_idempotency_key(&workspace_id),
            project_idempotency_key(&workspace_id)
        );
        assert_eq!(
            project_metadata(&workspace_id).get(WORKSPACE_METADATA_KEY),
            Some(&workspace_id.to_string())
        );
    }

    #[test]
    fn derives_distinct_keys_for_distinct_workspaces() {
        let first = WorkspaceId::new();
        let second = WorkspaceId::new();

        assert_ne!(
            project_idempotency_key(&first),
            project_idempotency_key(&second)
        );
        assert_ne!(project_metadata(&first), project_metadata(&second));
    }
}
