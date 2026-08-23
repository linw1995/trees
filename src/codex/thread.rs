use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::codex::app_server::{AppServerError, RpcClient};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartResponse {
    pub thread: ThreadSummary,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub id: String,
}

pub fn start_thread<R: RpcClient>(
    rpc: &mut R,
    project_id: &str,
    cwd: &Path,
    roots: &[PathBuf],
    timeout: Duration,
) -> Result<ThreadStartResponse, ThreadStartError> {
    start_thread_with_instructions(rpc, project_id, cwd, roots, None, timeout)
}

pub fn start_thread_with_instructions<R: RpcClient>(
    rpc: &mut R,
    project_id: &str,
    cwd: &Path,
    roots: &[PathBuf],
    developer_instructions: Option<&str>,
    timeout: Duration,
) -> Result<ThreadStartResponse, ThreadStartError> {
    if project_id.trim().is_empty() {
        return Err(ThreadStartError::InvalidInput(
            "project identifier must not be empty".to_owned(),
        ));
    }
    if !cwd.is_absolute() {
        return Err(ThreadStartError::InvalidInput(format!(
            "thread cwd must be absolute: {}",
            cwd.display()
        )));
    }
    if roots.is_empty() {
        return Err(ThreadStartError::InvalidInput(
            "thread must contain at least one runtime root".to_owned(),
        ));
    }
    if let Some(path) = roots.iter().find(|path| !path.is_absolute()) {
        return Err(ThreadStartError::InvalidInput(format!(
            "runtime root must be absolute: {}",
            path.display()
        )));
    }

    let mut params = json!({
        "projectId": project_id,
        "cwd": cwd,
        "runtimeWorkspaceRoots": roots
    });
    if let Some(developer_instructions) = developer_instructions {
        params["developerInstructions"] = Value::String(developer_instructions.to_owned());
    }

    let response = rpc.request("thread/start", params, timeout)?;
    let response: ThreadStartResponse = serde_json::from_value(response)
        .map_err(|source| ThreadStartError::MalformedResponse { source })?;
    if response.thread.id.trim().is_empty() {
        return Err(ThreadStartError::InvalidResponse(
            "thread/start returned an empty thread identifier".to_owned(),
        ));
    }
    Ok(response)
}

#[derive(Debug)]
pub enum ThreadStartError {
    AppServer(AppServerError),
    InvalidInput(String),
    MalformedResponse { source: serde_json::Error },
    InvalidResponse(String),
}

impl From<AppServerError> for ThreadStartError {
    fn from(error: AppServerError) -> Self {
        Self::AppServer(error)
    }
}

impl fmt::Display for ThreadStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AppServer(error) => error.fmt(formatter),
            Self::InvalidInput(message) | Self::InvalidResponse(message) => {
                formatter.write_str(message)
            }
            Self::MalformedResponse { source } => {
                write!(formatter, "malformed thread/start response: {source}")
            }
        }
    }
}

impl std::error::Error for ThreadStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AppServer(error) => Some(error),
            Self::MalformedResponse { source } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::Value;

    use super::*;
    use crate::codex::app_server::RpcClient;

    #[derive(Default)]
    struct FakeRpc {
        method: Option<String>,
        params: Option<Value>,
        response: Option<Result<Value, AppServerError>>,
    }

    impl RpcClient for FakeRpc {
        fn request(
            &mut self,
            method: &str,
            params: Value,
            _timeout: Duration,
        ) -> Result<Value, AppServerError> {
            self.method = Some(method.to_owned());
            self.params = Some(params);
            self.response
                .take()
                .expect("fake response should be configured")
        }

        fn notify(&mut self, _method: &str, _params: Value) -> Result<(), AppServerError> {
            Ok(())
        }
    }

    #[test]
    fn starts_thread_with_project_and_all_runtime_roots() {
        let mut rpc = FakeRpc {
            response: Some(Ok(json!({"thread": {"id": "thread-id"}}))),
            ..FakeRpc::default()
        };
        let roots = vec![
            PathBuf::from("/workspace/one"),
            PathBuf::from("/workspace/two"),
        ];

        let response = start_thread(
            &mut rpc,
            "project-id",
            Path::new("/workspace"),
            &roots,
            Duration::from_secs(1),
        )
        .expect("thread should start");

        assert_eq!(response.thread.id, "thread-id");
        assert_eq!(rpc.method.as_deref(), Some("thread/start"));
        let params = rpc.params.expect("request params should be captured");
        assert_eq!(params["projectId"], "project-id");
        assert_eq!(params["cwd"], "/workspace");
        assert_eq!(params["runtimeWorkspaceRoots"][0], "/workspace/one");
        assert_eq!(params["runtimeWorkspaceRoots"][1], "/workspace/two");
    }

    #[test]
    fn starts_thread_with_model_visible_workspace_context() {
        let mut rpc = FakeRpc {
            response: Some(Ok(json!({"thread": {"id": "thread-id"}}))),
            ..FakeRpc::default()
        };

        start_thread_with_instructions(
            &mut rpc,
            "project-id",
            Path::new("/workspace"),
            &[PathBuf::from("/workspace/one")],
            Some("Treat this as one logical monorepo."),
            Duration::from_secs(1),
        )
        .expect("thread should start");

        let params = rpc.params.expect("request params should be captured");
        assert_eq!(
            params["developerInstructions"],
            "Treat this as one logical monorepo."
        );
    }

    #[test]
    fn rejects_empty_project_identifier() {
        let mut rpc = FakeRpc::default();

        let error = start_thread(
            &mut rpc,
            "",
            Path::new("/workspace"),
            &[PathBuf::from("/workspace/one")],
            Duration::from_secs(1),
        )
        .expect_err("empty project id should fail");

        assert!(matches!(error, ThreadStartError::InvalidInput(_)));
    }
}
