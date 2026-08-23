use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::Duration;

use serde_json::{json, Value};

use crate::codex::app_server::{AppServerError, AppServerProcess, RpcClient};
use crate::codex::args::merge_codex_args;
use crate::codex::lock::{WorkspaceLock, WorkspaceLockError};
use crate::codex::project_sync::{ProjectSession, ProjectSyncError, ProjectSynchronizer};
use crate::codex::thread::{start_thread_with_instructions, ThreadStartError};
use crate::codex::workspace::{prepare, PreparedWorkspace, WorkspacePreparationError};

const SETUP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SETUP_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct LaunchRequest {
    pub workspace_path: PathBuf,
    pub codex_bin: PathBuf,
    pub codex_args: Vec<OsString>,
}

#[derive(Debug, Clone)]
pub struct ResumeRequest {
    pub workspace_path: PathBuf,
    pub codex_bin: PathBuf,
    pub codex_args: Vec<OsString>,
}

#[derive(Debug, Clone)]
pub struct PreparedLaunch {
    pub codex_home: PathBuf,
    pub project_id: String,
    pub thread_id: String,
    pub cwd: PathBuf,
    pub runtime_roots: Vec<PathBuf>,
    pub developer_instructions: String,
    pub codex_args: Vec<OsString>,
}

#[derive(Debug, Clone)]
pub struct PreparedResume {
    pub codex_home: PathBuf,
    pub project_id: String,
    pub cwd: PathBuf,
    pub runtime_roots: Vec<PathBuf>,
    pub developer_instructions: String,
    pub codex_args: Vec<OsString>,
}

pub fn launch(request: LaunchRequest) -> Result<ExitStatus, CodexLaunchError> {
    let _lock = WorkspaceLock::acquire(&request.workspace_path).map_err(CodexLaunchError::Lock)?;
    let codex_bin = request.codex_bin.clone();
    let prepared = prepare_launch(request)?;
    handoff(&codex_bin, &prepared)
}

pub fn resume(request: ResumeRequest) -> Result<ExitStatus, CodexLaunchError> {
    let _lock = WorkspaceLock::acquire(&request.workspace_path).map_err(CodexLaunchError::Lock)?;
    let codex_bin = request.codex_bin.clone();
    let prepared = prepare_resume(request)?;
    resume_handoff(&codex_bin, &prepared)
}

pub fn prepare_launch(request: LaunchRequest) -> Result<PreparedLaunch, CodexLaunchError> {
    let mut connection = crate::database::open_default().map_err(CodexLaunchError::DatabaseOpen)?;
    let workspace =
        prepare(&mut connection, &request.workspace_path).map_err(CodexLaunchError::Workspace)?;
    prepare_with_app_server_and_args(&request.codex_bin, &workspace, request.codex_args)
}

pub fn prepare_resume(request: ResumeRequest) -> Result<PreparedResume, CodexLaunchError> {
    let mut connection = crate::database::open_default().map_err(CodexLaunchError::DatabaseOpen)?;
    let workspace =
        prepare(&mut connection, &request.workspace_path).map_err(CodexLaunchError::Workspace)?;
    prepare_resume_with_app_server(&request.codex_bin, &workspace, request.codex_args)
}

#[cfg(test)]
fn prepare_with_app_server(
    codex_bin: &Path,
    workspace: &PreparedWorkspace,
) -> Result<PreparedLaunch, CodexLaunchError> {
    prepare_with_app_server_and_args(codex_bin, workspace, Vec::new())
}

