use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("trees-config-cli-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_trees"));
        command
            .current_dir(&self.root)
            .env("HOME", self.root.join("home"))
            .env("USERPROFILE", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("APPDATA", self.root.join("config"))
            .env("LOCALAPPDATA", self.root.join("data"));
        command
    }

    fn config_path(&self) -> PathBuf {
        #[cfg(target_os = "macos")]
        let base = self.root.join("home/Library/Application Support");
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        let base = self.root.join("config");
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        let base = self.root.join("home/.config");
        base.join("trees/config.toml")
    }

    fn data_path(&self) -> PathBuf {
        #[cfg(target_os = "macos")]
        let base = self.root.join("home/Library/Application Support");
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        let base = self.root.join("data");
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        let base = self.root.join("home/.local/share");
        base.join("trees")
    }

    fn write_config(&self, document: &str) {
        let path = self.config_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, document).unwrap();
    }

    fn show(&self, json: bool) -> Output {
        let mut command = self.command();
        command.args(["config", "show"]);
        if json {
            command.arg("--json");
        }
        command.output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn show_defaults_without_initializing_storage() {
    let fixture = Fixture::new();
    let workspaces = fixture.data_path().join("workspaces");
    let origins = fixture.data_path().join("origins");
    assert_eq!(
        success(fixture.show(false)),
        format!(
            "workspaces_dir='{}'\norigins_dir='{}'\n",
            workspaces.display(),
            origins.display()
        )
    );
    let output = success(fixture.show(true));
    assert!(output.ends_with('\n'));
    assert_eq!(output.lines().count(), 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&output).unwrap(),
        serde_json::json!({
            "workspaces_dir": workspaces.to_string_lossy(),
            "origins_dir": origins.to_string_lossy(),
        })
    );
    assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 0);
}

#[test]
fn show_resolves_partial_configuration_and_preserves_contents() {
    let fixture = Fixture::new();
    let document = "[workspace]\nworkspaces_dir = 'nested/../workspaces'\n[other]\nvalue = 42\n";
    fixture.write_config(document);
    let workspace = fixture.config_path().parent().unwrap().join("workspaces");
    let origins = fixture.data_path().join("origins");
    assert_eq!(
        success(fixture.show(false)),
        format!(
            "workspaces_dir='{}'\norigins_dir='{}'\n",
            workspace.display(),
            origins.display()
        )
    );
    let value: serde_json::Value = serde_json::from_str(&success(fixture.show(true))).unwrap();
    assert_eq!(
        value["workspaces_dir"],
        workspace.to_string_lossy().as_ref()
    );
    assert_eq!(value["origins_dir"], origins.to_string_lossy().as_ref());
    assert_eq!(fs::read_to_string(fixture.config_path()).unwrap(), document);
    assert_eq!(
        fs::read_dir(fixture.config_path().parent().unwrap())
            .unwrap()
            .count(),
        1
    );
    assert!(!workspace.exists());
    assert!(!origins.exists());
}

#[test]
fn show_failures_leave_stdout_empty() {
    let fixture = Fixture::new();
    for document in [
        "[",
        "workspace = 1",
        "repository = false",
        "[workspace]\nworkspaces_dir = 42",
        "[repository]\norigins_dir = []",
    ] {
        fixture.write_config(document);
        for json in [false, true] {
            let output = fixture.show(json);
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8_lossy(&output.stderr).contains("configuration"));
        }
        assert_eq!(fs::read_to_string(fixture.config_path()).unwrap(), document);
    }
    fs::remove_file(fixture.config_path()).unwrap();
    fs::create_dir(fixture.config_path()).unwrap();
    for json in [false, true] {
        let output = fixture.show(json);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("filesystem"));
    }
}

