use std::io::{self, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use snafu::{ResultExt, Snafu};

use super::{Issue, IssueCode, Request};
use crate::config::SessionHookConfig;

const STDOUT_LIMIT: usize = 1024 * 1024;
const STDERR_LIMIT: usize = 16 * 1024;
const TICK: Duration = Duration::from_millis(2);

pub fn execute(config: &SessionHookConfig, request: &Request) -> Result<Vec<u8>, Issue> {
    let stderr = Arc::new(Mutex::new(Vec::new()));
    run(config, request, stderr.clone()).map_err(|error| {
        let (code, exit_code) = match &error {
            RunError::Spawn { .. } => (IssueCode::SpawnFailed, None),
            RunError::Timeout => (IssueCode::TimedOut, None),
            RunError::OutputLimit => (IssueCode::OutputLimitExceeded, None),
            RunError::Exit { code } => (IssueCode::ExitFailed, *code),
            RunError::Io { .. } | RunError::Request { .. } => (IssueCode::IoFailed, None),
        };
        let bytes = stderr.lock().unwrap();
        Issue {
            code,
            exit_code,
            stderr: (!bytes.is_empty()).then(|| String::from_utf8_lossy(&bytes).into_owned()),
            workspace_id: None,
        }
    })
}

fn run(
    config: &SessionHookConfig,
    request: &Request,
    stderr: Arc<Mutex<Vec<u8>>>,
) -> Result<Vec<u8>, RunError> {
    let started = Instant::now();
    let request = serde_json::to_vec(request).context(RequestSnafu)?;
    let mut command = Command::new(&config.program);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command.spawn().context(SpawnSnafu)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut guard = Guard {
        child,
        cancelled: cancelled.clone(),
    };
    let mut input = guard.child.stdin.take().expect("piped stdin");
    let mut output = guard.child.stdout.take().expect("piped stdout");
    let mut errors = guard.child.stderr.take().expect("piped stderr");
    #[cfg(unix)]
    {
        nonblocking(&input).context(IoSnafu)?;
        nonblocking(&output).context(IoSnafu)?;
        nonblocking(&errors).context(IoSnafu)?;
    }
    let (sender, receiver) = mpsc::channel();
    let send = sender.clone();
    let cancel = cancelled.clone();
    thread::Builder::new()
        .name("session-hook-input".into())
        .spawn(move || {
            let mut position = 0;
            let result = (|| {
                while position < request.len() && !cancel.load(Ordering::Relaxed) {
                    match input.write(&request[position..]) {
                        Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                        Ok(count) => position += count,
                        Err(error) if retry(&error) => continue,
                        Err(error) => return Err(error),
                    }
                }
                Ok(())
            })();
            drop(input);
            let _ = send.send(Event::Input(result));
        })
        .context(IoSnafu)?;
    let send = sender.clone();
    let cancel = cancelled.clone();
    thread::Builder::new()
        .name("session-hook-output".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let result = read_pipe(&mut output, &cancel, |chunk| {
                if bytes.len() + chunk.len() > STDOUT_LIMIT {
                    return false;
                }
                bytes.extend_from_slice(chunk);
                true
            });
            let event = match result {
                Ok(true) => Event::Output(Ok(bytes)),
                Ok(false) => Event::Overflow,
                Err(error) => Event::Output(Err(error)),
            };
            let _ = send.send(event);
        })
        .context(IoSnafu)?;
    let cancel = cancelled.clone();
    thread::Builder::new()
        .name("session-hook-error".into())
        .spawn(move || {
            let result = read_pipe(&mut errors, &cancel, |chunk| {
                let mut bytes = stderr.lock().unwrap();
                let count = chunk.len().min(STDERR_LIMIT - bytes.len());
                bytes.extend_from_slice(&chunk[..count]);
                true
            })
            .map(|_| ());
            let _ = sender.send(Event::Error(result));
        })
        .context(IoSnafu)?;
    let mut completed = 0;
    let mut bytes = Vec::new();
    let mut io_error = None;
    let mut status = None;
    loop {
        if started.elapsed() >= config.timeout {
            return TimeoutSnafu.fail();
        }
        if status.is_none() {
            status = guard.child.try_wait().context(IoSnafu)?;
        }
        if completed == 3 {
            if let Some(status) = status {
                if !status.success() {
                    return ExitSnafu {
                        code: status.code(),
                    }
                    .fail();
                }
                if let Some(error) = io_error {
                    return Err(error);
                }
                return Ok(bytes);
            }
        }
        match receiver.recv_timeout(TICK.min(config.timeout.saturating_sub(started.elapsed()))) {
            Ok(event) => {
                completed += 1;
                let result = match event {
                    Event::Input(result) | Event::Error(result) => result,
                    Event::Output(result) => result.map(|output| bytes = output),
                    Event::Overflow => return OutputLimitSnafu.fail(),
                };
                if let Err(error) = result.context(IoSnafu) {
                    io_error = Some(error);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => thread::sleep(TICK),
        }
    }
}

fn read_pipe(
    pipe: &mut impl Read,
    cancelled: &AtomicBool,
    mut consume: impl FnMut(&[u8]) -> bool,
) -> io::Result<bool> {
    let mut buffer = [0; 8192];
    while !cancelled.load(Ordering::Relaxed) {
        match pipe.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if !consume(&buffer[..count]) {
                    return Ok(false);
                }
            }
            Err(error) if retry(&error) => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

fn retry(error: &io::Error) -> bool {
    match error.kind() {
        io::ErrorKind::Interrupted => true,
        io::ErrorKind::WouldBlock => {
            thread::sleep(TICK);
            true
        }
        _ => false,
    }
}

#[cfg(unix)]
fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // The owned pipe remains open throughout both fcntl calls.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

enum Event {
    Input(io::Result<()>),
    Output(io::Result<Vec<u8>>),
    Error(io::Result<()>),
    Overflow,
}

struct Guard {
    child: Child,
    cancelled: Arc<AtomicBool>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        #[cfg(unix)]
        {
            // A dedicated group includes descendants that still hold our pipe handles.
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Snafu)]
enum RunError {
    Spawn { source: io::Error },
    Io { source: io::Error },
    Request { source: serde_json::Error },
    Timeout,
    OutputLimit,
    Exit { code: Option<i32> },
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::status::session_hook::RequestedWorkspace;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn run_script(script: &str, timeout: Duration, large_input: bool) -> Result<Vec<u8>, Issue> {
        let root = std::env::temp_dir().join(format!("trees-hook-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        let program = root.join("session provider");
        fs::write(&program, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        let config = SessionHookConfig { program, timeout };
        let request = Request {
            version: 1,
            workspaces: if large_input {
                vec![RequestedWorkspace {
                    id: "a".into(),
                    path: "x".repeat(1024 * 1024),
                }]
            } else {
                Vec::new()
            },
        };
        let started = Instant::now();
        let result = execute(&config, &request);
        assert!(started.elapsed() < Duration::from_secs(5));
        fs::remove_dir_all(root).unwrap();
        result
    }

    #[test]
    fn invokes_executable_without_arguments_and_closes_input() {
        let result = run_script(
            "test $# -eq 0 || exit 9\ncat",
            Duration::from_secs(2),
            false,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&result).unwrap()["version"],
            1
        );
    }

    #[test]
    fn inherits_the_invoking_working_directory() {
        let result = run_script("cat >/dev/null\npwd -P", Duration::from_secs(2), false).unwrap();
        let expected = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
        assert_eq!(
            String::from_utf8(result).unwrap(),
            format!("{}\n", expected.display())
        );
    }

    #[test]
    fn captures_bounded_errors_and_discards_failed_output() {
        let error = run_script(
            "cat >/dev/null\nprintf 'partial'\nprintf 'failure' >&2\nexit 7",
            Duration::from_secs(2),
            false,
        )
        .unwrap_err();
        assert_eq!(error.code, IssueCode::ExitFailed);
        assert_eq!(error.exit_code, Some(7));
        assert_eq!(error.stderr.as_deref(), Some("failure"));
        let error = run_script(
            "cat >/dev/null\nhead -c 100000 /dev/zero >&2\nexit 3",
            Duration::from_secs(2),
            false,
        )
        .unwrap_err();
        assert_eq!(error.stderr.unwrap().len(), STDERR_LIMIT);
    }

    #[test]
    fn bounds_blocked_input_descendants_and_output() {
        for (script, large) in [
            ("sleep 10", true),
            ("cat >/dev/null\nsleep 10 &\nexit 0", false),
        ] {
            assert_eq!(
                run_script(script, Duration::from_millis(100), large)
                    .unwrap_err()
                    .code,
                IssueCode::TimedOut
            );
        }
        assert_eq!(
            run_script(
                "cat >/dev/null\nhead -c 2000000 /dev/zero",
                Duration::from_secs(2),
                false
            )
            .unwrap_err()
            .code,
            IssueCode::OutputLimitExceeded
        );
    }

    #[test]
    fn classifies_missing_and_nonexecutable_files() {
        let root = std::env::temp_dir().join(format!("trees-hook-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        let config = SessionHookConfig {
            program: root.join("missing"),
            timeout: Duration::from_secs(1),
        };
        let request = Request {
            version: 1,
            workspaces: vec![],
        };
        assert_eq!(
            execute(&config, &request).unwrap_err().code,
            IssueCode::SpawnFailed
        );
        fs::write(&config.program, "not executable").unwrap();
        assert_eq!(
            execute(&config, &request).unwrap_err().code,
            IssueCode::SpawnFailed
        );
        fs::remove_dir_all(root).unwrap();
    }
}