fn prepare_with_app_server_and_args(
    codex_bin: &Path,
    workspace: &PreparedWorkspace,
    codex_args: Vec<OsString>,
) -> Result<PreparedLaunch, CodexLaunchError> {
    let mut app_server = AppServerProcess::spawn(codex_bin).map_err(CodexLaunchError::AppServer)?;
    let setup_result = (|| {
        let project = ProjectSynchronizer::new(&mut app_server, SETUP_REQUEST_TIMEOUT)
            .synchronize(&workspace.id, &workspace.name, &workspace.roots)
            .map_err(CodexLaunchError::Project)?;
        let developer_instructions =
            workspace_developer_instructions(&mut app_server, workspace, SETUP_REQUEST_TIMEOUT)?;
        let thread = start_thread_with_instructions(
            &mut app_server,
            &project.project.id,
            workspace.path.as_path(),
            &workspace.roots,
            Some(&developer_instructions),
            SETUP_REQUEST_TIMEOUT,
        )
        .map_err(CodexLaunchError::Thread)?;
        Ok::<PreparedLaunch, CodexLaunchError>(prepared_launch(
            workspace,
            project,
            thread.thread.id,
            developer_instructions,
            codex_args,
        ))
    })();

    let shutdown_result = app_server.shutdown(SETUP_SHUTDOWN_TIMEOUT);
    match (setup_result, shutdown_result) {
        (Ok(launch), Ok(status)) if status.success() => Ok(launch),
        (Ok(_), Ok(status)) => Err(CodexLaunchError::SetupProcessExit(status)),
        (Ok(_), Err(error)) => Err(CodexLaunchError::Shutdown(error)),
        (Err(error), Ok(_)) => Err(error),
        (Err(error), Err(shutdown)) => Err(CodexLaunchError::SetupAndShutdown {
            setup: Box::new(error),
            shutdown,
        }),
    }
}

fn prepare_resume_with_app_server(
    codex_bin: &Path,
    workspace: &PreparedWorkspace,
    codex_args: Vec<OsString>,
) -> Result<PreparedResume, CodexLaunchError> {
    let mut app_server = AppServerProcess::spawn(codex_bin).map_err(CodexLaunchError::AppServer)?;
    let setup_result = (|| {
        let project = ProjectSynchronizer::new(&mut app_server, SETUP_REQUEST_TIMEOUT)
            .synchronize(&workspace.id, &workspace.name, &workspace.roots)
            .map_err(CodexLaunchError::Project)?;
        let developer_instructions =
            workspace_developer_instructions(&mut app_server, workspace, SETUP_REQUEST_TIMEOUT)?;
        Ok::<PreparedResume, CodexLaunchError>(PreparedResume {
            codex_home: project.codex_home,
            project_id: project.project.id,
            cwd: workspace.path.as_path().to_path_buf(),
            runtime_roots: workspace.roots.clone(),
            developer_instructions,
            codex_args,
        })
    })();

    let shutdown_result = app_server.shutdown(SETUP_SHUTDOWN_TIMEOUT);
    match (setup_result, shutdown_result) {
        (Ok(resume), Ok(status)) if status.success() => Ok(resume),
        (Ok(_), Ok(status)) => Err(CodexLaunchError::SetupProcessExit(status)),
        (Ok(_), Err(error)) => Err(CodexLaunchError::Shutdown(error)),
        (Err(error), Ok(_)) => Err(error),
        (Err(error), Err(shutdown)) => Err(CodexLaunchError::SetupAndShutdown {
            setup: Box::new(error),
            shutdown,
        }),
    }
}

fn workspace_developer_instructions<R: RpcClient>(
    rpc: &mut R,
    workspace: &PreparedWorkspace,
    timeout: Duration,
) -> Result<String, CodexLaunchError> {
    let response = rpc
        .request("config/read", json!({}), timeout)
        .map_err(CodexLaunchError::Config)?;
    let config = response
        .get("config")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            CodexLaunchError::InvalidConfigResponse(
                "config/read response did not contain a config object".to_owned(),
            )
        })?;
    let existing = match config.get("developer_instructions") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if value.trim().is_empty() => None,
        Some(Value::String(value)) => Some(value.as_str()),
        Some(_) => {
            return Err(CodexLaunchError::InvalidConfigResponse(
                "config.developer_instructions was not a string or null".to_owned(),
            ));
        }
    };
    Ok(merge_workspace_manifest(existing, workspace))
}

fn merge_workspace_manifest(existing: Option<&str>, workspace: &PreparedWorkspace) -> String {
    let mut manifest = format!(
        "Trees workspace `{}` is one logical monorepo composed of these managed worktrees:\n",
        workspace.name
    );
    for root in &workspace.roots {
        let repository_name = root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("repository");
        manifest.push_str(&format!("- `{repository_name}`: `{}`\n", root.display()));
    }
    manifest.push_str(
        "Treat all listed repositories as one coordinated workspace. Keep cross-repository changes consistent and edit only these managed worktree paths.",
    );

    match existing {
        Some(existing) => format!("{existing}\n\n--- Trees workspace context ---\n{manifest}"),
        None => manifest,
    }
}