#[test]
fn show_rejects_positional_arguments_and_unknown_flags() {
    let fixture = Fixture::new();
    for args in [
        vec!["config", "show", "workspaces-dir"],
        vec!["config", "show", "--unknown"],
        vec!["config", "set", "--json", "origins-dir", "sources"],
    ] {
        assert!(!fixture
            .command()
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
    let help = success(
        fixture
            .command()
            .args(["config", "show", "--help"])
            .output()
            .unwrap(),
    );
    assert!(help.contains("--json"));
}

#[cfg(unix)]
#[test]
fn bash_and_json_preserve_special_paths_without_executing_them() {
    let fixture = Fixture::new();
    let path = fixture
        .root
        .join("Alice's \"workspace\" $HOME $(touch injected) `touch injected` \\ tab\tline\nend");
    let value = path.to_string_lossy();
    let document = toml::to_string(&serde_json::json!({
        "workspace": { "workspaces_dir": value },
        "repository": { "origins_dir": value },
    }))
    .unwrap();
    fixture.write_config(&document);
    let bash = success(fixture.show(false));
    let script = fixture.root.join("settings.sh");
    fs::write(&script, bash).unwrap();
    let output = Command::new("bash")
        .args([
            "--noprofile",
            "--norc",
            "-c",
            "source \"$1\"; printf '%s\\0%s\\0' \"$workspaces_dir\" \"$origins_dir\"",
            "bash",
        ])
        .arg(&script)
        .current_dir(&fixture.root)
        .output()
        .unwrap();
    assert_eq!(success(output), format!("{value}\0{value}\0"));
    assert!(!fixture.root.join("injected").exists());
    let json: serde_json::Value = serde_json::from_str(&success(fixture.show(true))).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "workspaces_dir": value, "origins_dir": value })
    );
    assert_eq!(fs::read_to_string(fixture.config_path()).unwrap(), document);
    assert!(!path.exists());
}

#[test]
fn show_reflects_both_set_commands() {
    let fixture = Fixture::new();
    for (setting, key, directory) in [
        ("workspaces-dir", "workspaces_dir", "workspaces"),
        ("origins-dir", "origins_dir", "origins"),
    ] {
        let expected = fixture.config_path().parent().unwrap().join(directory);
        let output = fixture
            .command()
            .args(["config", "set", setting, directory])
            .output()
            .unwrap();
        assert_eq!(success(output), format!("{key}={}\n", expected.display()));
        let value: serde_json::Value = serde_json::from_str(&success(fixture.show(true))).unwrap();
        assert_eq!(value[key], expected.to_string_lossy().as_ref());
        assert!(!expected.exists());
    }
}

#[test]
fn path_locates_missing_and_invalid_configuration_without_reading_it() {
    let fixture = Fixture::new();
    let check = || {
        let output = fixture.command().args(["config", "path"]).output().unwrap();
        assert_eq!(
            success(output),
            format!("{}\n", fixture.config_path().display())
        );
    };
    check();
    assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 0);
    fixture.write_config("[");
    check();
    assert_eq!(fs::read_to_string(fixture.config_path()).unwrap(), "[");
    fs::remove_file(fixture.config_path()).unwrap();
    fs::create_dir(fixture.config_path()).unwrap();
    check();
    assert_eq!(fs::read_dir(fixture.config_path()).unwrap().count(), 0);
    assert_eq!(
        fs::read_dir(fixture.config_path().parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn path_rejects_arguments_and_json() {
    let fixture = Fixture::new();
    for argument in ["--json", "workspaces-dir"] {
        let output = fixture
            .command()
            .args(["config", "path", argument])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let help = success(
        fixture
            .command()
            .args(["config", "--help"])
            .output()
            .unwrap(),
    );
    assert!(help.contains("show"));
    assert!(help.contains("path"));
}

#[test]
fn configuration_commands_report_path_resolution_failure() {
    let fixture = Fixture::new();
    for args in [
        vec!["config", "path"],
        vec!["config", "show"],
        vec!["config", "show", "--json"],
    ] {
        let mut command = fixture.command();
        for name in [
            "HOME",
            "USERPROFILE",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "APPDATA",
            "LOCALAPPDATA",
        ] {
            command.env_remove(name);
        }
        let output = command.args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("home directory is unavailable"));
    }
    assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn path_preserves_spaces_and_does_not_follow_configuration_symlinks() {
    let fixture = Fixture::new();
    let base = fixture.root.join("home with spaces");
    let mut command = fixture.command();
    command.env("HOME", &base).env("XDG_CONFIG_HOME", &base);
    #[cfg(target_os = "macos")]
    let config = base.join("Library/Application Support/trees/config.toml");
    #[cfg(target_os = "linux")]
    let config = base.join("trees/config.toml");
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let config = base.join(".config/trees/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(fixture.root.join("absent.toml"), &config).unwrap();
    let output = command.args(["config", "path"]).output().unwrap();
    assert_eq!(success(output), format!("{}\n", config.display()));
    assert!(fs::symlink_metadata(config)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!fixture.root.join("absent.toml").exists());
}
