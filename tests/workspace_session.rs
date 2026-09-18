#![cfg(unix)]

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("trees-session-{}", uuid::Uuid::now_v7()));
        let repo = root.join("source");
        fs::create_dir_all(&repo).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "trees@example.invalid"],
            vec!["config", "user.name", "Trees Tests"],
        ] {
            git(&repo, &args);
        }
        fs::write(repo.join("README"), "initial\n").unwrap();
        git(&repo, &["add", "README"]);
        git(&repo, &["commit", "-qm", "initial"]);
        Self { root, repo }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_trees"));
        command
            .env("HOME", self.root.join("home"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("SESSION_TEST_BINARY", std::env::current_exe().unwrap());
        command
    }

    fn database_path(&self) -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            self.root
                .join("home/Library/Application Support/trees/db.sqlite")
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.root.join("state/trees/db.sqlite")
        }
    }

    fn connection(&self) -> diesel::sqlite::SqliteConnection {
        trees::database::connect(&self.database_path()).unwrap()
    }

    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn create(&self, program: &Path) -> Command {
        let mut command = self.command();
        command
            .args(["create", "--offline", "--repo"])
            .arg(&self.repo)
            .arg("--release-on-exit")
            .arg(format!("--open={}", program.display()));
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

struct Terminal {
    child: Child,
    master: Option<File>,
    output: String,
}

impl Terminal {
    fn spawn(mut command: Command) -> Self {
        let (mut master, mut slave) = (-1, -1);
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        let mut attributes = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::tcgetattr(slave.as_raw_fd(), &mut attributes) },
            0
        );
        attributes.c_lflag &= !libc::ECHO;
        assert_eq!(
            unsafe { libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &attributes) },
            0
        );
        // The test driver must not keep a slave descriptor alive after spawn.
        command
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave));
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().unwrap();
        Self {
            child,
            master: Some(master),
            output: String::new(),
        }
    }

    fn read(&mut self, deadline: Instant) -> bool {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "terminal timed out: {}", self.output);
        let master = self.master.as_mut().unwrap();
        let mut descriptor = libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe {
            libc::poll(
                &mut descriptor,
                1,
                remaining.as_millis().min(i32::MAX as u128) as i32,
            )
        };
        if ready < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            return true;
        }
        assert!(ready > 0, "terminal timed out: {}", self.output);
        let mut bytes = [0; 4096];
        match master.read(&mut bytes) {
            Ok(0) => false,
            Ok(count) => {
                self.output
                    .push_str(&String::from_utf8_lossy(&bytes[..count]));
                true
            }
            Err(error) if error.raw_os_error() == Some(libc::EIO) => false,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => true,
            Err(error) => panic!("terminal read failed: {error}"),
        }
    }

    fn until(&mut self, marker: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.output.contains(marker) {
            assert!(self.read(deadline), "missing {marker}: {}", self.output);
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.master.as_mut().unwrap().write_all(bytes).unwrap();
    }

    fn finish(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.read(deadline) {}
        self.child.wait().unwrap()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.master.take();
        // Kill the isolated test group and reap even when an assertion fails.
        if self.child.try_wait().ok().flatten().is_none() {
            unsafe { libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL) };
            let _ = self.child.wait();
        }
    }
}

#[test]
fn process_helper() {
    if std::env::var_os("SESSION_PROCESS_HELPER").is_none() {
        return;
    }
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) };
    for signal in [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM] {
        unsafe { libc::signal(signal, libc::SIG_DFL) };
    }
    println!("SESSION_CHILD_READY");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    println!("SESSION_CHILD_FINISHED");
}

#[test]
fn terminal_interrupts_reach_the_child_and_supervisor_termination_is_forwarded() {
    for signal in [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM] {
        let fixture = Fixture::new();
        let program = fixture.script(
            "program",
            "exec \"$SESSION_TEST_BINARY\" --exact process_helper --nocapture",
        );
        let mut command = fixture.create(&program);
        command.env("SESSION_PROCESS_HELPER", "1");
        let mut terminal = Terminal::spawn(command);
        terminal.until("SESSION_CHILD_READY");
        match signal {
            libc::SIGINT => terminal.send(&[3]),
            libc::SIGQUIT => terminal.send(&[28]),
            _ => {
                assert_eq!(
                    unsafe { libc::kill(terminal.child.id() as libc::pid_t, signal) },
                    0
                );
            }
        }
        assert_eq!(
            terminal.finish().code(),
            Some(128 + signal),
            "{}",
            terminal.output
        );
    }
}

