use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::Duration;

use crate::codex::app_server::{AppServerError, AppServerProcess};
use crate::codex::project_sync::{ProjectSession, ProjectSyncError, ProjectSynchronizer};
use crate::codex::thread::{start_thread, ThreadStartError};
use crate::codex::workspace::{prepare, PreparedWorkspace, WorkspacePreparationError};

const SETUP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SETUP_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct LaunchRequest {
    pub workspace_path: PathBuf,
    pub codex_bin: PathBuf,
}

#[derive(Debug, Clone)]
pub struct PreparedLaunch {
    pub codex_home: PathBuf,
    pub project_id: String,
    pub thread_id: String,
    pub primary_root: PathBuf,
}

pub fn launch(request: LaunchRequest) -> Result<ExitStatus, CodexLaunchError> {
    let codex_bin = request.codex_bin.clone();
    let prepared = prepare_launch(request)?;
    handoff(&codex_bin, &prepared)
}

pub fn prepare_launch(request: LaunchRequest) -> Result<PreparedLaunch, CodexLaunchError> {
    let mut connection = crate::database::open_default().map_err(CodexLaunchError::DatabaseOpen)?;
    let workspace =
        prepare(&mut connection, &request.workspace_path).map_err(CodexLaunchError::Workspace)?;
    prepare_with_app_server(&request.codex_bin, &workspace)
}

fn prepare_with_app_server(
    codex_bin: &Path,
    workspace: &PreparedWorkspace,
) -> Result<PreparedLaunch, CodexLaunchError> {
    let mut app_server = AppServerProcess::spawn(codex_bin).map_err(CodexLaunchError::AppServer)?;
    let setup_result = (|| {
        let project = ProjectSynchronizer::new(&mut app_server, SETUP_REQUEST_TIMEOUT)
            .synchronize(&workspace.id, &workspace.name, &workspace.roots)
            .map_err(CodexLaunchError::Project)?;
        let thread = start_thread(
            &mut app_server,
            &project.project.id,
            &workspace.roots,
            SETUP_REQUEST_TIMEOUT,
        )
        .map_err(CodexLaunchError::Thread)?;
        Ok::<PreparedLaunch, CodexLaunchError>(prepared_launch(
            workspace,
            project,
            thread.thread.id,
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

fn prepared_launch(
    workspace: &PreparedWorkspace,
    project: ProjectSession,
    thread_id: String,
) -> PreparedLaunch {
    PreparedLaunch {
        codex_home: project.codex_home,
        project_id: project.project.id,
        thread_id,
        primary_root: workspace.roots[0].clone(),
    }
}

pub fn handoff(
    codex_bin: &Path,
    prepared: &PreparedLaunch,
) -> Result<ExitStatus, CodexLaunchError> {
    Command::new(codex_bin)
        .arg("resume")
        .arg(&prepared.thread_id)
        .current_dir(&prepared.primary_root)
        .status()
        .map_err(|source| CodexLaunchError::Handoff {
            executable: codex_bin.to_owned(),
            project_id: prepared.project_id.clone(),
            thread_id: prepared.thread_id.clone(),
            source,
        })
}

#[derive(Debug)]
pub enum CodexLaunchError {
    DatabaseOpen(crate::database::DatabaseError),
    Workspace(WorkspacePreparationError),
    AppServer(AppServerError),
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
            Self::AppServer(error) => error.fmt(formatter),
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
            Self::AppServer(error) => Some(error),
            Self::Project(error) => Some(error),
            Self::Thread(error) => Some(error),
            Self::Shutdown(error) => Some(error),
            Self::Handoff { source, .. } => Some(source),
            Self::SetupAndShutdown { setup, .. } => Some(setup),
            Self::SetupProcessExit(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[cfg(unix)]
    #[test]
    fn hands_thread_to_resume_with_inherited_terminal_context() {
        use std::os::unix::fs::PermissionsExt;

        let root =
            std::env::temp_dir().join(format!("trees-codex-handoff-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("handoff root should be created");
        let capture = root.join("capture");
        let executable = root.join("fake-codex");
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\nprintf '%s\\n%s\\n%s\\n%s\\n' \"$1\" \"$2\" \"$PWD\" \"$PATH\" > '{}'\nexit 7\n",
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
            primary_root: root.clone(),
        };
        let status = handoff(&executable, &prepared).expect("handoff should start");

        assert_eq!(status.code(), Some(7));
        let captured = fs::read_to_string(&capture).expect("fake executable should capture input");
        let lines: Vec<_> = captured.lines().collect();
        let canonical_root = fs::canonicalize(&root).expect("handoff root should be canonical");
        assert_eq!(lines[0], "resume");
        assert_eq!(lines[1], "thread-id");
        assert_eq!(Path::new(lines[2]), canonical_root.as_path());
        assert!(!lines[3].is_empty());

        fs::remove_dir_all(root).expect("handoff test root should be removable");
    }
}
