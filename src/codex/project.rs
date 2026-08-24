use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::codex::WORKSPACE_METADATA_KEY;
use crate::domain::WorkspaceId;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRoot {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub roots: Vec<ProjectRoot>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCreateResponse {
    pub project: Project,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectUpdateResponse {
    pub project: Project,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectListResponse {
    pub data: Vec<Project>,
    pub next_cursor: Option<String>,
}

impl Project {
    pub fn belongs_to(&self, workspace_id: &WorkspaceId) -> bool {
        self.metadata
            .get(WORKSPACE_METADATA_KEY)
            .map(String::as_str)
            == Some(workspace_id.to_string().as_str())
    }

    pub fn has_roots(&self, roots: &[PathBuf]) -> bool {
        self.roots.iter().map(|root| &root.path).eq(roots.iter())
    }
}

pub fn matching_workspace_projects<'a>(
    projects: &'a [Project],
    workspace_id: &WorkspaceId,
) -> Vec<&'a Project> {
    projects
        .iter()
        .filter(|project| project.belongs_to(workspace_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(workspace_id: &WorkspaceId, roots: &[&str]) -> Project {
        Project {
            id: "project-id".to_owned(),
            name: "workspace".to_owned(),
            roots: roots
                .iter()
                .map(|path| ProjectRoot {
                    path: PathBuf::from(path),
                })
                .collect(),
            metadata: BTreeMap::from([(
                WORKSPACE_METADATA_KEY.to_owned(),
                workspace_id.to_string(),
            )]),
        }
    }

    #[test]
    fn matches_workspace_ownership_metadata() {
        let workspace_id = WorkspaceId::new();
        let owned = project(&workspace_id, &["/workspace/one"]);
        let other = project(&WorkspaceId::new(), &["/workspace/one"]);

        assert!(owned.belongs_to(&workspace_id));
        assert!(!other.belongs_to(&workspace_id));
        assert_eq!(
            matching_workspace_projects(&[owned, other], &workspace_id).len(),
            1
        );
    }

    #[test]
    fn compares_roots_in_order() {
        let workspace_id = WorkspaceId::new();
        let project = project(&workspace_id, &["/workspace/one", "/workspace/two"]);

        assert!(project.has_roots(&[
            PathBuf::from("/workspace/one"),
            PathBuf::from("/workspace/two"),
        ]));
        assert!(!project.has_roots(&[
            PathBuf::from("/workspace/two"),
            PathBuf::from("/workspace/one"),
        ]));
    }

    #[test]
    fn deserializes_project_list_response() {
        let response: ProjectListResponse = serde_json::from_str(
            r#"{
                "data": [{
                    "id": "project-id",
                    "name": "workspace",
                    "roots": [{"path": "/workspace/one"}],
                    "metadata": {"treesWorkspaceId": "01900000-0000-7000-8000-000000000000"}
                }],
                "nextCursor": "next"
            }"#,
        )
        .expect("project list response should deserialize");

        assert_eq!(response.data.len(), 1);
        assert_eq!(response.next_cursor.as_deref(), Some("next"));
    }
}