fn prepared_launch(
    workspace: &PreparedWorkspace,
    project: ProjectSession,
    thread_id: String,
    developer_instructions: String,
    codex_args: Vec<OsString>,
) -> PreparedLaunch {
    PreparedLaunch {
        codex_home: project.codex_home,
        project_id: project.project.id,
        thread_id,
        cwd: workspace.path.as_path().to_path_buf(),
        runtime_roots: workspace.roots.clone(),
        developer_instructions,
        codex_args,
    }
}

pub fn handoff(
    codex_bin: &Path,
    prepared: &PreparedLaunch,
) -> Result<ExitStatus, CodexLaunchError> {
    let codex_args = merge_codex_args(
        &prepared.codex_args,
        &prepared.cwd,
        &prepared.runtime_roots,
        &prepared.developer_instructions,
    )
    .map_err(CodexLaunchError::Arguments)?;
    let mut command = Command::new(codex_bin);
    command.arg("resume").arg(&prepared.thread_id);
    command.args(codex_args);

    command
        .current_dir(&prepared.cwd)
        .status()
        .map_err(|source| CodexLaunchError::Handoff {
            executable: codex_bin.to_owned(),
            project_id: prepared.project_id.clone(),
            thread_id: prepared.thread_id.clone(),
            source,
        })
}

pub fn resume_handoff(
    codex_bin: &Path,
    prepared: &PreparedResume,
) -> Result<ExitStatus, CodexLaunchError> {
    let codex_args = merge_codex_args(
        &prepared.codex_args,
        &prepared.cwd,
        &prepared.runtime_roots,
        &prepared.developer_instructions,
    )
    .map_err(CodexLaunchError::Arguments)?;
    let mut command = Command::new(codex_bin);
    command.arg("resume").args(codex_args);

    command
        .current_dir(&prepared.cwd)
        .status()
        .map_err(|source| CodexLaunchError::ResumeHandoff {
            executable: codex_bin.to_owned(),
            project_id: prepared.project_id.clone(),
            source,
        })
}

#[derive(Debug)]
pub enum CodexLaunchError {
    DatabaseOpen(crate::database::DatabaseError),
    Workspace(WorkspacePreparationError),
    Lock(WorkspaceLockError),
    Arguments(crate::codex::args::CodexArgumentError),
    AppServer(AppServerError),
    Config(AppServerError),
    InvalidConfigResponse(String),
    Project(ProjectSyncError),
    Thread(ThreadStartError),
    SetupProcessExit(std::process::ExitStatus),
    Shutdown(AppServerError),
    Handoff {
        executable: PathBuf,
        project_id: String,
        thread_id: String,
        source: io::Error,
    },
    ResumeHandoff {
        executable: PathBuf,
        project_id: String,
        source: io::Error,
    },
    SetupAndShutdown {
        setup: Box<CodexLaunchError>,
        shutdown: AppServerError,
    },
}

impl fmt::Display for CodexLaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DatabaseOpen(error) => {
                write!(formatter, "failed to open Trees database: {error}")
            }
            Self::Workspace(error) => error.fmt(formatter),
            Self::Lock(error) => error.fmt(formatter),
            Self::Arguments(error) => error.fmt(formatter),
            Self::AppServer(error) => error.fmt(formatter),
            Self::Config(error) => error.fmt(formatter),
            Self::InvalidConfigResponse(message) => formatter.write_str(message),
            Self::Project(error) => error.fmt(formatter),
            Self::Thread(error) => error.fmt(formatter),
            Self::SetupProcessExit(status) => {
                write!(
                    formatter,
                    "setup app-server exited unsuccessfully: {status}"
                )
            }
            Self::Shutdown(error) => {
                write!(formatter, "failed to shut down setup app-server: {error}")
            }
            Self::Handoff {
                executable,
                project_id,
                thread_id,
                source,
            } => write!(
                formatter,
                "failed to resume Codex thread {thread_id} for project {project_id} using {}: {source}",
                executable.display()
            ),
            Self::ResumeHandoff {
                executable,
                project_id,
                source,
            } => write!(
                formatter,
                "failed to open a Codex resume picker for project {project_id} using {}: {source}",
                executable.display()
            ),
            Self::SetupAndShutdown { setup, shutdown } => write!(
                formatter,
                "Codex setup failed: {setup}; setup app-server shutdown also failed: {shutdown}"
            ),
        }
    }
}