#[test]
fn terminal_job_can_be_suspended_and_resumed() {
    let fixture = Fixture::new();
    let program = fixture.script(
        "program",
        "exec \"$SESSION_TEST_BINARY\" --exact process_helper --nocapture",
    );
    let command = fixture.create(&program);
    let mut shell = Command::new("/bin/sh");
    shell
        .arg("-i")
        .env("PS1", "OUTER_READY> ")
        .env("SESSION_PROCESS_HELPER", "1");
    for (name, value) in command.get_envs() {
        if let Some(value) = value {
            shell.env(name, value);
        }
    }
    let mut terminal = Terminal::spawn(shell);
    terminal.until("OUTER_READY>");
    let quote =
        |value: &std::ffi::OsStr| format!("'{}'", value.to_string_lossy().replace('\'', "'\"'\"'"));
    let mut line = quote(command.get_program());
    for arg in command.get_args() {
        line.push(' ');
        line.push_str(&quote(arg));
    }
    line.push('\n');
    terminal.send(line.as_bytes());
    terminal.until("SESSION_CHILD_READY");
    terminal.output.clear();
    terminal.send(&[26]);
    terminal.until("OUTER_READY>");
    terminal.send(b"printf 'SESSION_SUSPENDED_OK\\n'\n");
    terminal.until("SESSION_SUSPENDED_OK");
    terminal.send(b"fg\n\n");
    terminal.until("SESSION_CHILD_FINISHED");
    terminal.send(b"exit 0\n");
    assert!(terminal.finish().success(), "{}", terminal.output);
}

