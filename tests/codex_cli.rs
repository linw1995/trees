#[cfg(unix)]
mod unix {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use std::os::unix::fs::PermissionsExt;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("trees-codex-cli-{}", uuid::Uuid::now_v7()))
    }

    fn run_git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
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
        fs::write(path.join("README"), format!("{name}\n")).expect("README should be written");
        run_git(&path, &["add", "README"]);
        run_git(&path, &["commit", "-qm", "initial"]);
        path
    }

    fn worktree_snapshot(repository: &Path) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(["worktree", "list", "--porcelain"])
            .output()
            .expect("git should list worktrees");
        assert!(output.status.success(), "git worktree list should succeed");
        String::from_utf8(output.stdout).expect("worktree output should be UTF-8")
    }

    fn fake_codex(root: &Path, capture: &Path) -> PathBuf {
        let executable = root.join("fake-codex");
        let script = format!(
            r##"#!/bin/sh
if [ "$1" = "app-server" ]; then
    while IFS= read -r request; do
        case "$request" in
            *'"method":"initialize"'*)
                printf '%s\n' '{{"id":1,"result":{{"codexHome":"/tmp/codex"}}}}'
                ;;
            *'"method":"project/create"'*)
                printf '%s\n' '{{"id":2,"result":{{"project":{{"id":"project-id","name":"workspace","roots":[],"metadata":{{}}}}}}}}'
                ;;
            *'"method":"project/update"'*)
                printf '%s\n' '{{"id":3,"result":{{"project":{{"id":"project-id","name":"workspace","roots":[],"metadata":{{}}}}}}}}'
                ;;
            *'"method":"config/read"'*)
                printf '%s\n' '{{"id":4,"result":{{"config":{{"developer_instructions":"Keep user rules."}}}}}}'
                ;;
            *'"method":"thread/start"'*)
                printf '%s\n' '{{"id":5,"result":{{"thread":{{"id":"thread-id"}}}}}}'
                ;;
        esac
    done
    exit 0
fi
printf '%s\n' "$@" > '{}'
exit 0
"##,
            capture.display()
        );
        fs::write(&executable, script).expect("fake Codex should be written");
        let mut permissions = fs::metadata(&executable)
            .expect("fake Codex metadata should be available")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).expect("fake Codex should be executable");
        executable
    }

    #[test]
    fn dispatches_direct_native_arguments_without_mutating_git_worktrees() {
        let root = test_root();
        fs::create_dir_all(&root).expect("test root should be created");
        let home = root.join("home");
        let state_home = root.join("state");
        fs::create_dir_all(&home).expect("home should be created");
        fs::create_dir_all(&state_home).expect("state home should be created");

        let first = repository(&root, "alpha");
        let second = repository(&root, "beta");
        let workspace = root.join("workspace");
        let binary = env!("CARGO_BIN_EXE_trees");
        let create = Command::new(binary)
            .args([
                "create",
                workspace.to_str().expect("workspace should be UTF-8"),
                "--repo",
                first.to_str().expect("first repository should be UTF-8"),
                "--repo",
                second.to_str().expect("second repository should be UTF-8"),
            ])
            .env("HOME", &home)
            .env("XDG_STATE_HOME", &state_home)
            .output()
            .expect("trees create should run");
        assert!(
            create.status.success(),
            "trees create failed: {}",
            String::from_utf8_lossy(&create.stderr)
        );

        let before_first = worktree_snapshot(&first);
        let before_second = worktree_snapshot(&second);
        let capture = root.join("codex-args");
        let fake_codex = fake_codex(&root, &capture);
        let launch = Command::new(binary)
            .args([
                "codex",
                "--codex-bin",
                fake_codex.to_str().expect("fake Codex should be UTF-8"),
                "-C",
                workspace.to_str().expect("workspace should be UTF-8"),
                "--model",
                "smoke",
                "--add-dir",
                root.to_str().expect("test root should be UTF-8"),
            ])
            .env("HOME", &home)
            .env("XDG_STATE_HOME", &state_home)
            .output()
            .expect("trees codex should run");
        assert!(
            launch.status.success(),
            "trees codex failed: {}",
            String::from_utf8_lossy(&launch.stderr)
        );

        let captured = fs::read_to_string(&capture).expect("final Codex arguments should exist");
        assert!(captured.lines().any(|line| line == "resume"));
        assert!(captured.lines().any(|line| line == "thread-id"));
        assert!(captured.lines().any(|line| line == "--model"));
        assert!(captured.lines().any(|line| line == "smoke"));
        assert!(captured.lines().any(|line| line == "--add-dir"));
        let first_worktree = fs::canonicalize(workspace.join("alpha"))
            .expect("first managed worktree should be canonical");
        let second_worktree = fs::canonicalize(workspace.join("beta"))
            .expect("second managed worktree should be canonical");
        assert!(captured
            .lines()
            .any(|line| line == first_worktree.to_string_lossy()));
        assert!(captured
            .lines()
            .any(|line| line == second_worktree.to_string_lossy()));

        assert_eq!(worktree_snapshot(&first), before_first);
        assert_eq!(worktree_snapshot(&second), before_second);

        fs::remove_dir_all(root).expect("test root should be removable");
    }
}
