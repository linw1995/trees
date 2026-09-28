use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use trees::domain::{
    CanonicalPath, ClaimId, OriginRepositoryId, Timestamp, WorkspaceId, WorkspaceState,
};
use trees::storage::{NewOriginRepository, NewWorkspace, NewWorkspaceClaim};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_trees")
}

fn temporary_home() -> PathBuf {
    let path = std::env::temp_dir().join(format!("trees-completion-{}", WorkspaceId::new()));
    fs::create_dir(&path).expect("temporary home should be created");
    path
}

fn database_path(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    let base = home.join("Library/Application Support");
    #[cfg(target_os = "linux")]
    let base = home.to_path_buf();
    base.join("trees/db.sqlite")
}

fn configuration_path(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    let base = home.join("Library/Application Support");
    #[cfg(target_os = "linux")]
    let base = home.join(".config");
    base.join("trees/config.toml")
}

fn completion_values(home: &Path, shell: &str, words: &[&str]) -> Vec<String> {
    completion_values_with_path(home, shell, words, None)
}

fn completion_values_with_path(
    home: &Path,
    shell: &str,
    words: &[&str],
    path: Option<&Path>,
) -> Vec<String> {
    let mut command = Command::new(binary());
    command
        .arg("--")
        .args(words)
        .env("TREES_COMPLETE", shell)
        .env("_CLAP_COMPLETE_INDEX", (words.len() - 1).to_string())
        .env("HOME", home)
        .env("XDG_STATE_HOME", home);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let output = command.output().expect("completion should run");
    assert!(output.status.success(), "{shell}: {output:?}");
    assert!(output.stderr.is_empty(), "{shell}: {output:?}");
    String::from_utf8(output.stdout)
        .expect("completion should be UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

struct IdFixture {
    home: PathBuf,
    active: WorkspaceId,
    removed: WorkspaceId,
    collision: WorkspaceId,
    origin: OriginRepositoryId,
    claim: ClaimId,
}

impl IdFixture {
    fn new() -> Self {
        let home = temporary_home();
        let path = database_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut connection = trees::database::connect(&path).unwrap();
        let active = WorkspaceId::new();
        let removed = WorkspaceId::new();
        let collision = WorkspaceId::new();
        for (id, state) in [
            (active, WorkspaceState::Ready),
            (removed, WorkspaceState::Removed),
            (collision, WorkspaceState::Ready),
        ] {
            let workspace_dir = home.join(id.to_string());
            fs::create_dir(&workspace_dir).unwrap();
            let workspace_path = CanonicalPath::resolve(workspace_dir).unwrap();
            trees::storage::insert_workspace(
                &mut connection,
                &NewWorkspace {
                    id,
                    canonical_path: workspace_path,
                    state,
                    created_at: Timestamp::now(),
                    updated_at: Timestamp::now(),
                    last_reconciled_at: None,
                },
            )
            .unwrap();
        }
        let claim = ClaimId::new();
        trees::storage::insert_workspace_claim(
            &mut connection,
            &NewWorkspaceClaim {
                id: claim,
                workspace_id: active,
                claimed_at: Timestamp::now(),
            },
        )
        .unwrap();
        let origin = OriginRepositoryId::new();
        for (id, name) in [
            (origin, "origin"),
            (collision.to_string().parse().unwrap(), "collision-origin"),
        ] {
            let origin_dir = home.join(name);
            fs::create_dir(&origin_dir).unwrap();
            let origin_path = CanonicalPath::resolve(origin_dir).unwrap();
            trees::storage::insert_origin_repository(
                &mut connection,
                &NewOriginRepository {
                    id,
                    repository_identity: origin_path.clone(),
                    source_path: origin_path,
                },
            )
            .unwrap();
        }
        drop(connection);
        Self {
            home,
            active,
            removed,
            collision,
            origin,
            claim,
        }
    }
}

impl Drop for IdFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.home).unwrap();
    }
}

#[test]
fn generates_shell_registrations_without_state() {
    let home = temporary_home();
    for shell in ["bash", "zsh"] {
        let output = Command::new(binary())
            .env("TREES_COMPLETE", shell)
            .env("HOME", &home)
            .env("XDG_STATE_HOME", &home)
            .output()
            .expect("completion registration should run");
        assert!(output.status.success(), "{shell}: {output:?}");
        assert!(output.stderr.is_empty(), "{shell}: {output:?}");
        assert!(!output.stdout.is_empty(), "{shell}: {output:?}");
        assert!(!home.join("trees").exists());
        assert!(!home.join("Library").exists());
    }
    fs::remove_dir(home).expect("temporary home should remain empty");
}

