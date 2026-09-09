use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use diesel::sqlite::SqliteConnection;
use serde_json::{json, Value};

use trees::codex::app_server::{AppServerError, RpcClient};
use trees::codex::project::{Project, ProjectCreateResponse, ProjectRoot, ProjectUpdateResponse};
use trees::codex::project_idempotency_key;
use trees::codex::project_sync::{ProjectSession, ProjectSynchronizer};
use trees::codex::thread::{start_thread, ThreadStartResponse};
use trees::codex::workspace::{prepare, PreparedWorkspace};
use trees::workspace::{create_with_connection, prepare_create, CreateRequest};

#[derive(Debug, Clone)]
struct Request {
    method: String,
    params: Value,
}

struct FakeRpc {
    codex_home: PathBuf,
    projects: HashMap<String, Project>,
    project_keys: BTreeMap<String, String>,
    deleted_keys: HashSet<String>,
    next_project: usize,
    next_thread: usize,
    requests: Vec<Request>,
    notifications: Vec<String>,
}

impl FakeRpc {
    fn new(codex_home: PathBuf) -> Self {
        Self {
            codex_home,
            projects: HashMap::new(),
            project_keys: BTreeMap::new(),
            deleted_keys: HashSet::new(),
            next_project: 1,
            next_thread: 1,
            requests: Vec::new(),
            notifications: Vec::new(),
        }
    }

    fn delete_project(&mut self, idempotency_key: &str) {
        let project_id = self
            .project_keys
            .remove(idempotency_key)
            .expect("project key should exist");
        self.projects
            .remove(&project_id)
            .expect("project should exist");
        self.deleted_keys.insert(idempotency_key.to_owned());
    }

    fn replace_roots(&mut self, idempotency_key: &str, roots: &[&str]) {
        let project_id = self
            .project_keys
            .get(idempotency_key)
            .expect("project key should exist")
            .clone();
        let project = self
            .projects
            .get_mut(&project_id)
            .expect("project should exist");
        project.roots = roots
            .iter()
            .map(|path| ProjectRoot {
                path: PathBuf::from(path),
            })
            .collect();
    }

    fn project_key_for(&self, project_id: &str) -> &str {
        self.project_keys
            .iter()
            .find_map(|(key, value)| (value == project_id).then_some(key.as_str()))
            .expect("project key should exist")
    }

    fn request(&self, method: &str) -> &Request {
        self.requests
            .iter()
            .find(|request| request.method == method)
            .expect("request should exist")
    }

    fn request_count(&self, method: &str) -> usize {
        self.requests
            .iter()
            .filter(|request| request.method == method)
            .count()
    }

    fn create_project(&mut self, params: Value) -> Result<Value, AppServerError> {
        let idempotency_key = params["idempotencyKey"]
            .as_str()
            .expect("project create should contain an idempotency key")
            .to_owned();
        if self.deleted_keys.contains(&idempotency_key) {
            return Err(AppServerError::Remote {
                method: "project/create".to_owned(),
                error: "idempotency key refers to deleted project".to_owned(),
            });
        }

        if let Some(project_id) = self.project_keys.get(&idempotency_key) {
            return Ok(project_create_response(
                self.projects
                    .get(project_id)
                    .expect("project key should point to a project"),
            ));
        }

        let project_id = format!(
            "{}-project-{}",
            self.codex_home
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("codex"),
            self.next_project
        );
        self.next_project += 1;
        let project = Project {
            id: project_id.clone(),
            name: params["name"]
                .as_str()
                .expect("project create should contain a name")
                .to_owned(),
            roots: project_roots(&params["roots"]),
            metadata: serde_json::from_value(params["metadata"].clone())
                .expect("project metadata should be a map"),
        };
        self.project_keys
            .insert(idempotency_key, project_id.clone());
        self.projects.insert(project_id, project.clone());
        Ok(project_create_response(&project))
    }

    fn update_project(&mut self, params: Value) -> Result<Value, AppServerError> {
        let project_id = params["projectId"]
            .as_str()
            .expect("project update should contain a project id")
            .to_owned();
        let project = self
            .projects
            .get_mut(&project_id)
            .ok_or_else(|| AppServerError::Remote {
                method: "project/update".to_owned(),
                error: "project not found".to_owned(),
            })?;
        project.roots = project_roots(&params["roots"]);
        Ok(serde_json::to_value(ProjectUpdateResponse {
            project: project.clone(),
        })
        .expect("project update should serialize"))
    }
}