#[test]
fn releases_clean_workspaces_and_retains_dirty_claims() {
    for (body, released) in [
        ("exit 0", true),
        ("exit 7", true),
        ("printf dirty > unsaved", false),
    ] {
        let fixture = Fixture::new();
        let program = fixture.script("program", body);
        let output = fixture.create(&program).output().unwrap();
        let mut connection = fixture.connection();
        let workspaces = trees::storage::list_workspaces(&mut connection, false).unwrap();
        assert_eq!(workspaces.len(), 1);
        let workspace = &workspaces[0];
        let claim = trees::storage::find_workspace_claim(&mut connection, &workspace.id).unwrap();
        assert_eq!(
            claim.is_none(),
            released,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if !released {
            assert!(workspace.canonical_path.as_path().join("unsaved").exists());
        }
    }
}

#[test]
fn ends_cleanup_after_manual_release_without_adopting_a_replacement_claim() {
    let fixture = Fixture::new();
    let program = fixture.script("program", "\"$TREES_BINARY\" release && \"$TREES_BINARY\" create --repo \"$SESSION_SOURCE\" --offline --json");
    let output = fixture
        .create(&program)
        .env("TREES_BINARY", env!("CARGO_BIN_EXE_trees"))
        .env("SESSION_SOURCE", &fixture.repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut connection = fixture.connection();
    let workspaces = trees::storage::list_workspaces(&mut connection, false).unwrap();
    assert_eq!(workspaces.len(), 1);
    assert!(
        trees::storage::find_workspace_claim(&mut connection, &workspaces[0].id)
            .unwrap()
            .is_some()
    );
}

#[test]
fn missing_database_does_not_recreate_storage_or_imply_released_ownership() {
    let fixture = Fixture::new();
    let program = fixture.script("program", "mv \"$SESSION_DB\" \"$SESSION_DB.saved\"");
    let output = fixture
        .create(&program)
        .env("SESSION_DB", fixture.database_path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!fixture.database_path().exists());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Manual recovery: trees release --claim-id"));
    assert!(!stderr.contains("Opening $SHELL"));
    let mut connection =
        trees::database::connect(&fixture.database_path().with_extension("sqlite.saved")).unwrap();
    let workspace = trees::storage::list_workspaces(&mut connection, false)
        .unwrap()
        .remove(0);
    assert!(
        trees::storage::find_workspace_claim(&mut connection, &workspace.id)
            .unwrap()
            .is_some()
    );
}

#[test]
fn recovery_shell_retries_after_each_exit_and_allows_manual_release() {
    for manual_release in [false, true] {
        let fixture = Fixture::new();
        let program = fixture.script("program", "printf dirty > unsaved; exit 7");
        let mut command = fixture.create(&program);
        command
            .env("SHELL", "/bin/sh")
            .env("PS1", "RECOVERY_READY> ")
            .env("LC_ALL", "C")
            .env("TREES_BINARY", env!("CARGO_BIN_EXE_trees"));
        let mut terminal = Terminal::spawn(command);
        terminal.until("RECOVERY_READY>");
        assert!(terminal
            .output
            .contains("[trees:release-on-exit] RECOVERY_READY>"));
        assert!(terminal
            .output
            .contains("Exiting the shell will retry release"));
        terminal.output.clear();
        terminal.send(b"exit 9\n");
        terminal.until("RECOVERY_READY>");
        assert!(terminal.output.contains("Release failed"));
        assert!(terminal
            .output
            .contains("[trees:release-on-exit] RECOVERY_READY>"));
        if manual_release {
            terminal.send(b"rm unsaved; \"$TREES_BINARY\" release; exit 0\n");
        } else {
            terminal.send(b"rm unsaved; exit 0\n");
        }
        assert_eq!(terminal.finish().code(), Some(7), "{}", terminal.output);
        let mut connection = fixture.connection();
        let workspace = trees::storage::list_workspaces(&mut connection, false)
            .unwrap()
            .remove(0);
        assert!(
            trees::storage::find_workspace_claim(&mut connection, &workspace.id)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn unavailable_recovery_shells_report_manual_intervention() {
    for shell in [None, Some(""), Some("/nonexistent/trees-shell")] {
        let fixture = Fixture::new();
        let program = fixture.script("program", "printf dirty > unsaved");
        let mut command = fixture.create(&program);
        match shell {
            Some(shell) => {
                command.env("SHELL", shell);
            }
            None => {
                command.env_remove("SHELL");
            }
        }
        let mut terminal = Terminal::spawn(command);
        assert_eq!(terminal.finish().code(), Some(1));
        let mut connection = fixture.connection();
        let workspace = trees::storage::list_workspaces(&mut connection, false)
            .unwrap()
            .remove(0);
        let claim = trees::storage::find_workspace_claim(&mut connection, &workspace.id)
            .unwrap()
            .unwrap();
        assert!(terminal
            .output
            .contains(&workspace.canonical_path.to_string()));
        assert!(terminal
            .output
            .contains(&format!("trees release --claim-id {}", claim.id)));
        assert!(workspace.canonical_path.as_path().join("unsaved").exists());
    }
}

#[test]
fn missing_workspace_directory_reports_recovery_start_failure() {
    let fixture = Fixture::new();
    let program = fixture.script("program", "workspace=\"$PWD\"; cd /; rm -rf \"$workspace\"");
    let mut command = fixture.create(&program);
    command.env("SHELL", "/bin/sh");
    let mut terminal = Terminal::spawn(command);
    assert_eq!(terminal.finish().code(), Some(1));
    assert!(
        terminal.output.contains("recovery shell failed"),
        "{}",
        terminal.output
    );
    assert!(terminal
        .output
        .contains("Manual recovery: trees release --claim-id"));
}

#[test]
fn noninteractive_failure_never_starts_a_shell_but_clean_release_needs_no_shell() {
    let fixture = Fixture::new();
    let shell = fixture.script("shell", "touch \"$SHELL_STARTED\"");
    let started = fixture.root.join("shell-started");
    let program = fixture.script("program", "printf dirty > unsaved");
    let output = fixture
        .create(&program)
        .env("SHELL", shell)
        .env("SHELL_STARTED", &started)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("interactive recovery requires terminal input and output"));
    assert!(stderr.contains("Manual recovery: trees release --claim-id"));
    assert!(!started.exists());
    let output = fixture
        .create(Path::new("true"))
        .env_remove("SHELL")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn preserves_program_exit_codes_after_successful_and_failed_cleanup() {
    for code in [0, 7, 255] {
        for dirty in [false, true] {
            let fixture = Fixture::new();
            let body = format!(
                "{}exit {code}",
                if dirty {
                    "printf dirty > unsaved; "
                } else {
                    ""
                }
            );
            let program = fixture.script("program", &body);
            let output = fixture.create(&program).output().unwrap();
            let expected = if dirty && code == 0 { 1 } else { code };
            assert_eq!(
                output.status.code(),
                Some(expected),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stdout.is_empty());
        }
    }
}

#[test]
fn initial_launch_failure_stays_failed_after_successful_release() {
    let fixture = Fixture::new();
    let output = fixture
        .create(&fixture.root.join("missing-program"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to start"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Manual recovery"));
    let mut connection = fixture.connection();
    let workspace = trees::storage::list_workspaces(&mut connection, false)
        .unwrap()
        .remove(0);
    assert!(
        trees::storage::find_workspace_claim(&mut connection, &workspace.id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn recovery_launch_failure_preserves_the_initial_nonzero_exit() {
    let fixture = Fixture::new();
    let program = fixture.script("program", "printf dirty > unsaved; exit 7");
    let mut command = fixture.create(&program);
    command.env("SHELL", "/nonexistent/trees-shell");
    let mut terminal = Terminal::spawn(command);
    assert_eq!(terminal.finish().code(), Some(7), "{}", terminal.output);
    assert!(terminal.output.contains("recovery shell failed"));
    assert!(terminal.output.contains("Manual recovery"));
}

#[test]
fn clean_sessions_reuse_the_same_workspace() {
    let fixture = Fixture::new();
    let mut paths = Vec::new();
    for _ in 0..2 {
        let output = fixture.create(Path::new("pwd")).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        paths.push(output.stdout);
    }
    assert_eq!(paths[0], paths[1]);
    let mut connection = fixture.connection();
    assert_eq!(
        trees::storage::list_workspaces(&mut connection, false)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn running_children_keep_their_claim_without_holding_a_database_transaction() {
    let fixture = Fixture::new();
    let program = fixture.script(
        "program",
        "exec \"$SESSION_TEST_BINARY\" --exact process_helper --nocapture",
    );
    let mut command = fixture.create(&program);
    command.env("SESSION_PROCESS_HELPER", "1");
    let mut terminal = Terminal::spawn(command);
    terminal.until("SESSION_CHILD_READY");
    let mut connection = fixture.connection();
    let workspace = trees::storage::list_workspaces(&mut connection, false)
        .unwrap()
        .remove(0);
    connection
        .immediate_transaction::<_, diesel::result::Error, _>(|connection| {
            assert!(trees::storage::find_workspace_claim(connection, &workspace.id)?.is_some());
            Ok(())
        })
        .unwrap();
    drop(connection);
    terminal.send(b"\n");
    assert!(terminal.finish().success(), "{}", terminal.output);
    let mut connection = fixture.connection();
    assert!(
        trees::storage::find_workspace_claim(&mut connection, &workspace.id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn multi_repository_recovery_preserves_dirty_work_and_records_release_attempts() {
    use diesel::prelude::*;
    use trees::schema::lifecycle_events::dsl as events;
    let fixture = Fixture::new();
    let other = fixture.root.join("other");
    let cloned = Command::new("git")
        .args(["clone", "-q"])
        .arg(&fixture.repo)
        .arg(&other)
        .output()
        .unwrap();
    assert!(cloned.status.success());
    let program = fixture.script(
        "program",
        "printf dirty > source/unsaved; git -C other checkout -qb saved-branch",
    );
    let mut command = fixture.create(&program);
    command
        .arg("--repo")
        .arg(&other)
        .env("SHELL", "/bin/sh")
        .env("PS1", "RECOVERY_READY> ");
    let mut terminal = Terminal::spawn(command);
    terminal.until("RECOVERY_READY>");
    let mut connection = fixture.connection();
    let workspace = trees::storage::list_workspaces(&mut connection, false)
        .unwrap()
        .remove(0);
    let second = workspace.canonical_path.as_path().join("other");
    let branch = Command::new("git")
        .arg("-C")
        .arg(&second)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&branch.stdout).trim(),
        "saved-branch"
    );
    assert!(workspace
        .canonical_path
        .as_path()
        .join("source/unsaved")
        .exists());
    let rejected = events::lifecycle_events
        .filter(events::entity_id.eq(workspace.id.to_string()))
        .filter(events::event_type.eq("workspace_release_rejected"))
        .count()
        .get_result::<i64>(&mut connection)
        .unwrap();
    assert_eq!(rejected, 1);
    drop(connection);
    terminal.send(b"rm source/unsaved; exit 0\n");
    assert!(terminal.finish().success(), "{}", terminal.output);
    let mut connection = fixture.connection();
    let released = events::lifecycle_events
        .filter(events::entity_id.eq(workspace.id.to_string()))
        .filter(events::event_type.eq("workspace_released"))
        .count()
        .get_result::<i64>(&mut connection)
        .unwrap();
    assert_eq!(released, 1);
    assert!(
        trees::storage::find_workspace_claim(&mut connection, &workspace.id)
            .unwrap()
            .is_none()
    );
    let branch = Command::new("git")
        .arg("-C")
        .arg(&second)
        .args(["symbolic-ref", "--quiet", "HEAD"])
        .output()
        .unwrap();
    assert!(!branch.status.success());
}

#[test]
fn recovery_shell_supports_job_control() {
    let fixture = Fixture::new();
    let program = fixture.script("program", "printf dirty > unsaved; exit 7");
    let mut command = fixture.create(&program);
    command
        .env("SHELL", "/bin/sh")
        .env("PS1", "RECOVERY_READY> ")
        .env("SESSION_PROCESS_HELPER", "1");
    let mut terminal = Terminal::spawn(command);
    terminal.until("RECOVERY_READY>");
    terminal.send(b"\"$SESSION_TEST_BINARY\" --exact process_helper --nocapture\n");
    terminal.until("SESSION_CHILD_READY");
    terminal.output.clear();
    terminal.send(&[26]);
    terminal.until("RECOVERY_READY>");
    terminal.send(b"fg\n\n");
    terminal.until("SESSION_CHILD_FINISHED");
    terminal.send(b"rm unsaved; exit 0\n");
    assert_eq!(terminal.finish().code(), Some(7), "{}", terminal.output);
}

#[test]
fn initial_shell_displays_release_on_exit_prompt() {
    for (lc_all, lc_ctype, lang, inherited, marker) in [
        (
            "C",
            "en_US.UTF-8",
            "en_US.UTF-8",
            None,
            "trees:release-on-exit",
        ),
        ("en_US.UTF-8", "C", "C", Some("CUSTOM> "), "♻️"),
        ("", "en_US.utf8", "C", None, "♻️"),
        ("", "", "en_US.UTF-8", Some("CUSTOM> "), "♻️"),
        ("", "", "", Some("CUSTOM> "), "trees:release-on-exit"),
    ] {
        let fixture = Fixture::new();
        let mut command = fixture.create(Path::new("/bin/sh"));
        command
            .env_remove("ENV")
            .env("LC_ALL", lc_all)
            .env("LC_CTYPE", lc_ctype)
            .env("LANG", lang);
        match inherited {
            Some(prompt) => {
                command.env("PS1", prompt);
            }
            None => {
                command.env_remove("PS1");
            }
        }
        let mut terminal = Terminal::spawn(command);
        terminal.until(&format!("[{marker}] {}", inherited.unwrap_or("$ ")));
        terminal.send(b"exit 0\n");
        assert_eq!(terminal.finish().code(), Some(0), "{}", terminal.output);
    }
}

#[test]
fn default_shell_supports_supervision_and_unflagged_create_retains_its_claim() {
    for release in [false, true] {
        let fixture = Fixture::new();
        let mut command = fixture.command();
        command
            .args(["create", "--offline", "--repo"])
            .arg(&fixture.repo)
            .arg("--open")
            .env("SHELL", "true");
        if release {
            command.arg("--release-on-exit");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut connection = fixture.connection();
        let workspace = trees::storage::list_workspaces(&mut connection, false)
            .unwrap()
            .remove(0);
        assert_eq!(
            trees::storage::find_workspace_claim(&mut connection, &workspace.id)
                .unwrap()
                .is_none(),
            release
        );
    }
}

#[test]
fn invalid_session_arguments_fail_before_storage_or_workspace_mutation() {
    let fixture = Fixture::new();
    for extra in [
        vec![],
        vec!["--open", "--json"],
        vec!["--open", "manual-workspace"],
    ] {
        let output = fixture
            .command()
            .current_dir(&fixture.root)
            .args(["create", "--repo"])
            .arg(&fixture.repo)
            .arg("--release-on-exit")
            .args(extra)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!fixture.database_path().exists());
        assert!(!fixture.root.join("manual-workspace").exists());
    }
}

#[test]
fn inherited_blocked_child_notifications_do_not_prevent_release() {
    let fixture = Fixture::new();
    let program = fixture.script(
        "program",
        "exec \"$SESSION_TEST_BINARY\" --exact process_helper --nocapture",
    );
    let mut command = fixture.create(&program);
    command.env("SESSION_PROCESS_HELPER", "1");
    unsafe {
        command.pre_exec(|| {
            let mut signals = std::mem::zeroed();
            libc::sigemptyset(&mut signals);
            libc::sigaddset(&mut signals, libc::SIGCHLD);
            let result = libc::pthread_sigmask(libc::SIG_BLOCK, &signals, std::ptr::null_mut());
            if result != 0 {
                return Err(std::io::Error::from_raw_os_error(result));
            }
            Ok(())
        });
    }
    let mut terminal = Terminal::spawn(command);
    terminal.until("SESSION_CHILD_READY");
    terminal.send(b"\n");
    assert!(terminal.finish().success(), "{}", terminal.output);
    let mut connection = fixture.connection();
    let workspace = trees::storage::list_workspaces(&mut connection, false)
        .unwrap()
        .remove(0);
    assert!(
        trees::storage::find_workspace_claim(&mut connection, &workspace.id)
            .unwrap()
            .is_none()
    );
}
