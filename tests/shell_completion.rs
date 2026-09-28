use std::fs;
use std::path::PathBuf;
use std::process::Command;

use trees::domain::WorkspaceId;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_trees")
}

fn temporary_home() -> PathBuf {
    let path = std::env::temp_dir().join(format!("trees-completion-{}", WorkspaceId::new()));
    fs::create_dir(&path).expect("temporary home should be created");
    path
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