impl std::error::Error for CodexLaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DatabaseOpen(error) => Some(error),
            Self::Workspace(error) => Some(error),
            Self::Lock(error) => Some(error),
            Self::Arguments(error) => Some(error),
            Self::AppServer(error) => Some(error),
            Self::Config(error) => Some(error),
            Self::Project(error) => Some(error),
            Self::Thread(error) => Some(error),
            Self::Shutdown(error) => Some(error),
            Self::Handoff { source, .. } => Some(source),
            Self::ResumeHandoff { source, .. } => Some(source),
            Self::SetupAndShutdown { setup, .. } => Some(setup),
            Self::SetupProcessExit(_) | Self::InvalidConfigResponse(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::Value;

    use super::*;

    #[derive(Default)]
    struct ConfigRpc {
        response: Option<Result<Value, AppServerError>>,
        method: Option<String>,
    }

    impl RpcClient for ConfigRpc {
        fn request(
            &mut self,
            method: &str,
            _params: Value,
            _timeout: Duration,
        ) -> Result<Value, AppServerError> {
            self.method = Some(method.to_owned());
            self.response
                .take()
                .expect("config response should be configured")
        }

        fn notify(&mut self, _method: &str, _params: Value) -> Result<(), AppServerError> {
            Ok(())
        }
    }

    #[cfg(unix)]
    fn prepared_workspace(root: &Path) -> PreparedWorkspace {
        PreparedWorkspace {
            id: crate::domain::WorkspaceId::new(),
            path: crate::domain::CanonicalPath::resolve(root)
                .expect("test workspace should be canonical"),
            name: "workspace".to_owned(),
            roots: vec![root.to_owned()],
        }
    }

    fn workspace_for_context() -> PreparedWorkspace {
        PreparedWorkspace {
            id: crate::domain::WorkspaceId::new(),
            path: crate::domain::CanonicalPath::from_absolute("/workspace")
                .expect("workspace path should be absolute"),
            name: "workspace".to_owned(),
            roots: vec![
                PathBuf::from("/workspace/one"),
                PathBuf::from("/workspace/two"),
            ],
        }
    }

    #[cfg(unix)]
    #[test]
    fn starts_thread_with_workspace_manifest_and_preserved_instructions() {
        let root =
            std::env::temp_dir().join(format!("trees-codex-context-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("test root should be created");
        let capture = root.join("thread-start-request");
        let script = r#"
IFS= read -r request
printf '%s\n' '{"id":1,"result":{"codexHome":"/tmp/codex"}}'
IFS= read -r request
IFS= read -r request
printf '%s\n' '{"id":2,"result":{"project":{"id":"project-id","name":"workspace","roots":[{"path":"/workspace/one"},{"path":"/workspace/two"}],"metadata":{}}}}'
IFS= read -r request
printf '%s\n' '{"id":3,"result":{"config":{"developer_instructions":"Keep user rules."}}}'
IFS= read -r request
printf '%s\n' "$request" > '__CAPTURE__'
printf '%s\n' '{"id":4,"result":{"thread":{"id":"thread-id"}}}'
        "#
        .replace("__CAPTURE__", &capture.display().to_string());
        let executable = fake_app_server(&root, &script);
        let workspace = workspace_for_context();

        let prepared =
            prepare_with_app_server(&executable, &workspace).expect("Codex setup should succeed");

        assert_eq!(prepared.project_id, "project-id");
        assert_eq!(prepared.thread_id, "thread-id");
        let request: Value = serde_json::from_str(
            &fs::read_to_string(&capture).expect("thread request should be captured"),
        )
        .expect("thread request should be JSON");
        assert_eq!(request["method"], "thread/start");
        assert_eq!(request["params"]["projectId"], "project-id");
        assert_eq!(request["params"]["cwd"], "/workspace");
        assert_eq!(
            request["params"]["runtimeWorkspaceRoots"],
            json!(["/workspace/one", "/workspace/two"])
        );
        let instructions = request["params"]["developerInstructions"]
            .as_str()
            .expect("developer instructions should be present");
        assert!(instructions.starts_with("Keep user rules."));
        assert!(instructions.contains("/workspace/one"));
        assert!(instructions.contains("/workspace/two"));

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn merges_effective_user_instructions_with_workspace_manifest() {
        let workspace = workspace_for_context();
        let mut rpc = ConfigRpc {
            response: Some(Ok(json!({
                "config": {"developer_instructions": "Keep user rules."}
            }))),
            ..ConfigRpc::default()
        };

        let instructions =
            workspace_developer_instructions(&mut rpc, &workspace, Duration::from_secs(1))
                .expect("workspace context should be built");

        assert_eq!(rpc.method.as_deref(), Some("config/read"));
        assert!(instructions.starts_with("Keep user rules."));
        assert!(instructions.contains("/workspace/one"));
        assert!(instructions.contains("/workspace/two"));
        assert!(instructions.contains("one logical monorepo"));
    }

    #[test]
    fn builds_workspace_manifest_without_existing_instructions() {
        let workspace = workspace_for_context();
        let mut rpc = ConfigRpc {
            response: Some(Ok(json!({"config": {}}))),
            ..ConfigRpc::default()
        };

        let instructions =
            workspace_developer_instructions(&mut rpc, &workspace, Duration::from_secs(1))
                .expect("workspace context should be built");

        assert!(!instructions.contains("Trees workspace context"));
        assert!(instructions.starts_with("Trees workspace `workspace`"));
    }

    #[test]
    fn rejects_malformed_config_instructions() {
        let workspace = workspace_for_context();
        let mut rpc = ConfigRpc {
            response: Some(Ok(json!({
                "config": {"developer_instructions": ["invalid"]}
            }))),
            ..ConfigRpc::default()
        };

        let error = workspace_developer_instructions(&mut rpc, &workspace, Duration::from_secs(1))
            .expect_err("invalid developer instructions should fail");

        assert!(error.to_string().contains("developer_instructions"));
    }

    #[cfg(unix)]
    fn fake_app_server(root: &Path, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let executable = root.join("fake-codex");
        fs::write(&executable, format!("#!/bin/sh\n{script}\n"))
            .expect("fake app-server should be written");
        let mut permissions = fs::metadata(&executable)
            .expect("fake app-server metadata should be available")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions)
            .expect("fake app-server should be executable");
        executable
    }

    #[cfg(unix)]
    #[test]
    fn hands_thread_to_resume_with_inherited_terminal_context() {
        use std::os::unix::fs::PermissionsExt;

        let root =
            std::env::temp_dir().join(format!("trees-codex-handoff-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("handoff root should be created");
        let capture = root.join("capture");
        let first = root.join("one");
        let secondary = root.join("secondary");
        fs::create_dir_all(&first).expect("first root should be created");
        fs::create_dir_all(&secondary).expect("secondary root should be created");
        let executable = root.join("fake-codex");
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'PWD=%s\\nPATH=%s\\n' \"$PWD\" \"$PATH\" >> '{}'\nexit 7\n",
                capture.display(),
                capture.display()
            ),
        )
        .expect("fake executable should be written");
        let mut permissions = fs::metadata(&executable)
            .expect("fake executable metadata should be available")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions)
            .expect("fake executable should be executable");

        let prepared = PreparedLaunch {
            codex_home: root.join("codex-home"),
            project_id: "project-id".to_owned(),
            thread_id: "thread-id".to_owned(),
            cwd: root.clone(),
            runtime_roots: vec![root.join("one"), secondary.clone()],
            developer_instructions: "workspace instructions".to_owned(),
            codex_args: vec![OsString::from("--model"), OsString::from("gpt-5.5")],
        };
        let status = handoff(&executable, &prepared).expect("handoff should start");

        assert_eq!(status.code(), Some(7));
        let captured = fs::read_to_string(&capture).expect("fake executable should capture input");
        let lines: Vec<_> = captured.lines().collect();
        let canonical_root = fs::canonicalize(&root).expect("handoff root should be canonical");
        assert_eq!(lines[0], "resume");
        assert_eq!(lines[1], "thread-id");
        assert_eq!(lines[2], "--model");
        assert_eq!(lines[3], "gpt-5.5");
        assert_eq!(lines[4], "--cd");
        assert_eq!(Path::new(lines[5]), root.as_path());
        assert_eq!(lines[6], "--add-dir");
        assert_eq!(Path::new(lines[7]), first.as_path());
        assert_eq!(lines[8], "--add-dir");
        assert_eq!(Path::new(lines[9]), secondary.as_path());
        assert_eq!(lines[10], "--config");
        assert_eq!(
            lines[11],
            "developer_instructions=\"workspace instructions\""
        );
        assert_eq!(Path::new(&lines[12][4..]), canonical_root.as_path());
        assert!(lines[13].starts_with("PATH="));

        fs::remove_dir_all(root).expect("handoff test root should be removable");
    }

    #[cfg(unix)]
    #[test]
    fn hands_off_to_native_resume_picker_without_a_session_id() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "trees-codex-picker-handoff-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(&root).expect("handoff root should be created");
        let capture = root.join("capture");
        let secondary = root.join("secondary");
        fs::create_dir_all(&secondary).expect("secondary root should be created");
        let executable = root.join("fake-codex");
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'PWD=%s\\n' \"$PWD\" >> '{}'\nexit 9\n",
                capture.display(),
                capture.display()
            ),
        )
        .expect("fake executable should be written");
        let mut permissions = fs::metadata(&executable)
            .expect("fake executable metadata should be available")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions)
            .expect("fake executable should be executable");

        let prepared = PreparedResume {
            codex_home: root.join("codex-home"),
            project_id: "project-id".to_owned(),
            cwd: root.clone(),
            runtime_roots: vec![root.clone(), secondary.clone()],
            developer_instructions: "workspace instructions".to_owned(),
            codex_args: vec![
                OsString::from("--all"),
                OsString::from("--profile"),
                OsString::from("work"),
            ],
        };
        let status = resume_handoff(&executable, &prepared).expect("picker handoff should start");

        assert_eq!(status.code(), Some(9));
        let captured = fs::read_to_string(&capture).expect("fake executable should capture input");
        let lines: Vec<_> = captured.lines().collect();
        assert_eq!(lines[0], "resume");
        assert_eq!(lines[1], "--all");
        assert_eq!(lines[2], "--profile");
        assert_eq!(lines[3], "work");
        assert_eq!(lines[4], "--cd");
        assert_eq!(Path::new(lines[5]), root.as_path());
        assert_eq!(lines[6], "--add-dir");
        assert_eq!(Path::new(lines[7]), root.as_path());
        assert_eq!(lines[8], "--add-dir");
        assert_eq!(Path::new(lines[9]), secondary.as_path());
        assert_eq!(lines[10], "--config");
        assert_eq!(
            lines[11],
            "developer_instructions=\"workspace instructions\""
        );

        fs::remove_dir_all(root).expect("handoff root should be removable");
    }

    #[cfg(unix)]
    #[test]
    fn prepares_resume_context_without_starting_a_new_thread() {
        let root =
            std::env::temp_dir().join(format!("trees-codex-picker-setup-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("setup root should be created");
        let capture = root.join("requests");
        let script = format!(
            r#"
IFS= read -r request
printf '%s\n' "$request" > '{capture}'
printf '%s\n' '{{"id":1,"result":{{"codexHome":"/tmp/codex"}}}}'
# Consume the initialized notification before reading the next request.
IFS= read -r request
IFS= read -r request
printf '%s\n' "$request" >> '{capture}'
printf '%s\n' '{{"id":2,"result":{{"project":{{"id":"project-id","name":"workspace","roots":[{{"path":"/workspace/one"}},{{"path":"/workspace/two"}}],"metadata":{{}}}}}}}}'
IFS= read -r request
printf '%s\n' "$request" >> '{capture}'
printf '%s\n' '{{"id":3,"result":{{"config":{{"developer_instructions":"Keep user rules."}}}}}}'
"#,
            capture = capture.display()
        );
        let executable = fake_app_server(&root, &script);
        let workspace = workspace_for_context();

        let prepared = prepare_resume_with_app_server(&executable, &workspace, Vec::new())
            .expect("resume setup should succeed");

        assert_eq!(prepared.project_id, "project-id");
        assert_eq!(prepared.cwd, PathBuf::from("/workspace"));
        assert!(prepared.developer_instructions.contains("Keep user rules."));
        let requests = fs::read_to_string(&capture).expect("requests should be captured");
        assert!(requests.contains("initialize"));
        assert!(requests.contains("project/create"));
        assert!(requests.contains("config/read"));
        assert!(!requests.contains("thread/start"));

        fs::remove_dir_all(root).expect("setup root should be removable");
    }

    #[cfg(unix)]
    #[test]
    fn reports_missing_app_server_binary() {
        let root =
            std::env::temp_dir().join(format!("trees-codex-missing-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("test workspace should be created");
        let executable = root.join("missing-codex");
        let workspace = prepared_workspace(&root);

        let error = prepare_with_app_server(&executable, &workspace)
            .expect_err("missing app-server should fail before setup");
        let message = error.to_string();

        assert!(message.contains("missing-codex"));
        assert!(message.contains("failed to start"));
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[cfg(unix)]
    #[test]
    fn reports_unsupported_app_server_method() {
        let root =
            std::env::temp_dir().join(format!("trees-codex-unsupported-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("test workspace should be created");
        let executable = fake_app_server(
            &root,
            r#"
IFS= read -r request
printf '%s\n' '{"id":1,"result":{"codexHome":"/tmp/codex"}}'
IFS= read -r request
IFS= read -r request
printf '%s\n' '{"id":2,"error":{"code":-32601,"message":"Method not found"}}'
            "#,
        );
        let workspace = prepared_workspace(&root);

        let error = prepare_with_app_server(&executable, &workspace)
            .expect_err("unsupported app-server method should fail setup");
        let message = error.to_string();

        assert!(message.contains("project/create"));
        assert!(message.contains("Method not found"));
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[cfg(unix)]
    #[test]
    fn reports_malformed_app_server_response() {
        let root =
            std::env::temp_dir().join(format!("trees-codex-malformed-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("test workspace should be created");
        let executable = fake_app_server(
            &root,
            r#"
IFS= read -r request
printf '%s\n' 'not-json'
            "#,
        );
        let workspace = prepared_workspace(&root);

        let error = prepare_with_app_server(&executable, &workspace)
            .expect_err("malformed app-server response should fail setup");
        let message = error.to_string();

        assert!(message.contains("initialize"));
        assert!(message.contains("not-json"));
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[cfg(unix)]
    #[test]
    fn reports_failed_resume_handoff_with_thread_context() {
        let root = std::env::temp_dir().join(format!(
            "trees-codex-handoff-error-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(&root).expect("test workspace should be created");
        let prepared = PreparedLaunch {
            codex_home: root.join("codex-home"),
            project_id: "project-id".to_owned(),
            thread_id: "thread-id".to_owned(),
            cwd: root.clone(),
            runtime_roots: vec![root.join("one")],
            developer_instructions: "workspace instructions".to_owned(),
            codex_args: Vec::new(),
        };
        let executable = root.join("missing-codex");

        let error = handoff(&executable, &prepared).expect_err("missing resume binary should fail");
        let message = error.to_string();

        assert!(message.contains("project-id"));
        assert!(message.contains("thread-id"));
        assert!(message.contains("missing-codex"));
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn formats_launch_errors_and_exposes_sources() {
        let errors = [
            CodexLaunchError::DatabaseOpen(crate::database::DatabaseError::Path(
                crate::paths::PathError::HomeDirectoryUnavailable,
            )),
            CodexLaunchError::Workspace(WorkspacePreparationError::NoWorktrees),
            CodexLaunchError::AppServer(AppServerError::Transport("closed".to_owned())),
            CodexLaunchError::Config(AppServerError::Transport("closed".to_owned())),
            CodexLaunchError::InvalidConfigResponse("invalid config".to_owned()),
            CodexLaunchError::Project(ProjectSyncError::InvalidInput("invalid project".to_owned())),
            CodexLaunchError::Thread(ThreadStartError::InvalidInput("invalid thread".to_owned())),
            CodexLaunchError::Shutdown(AppServerError::Transport("closed".to_owned())),
            CodexLaunchError::Handoff {
                executable: PathBuf::from("codex"),
                project_id: "project-id".to_owned(),
                thread_id: "thread-id".to_owned(),
                source: io::Error::new(io::ErrorKind::NotFound, "missing"),
            },
            CodexLaunchError::SetupAndShutdown {
                setup: Box::new(CodexLaunchError::Thread(ThreadStartError::InvalidInput(
                    "invalid thread".to_owned(),
                ))),
                shutdown: AppServerError::Transport("closed".to_owned()),
            },
        ];

        for error in errors {
            assert!(!error.to_string().is_empty());
            let _ = std::error::Error::source(&error);
        }
    }

    #[cfg(unix)]
    #[test]
    fn formats_setup_process_exit_error() {
        let status = std::process::Command::new("false")
            .status()
            .expect("false should run");
        let error = CodexLaunchError::SetupProcessExit(status);

        assert!(error.to_string().contains("setup app-server"));
        assert!(std::error::Error::source(&error).is_none());
    }
}
