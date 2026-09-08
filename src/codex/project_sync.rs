use std::path::PathBuf;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use snafu::Snafu;
use uuid::Uuid;

use crate::codex::app_server::{AppServerError, RpcClient};
use crate::codex::project::{
    matching_workspace_projects, Project, ProjectCreateResponse, ProjectListResponse,
    ProjectUpdateResponse,
};
use crate::codex::{project_idempotency_key, project_metadata};
use crate::domain::WorkspaceId;

const MAX_PROJECT_LIST_PAGES: usize = 10_000;

#[derive(Debug)]
pub struct ProjectSession {
    pub codex_home: PathBuf,
    pub project: Project,
}

pub struct ProjectSynchronizer<'a, R> {
    rpc: &'a mut R,
    request_timeout: Duration,
}

impl<'a, R: RpcClient> ProjectSynchronizer<'a, R> {
    pub fn new(rpc: &'a mut R, request_timeout: Duration) -> Self {
        Self {
            rpc,
            request_timeout,
        }
    }

    pub fn synchronize(
        &mut self,
        workspace_id: &WorkspaceId,
        name: &str,
        roots: &[PathBuf],
    ) -> Result<ProjectSession, ProjectSyncError> {
        validate_inputs(name, roots)?;
        let codex_home = self.initialize()?.codex_home;
        let project = self.create_or_recover(workspace_id, name, roots)?;
        let project = self.update_roots_if_needed(project, roots)?;

        Ok(ProjectSession {
            codex_home,
            project,
        })
    }