#[test]
fn documented_shell_commands_register_completion() {
    let home = temporary_home();
    for (shell, script) in [
        (
            "bash",
            "source <(TREES_COMPLETE=bash \"$1\"); complete -p trees",
        ),
        (
            "zsh",
            "autoload -Uz compinit; compinit -D; source <(TREES_COMPLETE=zsh \"$1\")",
        ),
    ] {
        let output = Command::new(shell)
            .args(["-c", script, "shell", binary()])
            .env("HOME", &home)
            .output()
            .expect("shell should start");
        assert!(output.status.success(), "{shell}: {output:?}");
        assert!(output.stderr.is_empty(), "{shell}: {output:?}");
    }
    fs::remove_dir_all(home).expect("temporary home should be removed");
}

#[test]
fn completes_every_workspace_and_claim_id_position() {
    let fixture = IdFixture::new();
    let active = fixture.active.to_string();
    let removed = fixture.removed.to_string();
    let claim = fixture.claim.to_string();

    for words in [
        vec!["trees", "add", "--workspace-id", &active],
        vec!["trees", "claim", "--workspace-id", &active],
        vec!["trees", "release", "--workspace-id", &active],
        vec!["trees", "status", &active],
        vec!["trees", "status", "--workspace-id", &active],
        vec!["trees", "open", "--workspace-id", &active],
    ] {
        assert!(
            completion_values(&fixture.home, "bash", &words).contains(&active),
            "{words:?}"
        );
    }
    for words in [
        vec!["trees", "add", "--claim-id", &claim],
        vec!["trees", "release", "--claim-id", &claim],
        vec!["trees", "status", "--claim-id", &claim],
        vec!["trees", "open", "--claim-id", &claim],
    ] {
        assert!(
            completion_values(&fixture.home, "zsh", &words).contains(&claim),
            "{words:?}"
        );
    }
    for words in [
        vec!["trees", "status", &removed],
        vec!["trees", "status", "--workspace-id", &removed],
    ] {
        assert!(completion_values(&fixture.home, "bash", &words).contains(&removed));
    }
    assert!(completion_values(
        &fixture.home,
        "bash",
        &["trees", "open", "--workspace-id", &removed]
    )
    .is_empty());

    let prefix = &active[..8];
    let ids = completion_values(
        &fixture.home,
        "bash",
        &["trees", "status", "--workspace-id", prefix],
    );
    assert!(ids.iter().all(|id| id.starts_with(prefix)));
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(!ids.contains(&fixture.origin.to_string()));
}

#[test]
fn completes_cross_entity_ids_without_ambiguous_values() {
    let fixture = IdFixture::new();
    let active = fixture.active.to_string();
    let origin = fixture.origin.to_string();
    let collision = fixture.collision.to_string();
    for command in ["open", "remove"] {
        assert!(
            completion_values(&fixture.home, "bash", &["trees", command, &active])
                .contains(&active)
        );
        assert!(
            completion_values(&fixture.home, "bash", &["trees", command, &origin])
                .contains(&origin)
        );
        assert!(
            completion_values(&fixture.home, "bash", &["trees", command, &collision]).is_empty()
        );
    }
    assert!(completion_values(
        &fixture.home,
        "bash",
        &["trees", "open", "--workspace-id", &collision]
    )
    .contains(&collision));
}

#[test]
fn missing_or_invalid_storage_produces_no_id_candidates() {
    let home = temporary_home();
    let words = ["trees", "status", "--workspace-id", ""];
    assert!(completion_values(&home, "bash", &words).is_empty());
    assert!(!database_path(&home).exists());

    let path = database_path(&home);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "not a database").unwrap();
    assert!(completion_values(&home, "zsh", &words).is_empty());
    assert_eq!(fs::read(&path).unwrap(), b"not a database");

    let output = Command::new(binary())
        .args(["status", "--workspace-id", &WorkspaceId::new().to_string()])
        .env("HOME", &home)
        .env("XDG_STATE_HOME", &home)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());

    fs::remove_dir_all(home).unwrap();
}

#[cfg(unix)]
#[test]
fn completion_does_not_run_configured_status_hooks() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = IdFixture::new();
    let marker = fixture.home.join("hook-ran");
    let git_marker = fixture.home.join("git-ran");
    let hook = fixture.home.join("hook.sh");
    fs::write(&hook, format!("#!/bin/sh\n: > '{}'\n", marker.display())).unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let fake_bin = fixture.home.join("bin");
    fs::create_dir(&fake_bin).unwrap();
    let fake_git = fake_bin.join("git");
    fs::write(
        &fake_git,
        format!("#!/bin/sh\n: > '{}'\n", git_marker.display()),
    )
    .unwrap();
    fs::set_permissions(&fake_git, fs::Permissions::from_mode(0o755)).unwrap();
    let config = configuration_path(&fixture.home);
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(
        config,
        format!(
            "[status.latest_session_hook]\nprogram = '{}'\n",
            hook.display()
        ),
    )
    .unwrap();

    let active = fixture.active.to_string();
    assert!(completion_values_with_path(
        &fixture.home,
        "bash",
        &["trees", "status", "--workspace-id", &active],
        Some(&fake_bin),
    )
    .contains(&active));
    assert!(!marker.exists());
    assert!(!git_marker.exists());
}