impl RpcClient for FakeRpc {
    fn request(
        &mut self,
        method: &str,
        params: Value,
        _timeout: Duration,
    ) -> Result<Value, AppServerError> {
        self.requests.push(Request {
            method: method.to_owned(),
            params: params.clone(),
        });
        match method {
            "initialize" => Ok(json!({"codexHome": self.codex_home})),
            "project/create" => self.create_project(params),
            "project/update" => self.update_project(params),
            "project/list" => Ok(json!({
                "data": self.projects.values().cloned().collect::<Vec<_>>(),
                "nextCursor": null
            })),
            "thread/start" => {
                let thread_id = format!("thread-{}", self.next_thread);
                self.next_thread += 1;
                Ok(json!({"thread": {"id": thread_id}}))
            }
            _ => Err(AppServerError::Remote {
                method: method.to_owned(),
                error: "method not found".to_owned(),
            }),
        }
    }

    fn notify(&mut self, method: &str, _params: Value) -> Result<(), AppServerError> {
        self.notifications.push(method.to_owned());
        Ok(())
    }
}

fn project_create_response(project: &Project) -> Value {
    serde_json::to_value(ProjectCreateResponse {
        project: project.clone(),
    })
    .expect("project create should serialize")
}

fn object_keys(value: &Value) -> Vec<String> {
    let mut keys = value
        .as_object()
        .expect("params should be an object")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

fn project_roots(value: &Value) -> Vec<ProjectRoot> {
    value
        .as_array()
        .expect("project roots should be an array")
        .iter()
        .map(|root| ProjectRoot {
            path: PathBuf::from(
                root["path"]
                    .as_str()
                    .expect("project root should contain a path"),
            ),
        })
        .collect()
}

fn synchronize(
    rpc: &mut FakeRpc,
    workspace: &PreparedWorkspace,
) -> (ProjectSession, ThreadStartResponse) {
    let project = ProjectSynchronizer::new(rpc, Duration::from_secs(1))
        .synchronize(&workspace.id, &workspace.name, &workspace.roots)
        .expect("project should synchronize");
    let thread = start_thread(
        rpc,
        &project.project.id,
        workspace.path.as_path(),
        &workspace.roots,
        Duration::from_secs(1),
    )
    .expect("thread should start");
    (project, thread)
}

fn test_root() -> PathBuf {
    std::env::temp_dir().join(format!("trees-codex-launch-{}", uuid::Uuid::now_v7()))
}

fn run_git(path: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(&path).expect("repository should be created");
    run_git(&path, &["init", "-q"]);
    run_git(&path, &["config", "user.email", "trees@example.invalid"]);
    run_git(&path, &["config", "user.name", "trees tests"]);
    fs::write(path.join("README"), format!("{name}\n")).expect("file should be written");
    run_git(&path, &["add", "README"]);
    run_git(&path, &["commit", "-qm", "initial"]);
    path
}

fn managed_workspace(root: &Path) -> (PreparedWorkspace, PathBuf, SqliteConnection) {
    let first = repository(root, "alpha");
    let second = repository(root, "beta");
    let workspace_path = root.join("workspace");
    let plan = prepare_create(&CreateRequest {
        workspace_path: workspace_path.clone(),
        repositories: vec![first, second],
        offline: false,
    })
    .expect("creation plan should be prepared");
    let database_path = root.join("state.sqlite");
    let mut connection = trees::database::connect(&database_path).expect("database should open");
    create_with_connection(&mut connection, plan).expect("workspace should be created");
    let workspace = prepare(&mut connection, &workspace_path).expect("workspace should prepare");
    (workspace, database_path, connection)
}

fn cleanup(root: PathBuf, database_path: PathBuf, connection: SqliteConnection) {
    drop(connection);
    fs::remove_file(database_path).expect("database should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}

#[test]
fn repeated_launch_reuses_project_and_starts_new_threads() {
    let root = test_root();
    let (workspace, database_path, connection) = managed_workspace(&root);
    let mut rpc = FakeRpc::new(root.join("codex-home"));

    let (first_project, first_thread) = synchronize(&mut rpc, &workspace);
    let (second_project, second_thread) = synchronize(&mut rpc, &workspace);

    assert_eq!(first_project.project.id, second_project.project.id);
    assert_ne!(first_thread.thread.id, second_thread.thread.id);
    assert_eq!(rpc.request_count("project/create"), 2);
    assert_eq!(rpc.project_keys.len(), 1);
    assert_eq!(
        rpc.project_keys
            .keys()
            .next()
            .expect("project key should exist"),
        &project_idempotency_key(&workspace.id)
    );
    assert_eq!(rpc.notifications, ["initialized", "initialized"]);

    cleanup(root, database_path, connection);
}

#[test]
fn changed_roots_replace_the_complete_project_root_list() {
    let root = test_root();
    let (workspace, database_path, connection) = managed_workspace(&root);
    let mut rpc = FakeRpc::new(root.join("codex-home"));
    let key = project_idempotency_key(&workspace.id);

    synchronize(&mut rpc, &workspace);
    rpc.replace_roots(&key, &["/stale/root"]);
    let (project, _) = synchronize(&mut rpc, &workspace);

    assert_eq!(project.project.roots.len(), workspace.roots.len());
    assert_eq!(
        project
            .project
            .roots
            .iter()
            .map(|root| &root.path)
            .collect::<Vec<_>>(),
        workspace.roots.iter().collect::<Vec<_>>()
    );
    let update = rpc.request("project/update");
    assert_eq!(
        update.params["roots"]
            .as_array()
            .expect("roots should exist")
            .len(),
        workspace.roots.len()
    );

    cleanup(root, database_path, connection);
}

#[test]
fn external_project_deletion_recovers_with_a_fresh_key() {
    let root = test_root();
    let (workspace, database_path, connection) = managed_workspace(&root);
    let mut rpc = FakeRpc::new(root.join("codex-home"));
    let key = project_idempotency_key(&workspace.id);

    let (first_project, _) = synchronize(&mut rpc, &workspace);
    rpc.delete_project(&key);
    let (recovered_project, _) = synchronize(&mut rpc, &workspace);

    assert_ne!(first_project.project.id, recovered_project.project.id);
    assert_eq!(rpc.request_count("project/list"), 1);
    assert_eq!(rpc.project_keys.len(), 1);
    assert!(rpc
        .project_key_for(&recovered_project.project.id)
        .starts_with(&format!("{key}:recovery:")));

    cleanup(root, database_path, connection);
}

#[test]
fn separate_codex_homes_keep_project_state_independent() {
    let root = test_root();
    let (workspace, database_path, connection) = managed_workspace(&root);
    let mut first_rpc = FakeRpc::new(root.join("codex-home-one"));
    let mut second_rpc = FakeRpc::new(root.join("codex-home-two"));

    let first = synchronize(&mut first_rpc, &workspace).0;
    let second = synchronize(&mut second_rpc, &workspace).0;

    assert_ne!(first.codex_home, second.codex_home);
    assert_ne!(first.project.id, second.project.id);
    assert_eq!(
        first_rpc.request("project/create").params["idempotencyKey"],
        second_rpc.request("project/create").params["idempotencyKey"]
    );

    cleanup(root, database_path, connection);
}

#[test]
fn launch_requests_preserve_user_security_defaults() {
    let root = test_root();
    let (workspace, database_path, connection) = managed_workspace(&root);
    let mut rpc = FakeRpc::new(root.join("codex-home"));

    synchronize(&mut rpc, &workspace);

    let initialize = rpc.request("initialize");
    assert_eq!(
        object_keys(&initialize.params),
        ["capabilities", "clientInfo"]
    );
    assert_eq!(
        initialize.params["capabilities"],
        json!({"experimentalApi": true})
    );
    let create = rpc.request("project/create");
    assert_eq!(
        object_keys(&create.params),
        ["idempotencyKey", "metadata", "name", "roots"]
    );
    let thread = rpc.request("thread/start");
    assert_eq!(
        object_keys(&thread.params),
        ["cwd", "projectId", "runtimeWorkspaceRoots"]
    );
    assert_eq!(rpc.notifications, ["initialized".to_owned()]);
    assert!(workspace.path.as_path().is_absolute());

    cleanup(root, database_path, connection);
}