    fn initialize(&mut self) -> Result<InitializeResponse, ProjectSyncError> {
        let response = self.rpc.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "trees",
                    "title": "Trees",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": true
                }
            }),
            self.request_timeout,
        )?;
        let response: InitializeResponse = parse_response("initialize", response)?;
        self.rpc.notify("initialized", json!({}))?;
        Ok(response)
    }

    fn create_or_recover(
        &mut self,
        workspace_id: &WorkspaceId,
        name: &str,
        roots: &[PathBuf],
    ) -> Result<Project, ProjectSyncError> {
        let idempotency_key = project_idempotency_key(workspace_id);
        let params = create_params(name, roots, workspace_id, &idempotency_key);
        match self
            .rpc
            .request("project/create", params, self.request_timeout)
        {
            Ok(response) => {
                Ok(parse_response::<ProjectCreateResponse>("project/create", response)?.project)
            }
            Err(error) if is_deleted_project_error(&error) => {
                if let Some(project) = self.find_owned_project(workspace_id)? {
                    return Ok(project);
                }

                let recovery_key = format!("{idempotency_key}:recovery:{}", Uuid::now_v7());
                let response = self.rpc.request(
                    "project/create",
                    create_params(name, roots, workspace_id, &recovery_key),
                    self.request_timeout,
                )?;
                Ok(parse_response::<ProjectCreateResponse>("project/create", response)?.project)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn find_owned_project(
        &mut self,
        workspace_id: &WorkspaceId,
    ) -> Result<Option<Project>, ProjectSyncError> {
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PROJECT_LIST_PAGES {
            let response = self.rpc.request(
                "project/list",
                json!({"cursor": cursor, "limit": 100}),
                self.request_timeout,
            )?;
            let page: ProjectListResponse = parse_response("project/list", response)?;
            let matches = matching_workspace_projects(&page.data, workspace_id);
            if matches.len() > 1 {
                return Err(ProjectSyncError::AmbiguousOwnership {
                    workspace_id: workspace_id.to_string(),
                    project_ids: matches.iter().map(|project| project.id.clone()).collect(),
                });
            }
            if let Some(project) = matches.first() {
                return Ok(Some((*project).clone()));
            }

            match page.next_cursor {
                None => return Ok(None),
                Some(next_cursor) if cursor.as_deref() == Some(next_cursor.as_str()) => {
                    return Err(ProjectSyncError::InvalidResponse {
                        method: "project/list".to_owned(),
                        message: "nextCursor did not advance".to_owned(),
                    });
                }
                Some(next_cursor) => cursor = Some(next_cursor),
            }
        }

        Err(ProjectSyncError::InvalidResponse {
            method: "project/list".to_owned(),
            message: format!("exceeded {MAX_PROJECT_LIST_PAGES} pages"),
        })
    }

    fn update_roots_if_needed(
        &mut self,
        project: Project,
        roots: &[PathBuf],
    ) -> Result<Project, ProjectSyncError> {
        if project.has_roots(roots) {
            return Ok(project);
        }

        let response = self.rpc.request(
            "project/update",
            json!({
                "projectId": project.id,
                "roots": root_values(roots)
            }),
            self.request_timeout,
        )?;
        Ok(parse_response::<ProjectUpdateResponse>("project/update", response)?.project)
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeResponse {
    codex_home: PathBuf,
}

fn create_params(
    name: &str,
    roots: &[PathBuf],
    workspace_id: &WorkspaceId,
    idempotency_key: &str,
) -> Value {
    json!({
        "name": name,
        "roots": root_values(roots),
        "metadata": project_metadata(workspace_id),
        "idempotencyKey": idempotency_key
    })
}

fn root_values(roots: &[PathBuf]) -> Vec<Value> {
    roots.iter().map(|path| json!({"path": path})).collect()
}

fn validate_inputs(name: &str, roots: &[PathBuf]) -> Result<(), ProjectSyncError> {
    if name.trim().is_empty() {
        return Err(ProjectSyncError::InvalidInput {
            message: "project name must not be empty".to_owned(),
        });
    }
    if roots.is_empty() {
        return Err(ProjectSyncError::InvalidInput {
            message: "project must contain at least one root".to_owned(),
        });
    }
    if let Some(path) = roots.iter().find(|path| !path.is_absolute()) {
        return Err(ProjectSyncError::InvalidInput {
            message: format!("project root must be absolute: {}", path.display()),
        });
    }
    Ok(())
}

fn parse_response<T: DeserializeOwned>(method: &str, value: Value) -> Result<T, ProjectSyncError> {
    serde_json::from_value(value).map_err(|source| ProjectSyncError::MalformedResponse {
        method: method.to_owned(),
        source,
    })
}

fn is_deleted_project_error(error: &AppServerError) -> bool {
    matches!(
        error,
        AppServerError::Remote { method, error }
            if method == "project/create"
                && error.contains("idempotency key refers to deleted project")
    )
}

#[derive(Debug, Snafu)]
pub enum ProjectSyncError {
    #[snafu(transparent)]
    AppServer { source: AppServerError },
    #[snafu(display("{message}"))]
    InvalidInput { message: String },
    #[snafu(display("malformed {method} response: {source}"))]
    MalformedResponse {
        method: String,
        source: serde_json::Error,
    },
    #[snafu(display("invalid {method} response: {message}"))]
    InvalidResponse { method: String, message: String },
    #[snafu(display(
        "multiple Codex projects belong to workspace {workspace_id}: {}",
        project_ids.join(", ")
    ))]
    AmbiguousOwnership {
        workspace_id: String,
        project_ids: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::VecDeque;

    use super::*;
    use crate::codex::app_server::RpcClient;
    use crate::codex::project::{ProjectRoot, ProjectUpdateResponse};

    #[derive(Default)]
    struct FakeRpc {
        responses: VecDeque<Result<Value, AppServerError>>,
        methods: Vec<String>,
        notifications: Vec<String>,
    }

    impl FakeRpc {
        fn with_responses(responses: Vec<Result<Value, AppServerError>>) -> Self {
            Self {
                responses: responses.into(),
                ..Self::default()
            }
        }
    }

    impl RpcClient for FakeRpc {
        fn request(
            &mut self,
            method: &str,
            _params: Value,
            _timeout: Duration,
        ) -> Result<Value, AppServerError> {
            self.methods.push(method.to_owned());
            self.responses
                .pop_front()
                .expect("fake response should be configured")
        }

        fn notify(&mut self, method: &str, _params: Value) -> Result<(), AppServerError> {
            self.notifications.push(method.to_owned());
            Ok(())
        }
    }

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
            metadata: BTreeMap::from([("treesWorkspaceId".to_owned(), workspace_id.to_string())]),
        }
    }

    fn initialize_response() -> Value {
        json!({"codexHome": "/tmp/codex"})
    }

    fn create_response(project: Project) -> Value {
        serde_json::to_value(ProjectCreateResponse { project })
            .expect("create response should serialize")
    }

    fn update_response(project: Project) -> Value {
        serde_json::to_value(ProjectUpdateResponse { project })
            .expect("update response should serialize")
    }

    #[test]
    fn creates_project_and_initializes_protocol() {
        let workspace_id = WorkspaceId::new();
        let roots = vec![PathBuf::from("/workspace/one")];
        let project = project(&workspace_id, &["/workspace/one"]);
        let mut rpc = FakeRpc::with_responses(vec![
            Ok(initialize_response()),
            Ok(create_response(project.clone())),
        ]);

        let session = ProjectSynchronizer::new(&mut rpc, Duration::from_secs(1))
            .synchronize(&workspace_id, "workspace", &roots)
            .expect("project should synchronize");

        assert_eq!(session.project, project);
        assert_eq!(rpc.methods, ["initialize", "project/create"]);
        assert_eq!(rpc.notifications, ["initialized"]);
    }

    #[test]
    fn updates_project_when_roots_change() {
        let workspace_id = WorkspaceId::new();
        let roots = vec![PathBuf::from("/workspace/new")];
        let existing = project(&workspace_id, &["/workspace/old"]);
        let updated = project(&workspace_id, &["/workspace/new"]);
        let mut rpc = FakeRpc::with_responses(vec![
            Ok(initialize_response()),
            Ok(create_response(existing)),
            Ok(update_response(updated.clone())),
        ]);

        let session = ProjectSynchronizer::new(&mut rpc, Duration::from_secs(1))
            .synchronize(&workspace_id, "workspace", &roots)
            .expect("project roots should update");

        assert_eq!(session.project, updated);
        assert_eq!(
            rpc.methods,
            ["initialize", "project/create", "project/update"]
        );
    }

    #[test]
    fn recovers_deleted_project_from_metadata() {
        let workspace_id = WorkspaceId::new();
        let roots = vec![PathBuf::from("/workspace/one")];
        let existing = project(&workspace_id, &["/workspace/one"]);
        let deleted = AppServerError::Remote {
            method: "project/create".to_owned(),
            error: "idempotency key refers to deleted project".to_owned(),
        };
        let list = serde_json::json!({
            "data": [existing.clone()],
            "nextCursor": null
        });
        let mut rpc =
            FakeRpc::with_responses(vec![Ok(initialize_response()), Err(deleted), Ok(list)]);

        let session = ProjectSynchronizer::new(&mut rpc, Duration::from_secs(1))
            .synchronize(&workspace_id, "workspace", &roots)
            .expect("deleted project should be recovered");

        assert_eq!(session.project, existing);
        assert_eq!(
            rpc.methods,
            ["initialize", "project/create", "project/list"]
        );
    }

    #[test]
    fn rejects_ambiguous_project_recovery() {
        let workspace_id = WorkspaceId::new();
        let project_one = project(&workspace_id, &["/workspace/one"]);
        let mut project_two = project(&workspace_id, &["/workspace/one"]);
        project_two.id = "project-two".to_owned();
        let deleted = AppServerError::Remote {
            method: "project/create".to_owned(),
            error: "idempotency key refers to deleted project".to_owned(),
        };
        let list = serde_json::json!({
            "data": [project_one, project_two],
            "nextCursor": null
        });
        let mut rpc =
            FakeRpc::with_responses(vec![Ok(initialize_response()), Err(deleted), Ok(list)]);

        let error = ProjectSynchronizer::new(&mut rpc, Duration::from_secs(1))
            .synchronize(
                &workspace_id,
                "workspace",
                &[PathBuf::from("/workspace/one")],
            )
            .expect_err("ambiguous recovery should fail");

        assert!(matches!(error, ProjectSyncError::AmbiguousOwnership { .. }));
    }
}
