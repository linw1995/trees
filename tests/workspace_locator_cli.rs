#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use trees::domain::WorkspaceId;

fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_trees"));
    command
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("LOCALAPPDATA", root.join("local-app-data"))
        .env("APPDATA", root.join("app-data"));
    command
}

fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("trees-locator-cli-{}", WorkspaceId::new()));
    fs::create_dir_all(root.join("home")).unwrap();
    root
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
fn named_selectors_reach_the_same_workspace_across_commands() {
    let root = root();
    let repo = root.join("source");
    fs::create_dir_all(&repo).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "trees@example.invalid"],
        vec!["config", "user.name", "trees tests"],
        vec!["commit", "--allow-empty", "-qm", "initial"],
    ] {
        success(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap(),
        );
    }
    for release_option in ["--workspace-id", "--workspace-dir", "--claim-id"] {
        let created = success(
            command(&root)
                .args(["create", "--repo", "source", "--offline", "--json"])
                .output()
                .unwrap(),
        );
        let created: Value = serde_json::from_slice(&created.stdout).unwrap();
        let path = created["workspace_path"].as_str().unwrap();
        let claim = created["claim_id"].as_str().unwrap();
        let status = success(
            command(&root)
                .args(["status", "--workspace-dir", path, "--json"])
                .output()
                .unwrap(),
        );
        let status: Value = serde_json::from_slice(&status.stdout).unwrap();
        let id = status["target_workspace"]["workspace_id"].as_str().unwrap();
        for (option, value, heading) in [
            ("--workspace-id", id, "selected by ID"),
            ("--workspace-dir", path, "selected by path"),
            ("--claim-id", claim, "selected by claim"),
        ] {
            for view in ["pools", "workspaces", "repos"] {
                let output = success(
                    command(&root)
                        .args(["status", option, value, "--view", view, "--json"])
                        .output()
                        .unwrap(),
                );
                let report: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(report["target_workspace"]["workspace_id"], id);
                assert_eq!(report["schema_version"], 2);
                let output = success(
                    command(&root)
                        .args(["status", option, value, "--view", view])
                        .output()
                        .unwrap(),
                );
                assert!(String::from_utf8_lossy(&output.stdout).contains(heading));
            }
            let opened = success(
                command(&root)
                    .args(["open", option, value, "--program=pwd"])
                    .output()
                    .unwrap(),
            );
            assert_eq!(String::from_utf8_lossy(&opened.stdout).trim(), path);
        }
        fs::create_dir_all(Path::new(path).join("child")).unwrap();
        let missing_id = WorkspaceId::new().to_string();
        for command_name in ["status", "open", "release"] {
            for selector in [
                vec!["--workspace-id", missing_id.as_str()],
                vec!["--claim-id", missing_id.as_str()],
                vec!["--workspace-dir", "child"],
            ] {
                let mut cmd = command(&root);
                cmd.current_dir(path).arg(command_name).args(selector);
                if command_name == "open" {
                    cmd.arg("--program=pwd");
                }
                let output = cmd.output().unwrap();
                assert!(!output.status.success());
                assert!(output.stdout.is_empty());
            }
        }
        let selected = match release_option {
            "--workspace-id" => id,
            "--workspace-dir" => path,
            _ => claim,
        };
        let released = success(
            command(&root)
                .args(["release", release_option, selected])
                .output()
                .unwrap(),
        );
        assert!(String::from_utf8_lossy(&released.stdout).contains(&format!("claim_id={claim}")));
        let missing = command(&root)
            .args(["status", "--claim-id", claim, "--json"])
            .output()
            .unwrap();
        assert!(!missing.status.success());
        assert!(missing.stdout.is_empty());
        for selector in [vec!["--workspace-id", id], vec!["--workspace-dir", path]] {
            let rejected = command(&root)
                .arg("open")
                .args(selector)
                .arg("--program=pwd")
                .output()
                .unwrap();
            assert!(!rejected.status.success());
            assert!(String::from_utf8_lossy(&rejected.stderr)
                .contains("automatic workspace is unclaimed"));
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_misses_and_conflicts_do_not_fall_back_to_current_directory() {
    let root = root();
    let id = WorkspaceId::new().to_string();
    for command_name in ["status", "open", "release"] {
        for selector in [
            vec!["--workspace-id", &id],
            vec!["--workspace-dir", "."],
            vec!["--claim-id", &id],
        ] {
            let mut cmd = command(&root);
            cmd.arg(command_name).args(selector);
            if command_name == "open" {
                cmd.arg("--program=pwd");
            }
            let output = cmd.output().unwrap();
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
        }
        let conflict = command(&root)
            .args([
                command_name,
                "--workspace-dir",
                "missing/parent/path",
                "--claim-id",
                "invalid",
            ])
            .output()
            .unwrap();
        assert!(!conflict.status.success());
        assert!(conflict.stdout.is_empty());
        assert!(String::from_utf8_lossy(&conflict.stderr).contains("cannot be used with"));
    }
    fs::remove_dir_all(root).unwrap();
}
