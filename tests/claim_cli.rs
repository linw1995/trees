#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

struct Fixture {
    root: PathBuf,
    workspace: String,
    id: String,
}

impl Fixture {
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_trees"));
        command
            .current_dir(&self.root)
            .env("HOME", self.root.join("home"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("LOCALAPPDATA", self.root.join("local-app-data"))
            .env("APPDATA", self.root.join("app-data"));
        command
    }

    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("trees-claim-'$()-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(root.join("home")).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "trees@example.invalid"],
            vec!["config", "user.name", "trees tests"],
            vec!["commit", "--allow-empty", "-qm", "initial"],
        ] {
            success(
                Command::new("git")
                    .arg("-C")
                    .arg(&source)
                    .args(args)
                    .output()
                    .unwrap(),
            );
        }
        let mut fixture = Self {
            root,
            workspace: String::new(),
            id: String::new(),
        };
        let output = success(
            fixture
                .command()
                .args(["create", "--offline", "--repo"])
                .arg(source)
                .arg("--json")
                .output()
                .unwrap(),
        );
        let created: Value = serde_json::from_slice(&output.stdout).unwrap();
        fixture.workspace = created["workspace_path"].as_str().unwrap().to_owned();
        fixture.id = fixture.status()["target_workspace"]["workspace_id"]
            .as_str()
            .unwrap()
            .to_owned();
        fixture.release();
        fixture
    }

    fn status(&self) -> Value {
        let output = success(
            self.command()
                .args(["status", "--workspace-dir", &self.workspace, "--json"])
                .output()
                .unwrap(),
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn release(&self) {
        success(
            self.command()
                .args(["release", &self.workspace])
                .output()
                .unwrap(),
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn claim_cli_selectors_output_and_release_round_trip() {
    let fixture = Fixture::new();
    let child = PathBuf::from(&fixture.workspace).join("child");
    fs::create_dir(&child).unwrap();
    for mode in ["id", "relative", "cwd"] {
        let mut command = fixture.command();
        command.args(["claim", "--json"]);
        match mode {
            "id" => {
                command.args(["--workspace-id", &fixture.id]);
            }
            "relative" => {
                command.arg(
                    PathBuf::from(&fixture.workspace)
                        .strip_prefix(fs::canonicalize(&fixture.root).unwrap())
                        .unwrap(),
                );
            }
            "cwd" => {
                command.current_dir(&child);
            }
            _ => unreachable!(),
        }
        let output = success(command.output().unwrap());
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result.as_object().unwrap().len(), 4);
        assert_eq!(result["workspace_id"], fixture.id);
        assert_eq!(result["workspace_path"], fixture.workspace);
        assert!(result["pool_id"]
            .as_str()
            .unwrap()
            .parse::<trees::domain::PoolId>()
            .is_ok());
        let status = fixture.status();
        assert_eq!(
            status["target_workspace"]["claim"]["claim_id"],
            result["claim_id"]
        );
        let duplicate = fixture
            .command()
            .args(["claim", &fixture.workspace])
            .output()
            .unwrap();
        assert!(!duplicate.status.success());
        assert!(duplicate.stdout.is_empty());
        assert!(String::from_utf8_lossy(&duplicate.stderr).contains("active claim"));
        fixture.release();
        assert!(fixture.status()["target_workspace"]["claim"].is_null());
    }
    let output = success(
        fixture
            .command()
            .args(["claim", &fixture.workspace])
            .output()
            .unwrap(),
    );
    let assignments = String::from_utf8(output.stdout).unwrap();
    assert_eq!(assignments.lines().count(), 4);
    let script = format!("{assignments}\nprintf '%s\\n' \"$WORKSPACE_ID\" \"$WORKSPACE_PATH\" \"$POOL_ID\" \"$CLAIM_ID\"");
    let evaluated = success(Command::new("bash").args(["-c", &script]).output().unwrap());
    let values = String::from_utf8(evaluated.stdout).unwrap();
    let values: Vec<_> = values.lines().collect();
    assert_eq!(values[0], fixture.id);
    assert_eq!(values[1], fixture.workspace);
    assert!(values[2].parse::<trees::domain::PoolId>().is_ok());
    assert_eq!(
        values[3],
        fixture.status()["target_workspace"]["claim"]["claim_id"]
            .as_str()
            .unwrap()
    );
}

#[test]
fn claim_cli_rejects_exact_child_and_unsupported_options() {
    let fixture = Fixture::new();
    let child = PathBuf::from(&fixture.workspace).join("child");
    fs::create_dir(&child).unwrap();
    let output = fixture.command().arg("claim").arg(&child).output().unwrap();
    assert!(!output.status.success());
    for args in [
        vec!["claim", "--workspace-dir", &fixture.workspace],
        vec!["claim", "--claim-id", "invalid"],
        vec!["claim", &fixture.workspace, "--workspace-id", &fixture.id],
    ] {
        let output = fixture.command().args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    assert!(fixture.status()["target_workspace"]["claim"].is_null());
}

#[test]
fn claim_cli_retains_the_claim_after_output_failure() {
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::process::Stdio;

    let fixture = Fixture::new();
    let (writer, reader) = UnixStream::pair().unwrap();
    drop(reader);
    let output = fixture
        .command()
        .args(["claim", &fixture.workspace])
        .stdout(Stdio::from(OwnedFd::from(writer)))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("claim committed but output failed"));
    let status = fixture.status();
    assert!(status["target_workspace"]["claim"]["claim_id"].is_string());
    let duplicate = fixture
        .command()
        .args(["claim", &fixture.workspace])
        .output()
        .unwrap();
    assert!(!duplicate.status.success());
    fixture.release();
}
