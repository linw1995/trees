use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use snafu::{ResultExt, Snafu};

const STDERR_TAIL_BYTES: usize = 8 * 1024;

type LineResult = Result<String, io::Error>;

pub struct AppServerProcess {
    child: Child,
    session: JsonRpcSession<ChildStdin>,
    stderr: StderrTail,
}

pub trait RpcClient {
    fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, AppServerError>;

    fn notify(&mut self, method: &str, params: Value) -> Result<(), AppServerError>;
}

impl AppServerProcess {
    pub fn spawn(executable: &Path) -> Result<Self, AppServerError> {
        let mut command = Command::new(executable);
        command.arg("app-server").arg("--stdio");
        Self::spawn_command(command, executable.to_owned())
    }

    fn spawn_command(mut command: Command, executable: PathBuf) -> Result<Self, AppServerError> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context(SpawnSnafu { executable })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            let _ = child.kill();
            AppServerError::Transport {
                message: "app-server standard input was not piped".to_owned(),
            }
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            let _ = child.kill();
            AppServerError::Transport {
                message: "app-server standard output was not piped".to_owned(),
            }
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            let _ = child.kill();
            AppServerError::Transport {
                message: "app-server standard error was not piped".to_owned(),
            }
        })?;

        let (line_sender, line_receiver) = mpsc::channel();
        spawn_stdout_reader(stdout, line_sender);
        let stderr_tail = StderrTail::new();
        spawn_stderr_reader(stderr, stderr_tail.clone());

        Ok(Self {
            child,
            session: JsonRpcSession::new(stdin, line_receiver),
            stderr: stderr_tail,
        })
    }

    pub fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, AppServerError> {
        self.session.request(method, params, timeout)
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<(), AppServerError> {
        self.session.notify(method, params)
    }

    pub fn stderr_tail(&self) -> String {
        self.stderr.text()
    }

    pub fn shutdown(mut self, timeout: Duration) -> Result<ExitStatus, AppServerError> {
        self.session.close_writer();
        let deadline = Instant::now() + timeout;

        loop {
            if let Some(status) = self.child.try_wait().context(IoSnafu)? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return Err(AppServerError::ShutdownTimeout {
                    stderr: self.stderr_tail(),
                });
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl RpcClient for AppServerProcess {
    fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, AppServerError> {
        Self::request(self, method, params, timeout)
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), AppServerError> {
        Self::notify(self, method, params)
    }
}

pub struct JsonRpcSession<W> {
    writer: Option<W>,
    lines: Receiver<LineResult>,
    next_id: u64,
}

impl<W: Write> JsonRpcSession<W> {
    fn new(writer: W, lines: Receiver<LineResult>) -> Self {
        Self {
            writer: Some(writer),
            lines,
            next_id: 1,
        }
    }

    pub fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, AppServerError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.write_message(json!({"id": id, "method": method, "params": params}))?;

        let deadline = Instant::now() + timeout;
        loop {
            let line = self.receive_line(method, deadline)?;
            let message: Value =
                serde_json::from_str(&line).context(MalformedMessageSnafu { method, line })?;

            if message.get("id").and_then(Value::as_u64) != Some(id) {
                if message.get("method").is_some() && message.get("id").is_some() {
                    return Err(AppServerError::UnexpectedServerRequest {
                        method: message
                            .get("method")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_owned(),
                    });
                }
                continue;
            }

            if let Some(error) = message.get("error") {
                return Err(AppServerError::Remote {
                    method: method.to_owned(),
                    error: error.to_string(),
                });
            }

            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<(), AppServerError> {
        self.write_message(json!({"method": method, "params": params}))
    }

    fn write_message(&mut self, message: Value) -> Result<(), AppServerError> {
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| AppServerError::Transport {
                message: "app-server input is closed".to_owned(),
            })?;
        serde_json::to_writer(&mut *writer, &message).context(JsonSnafu)?;
        writer.write_all(b"\n").context(IoSnafu)?;
        writer.flush().context(IoSnafu)
    }

    fn receive_line(&self, method: &str, deadline: Instant) -> Result<String, AppServerError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(AppServerError::Timeout {
                method: method.to_owned(),
            });
        }

        match self.lines.recv_timeout(remaining) {
            Ok(Ok(line)) => Ok(line),
            Ok(Err(source)) => Err(AppServerError::Io { source }),
            Err(RecvTimeoutError::Timeout) => Err(AppServerError::Timeout {
                method: method.to_owned(),
            }),
            Err(RecvTimeoutError::Disconnected) => Err(AppServerError::Transport {
                message: "app-server output reader disconnected".to_owned(),
            }),
        }
    }

    fn close_writer(&mut self) {
        self.writer.take();
    }
}

#[derive(Clone)]
struct StderrTail {
    bytes: Arc<Mutex<VecDeque<u8>>>,
}

impl StderrTail {
    fn new() -> Self {
        Self {
            bytes: Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_BYTES))),
        }
    }

    fn append(&self, bytes: &[u8]) {
        let Ok(mut tail) = self.bytes.lock() else {
            return;
        };
        for byte in bytes {
            if tail.len() == STDERR_TAIL_BYTES {
                tail.pop_front();
            }
            tail.push_back(*byte);
        }
    }

    fn text(&self) -> String {
        let Ok(mut tail) = self.bytes.lock() else {
            return String::new();
        };
        String::from_utf8_lossy(tail.make_contiguous())
            .trim()
            .to_owned()
    }
}

fn spawn_stdout_reader(stdout: ChildStdout, sender: Sender<LineResult>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = sender.send(Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "app-server standard output closed",
                    )));
                    return;
                }
                Ok(_) => {
                    let line = line.trim_end_matches(['\r', '\n']).to_owned();
                    if sender.send(Ok(line)).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error));
                    return;
                }
            }
        }
    });
}

fn spawn_stderr_reader(stderr: impl Read + Send + 'static, tail: StderrTail) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut buffer = [0_u8; 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => return,
                Ok(size) => tail.append(&buffer[..size]),
            }
        }
    });
}

#[derive(Debug, Snafu)]
pub enum AppServerError {
    #[snafu(display("failed to start {}: {source}", executable.display()))]
    Spawn {
        executable: PathBuf,
        source: io::Error,
    },
    #[snafu(display("app-server IO failed: {source}"))]
    Io { source: io::Error },
    #[snafu(display("failed to encode app-server request: {source}"))]
    Json { source: serde_json::Error },
    #[snafu(display("app-server returned malformed JSON for {method}: {source}: {line}"))]
    MalformedMessage {
        method: String,
        line: String,
        source: serde_json::Error,
    },
    #[snafu(display("app-server rejected {method}: {error}"))]
    Remote { method: String, error: String },
    #[snafu(display("app-server request timed out: {method}"))]
    Timeout { method: String },
    #[snafu(display("app-server transport failed: {message}"))]
    Transport { message: String },
    #[snafu(display("app-server sent unsupported setup request: {method}"))]
    UnexpectedServerRequest { method: String },
    #[snafu(display(
        "app-server did not exit after standard input closed{}",
        shutdown_timeout_suffix(stderr)
    ))]
    ShutdownTimeout { stderr: String },
}

fn shutdown_timeout_suffix(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn shutdown_waits_for_a_stdio_child_to_exit() {
        let mut command = Command::new("sh");
        command.args(["-c", "cat >/dev/null"]);
        let process = AppServerProcess::spawn_command(command, PathBuf::from("sh"))
            .expect("test app-server process should spawn");

        let status = process
            .shutdown(Duration::from_secs(1))
            .expect("child should exit after standard input closes");

        assert!(status.success());
    }

    #[cfg(windows)]
    #[test]
    fn shutdown_waits_for_a_stdio_child_to_exit() {
        let mut command = Command::new("cmd");
        command.args(["/C", "more > NUL"]);
        let process = AppServerProcess::spawn_command(command, PathBuf::from("cmd"))
            .expect("test app-server process should spawn");

        let status = process
            .shutdown(Duration::from_secs(1))
            .expect("child should exit after standard input closes");

        assert!(status.success());
    }

    fn session_with_lines(lines: &[&str]) -> JsonRpcSession<Vec<u8>> {
        let (sender, receiver) = mpsc::channel();
        for line in lines {
            sender
                .send(Ok((*line).to_owned()))
                .expect("test line should be queued");
        }
        drop(sender);

        JsonRpcSession::new(Vec::new(), receiver)
    }

    #[test]
    fn matches_response_after_interleaved_notification() {
        let mut session = session_with_lines(&[
            r#"{"method":"thread/started","params":{"thread":{"id":"thr"}}}"#,
            r#"{"id":1,"result":{"codexHome":"/tmp/codex"}}"#,
        ]);

        let result = session
            .request("initialize", json!({}), Duration::from_secs(1))
            .expect("response should be matched");

        assert_eq!(result["codexHome"], "/tmp/codex");
    }

    #[test]
    fn reports_malformed_response() {
        let mut session = session_with_lines(&["not-json"]);

        let error = session
            .request("initialize", json!({}), Duration::from_secs(1))
            .expect_err("malformed response should fail");

        assert!(matches!(error, AppServerError::MalformedMessage { .. }));
    }

    #[test]
    fn reports_request_timeout() {
        let (_sender, receiver) = mpsc::channel();
        let mut session = JsonRpcSession::new(Vec::<u8>::new(), receiver);

        let error = session
            .request("initialize", json!({}), Duration::from_millis(1))
            .expect_err("missing response should time out");

        assert!(matches!(error, AppServerError::Timeout { .. }));
    }

    #[test]
    fn formats_app_server_errors_and_exposes_sources() {
        let errors = [
            AppServerError::Spawn {
                executable: PathBuf::from("codex"),
                source: io::Error::new(io::ErrorKind::NotFound, "missing"),
            },
            AppServerError::Io {
                source: io::Error::new(io::ErrorKind::BrokenPipe, "closed"),
            },
            AppServerError::Json {
                source: serde_json::from_str::<Value>("not-json")
                    .expect_err("JSON should be invalid"),
            },
            AppServerError::MalformedMessage {
                method: "initialize".to_owned(),
                line: "not-json".to_owned(),
                source: serde_json::from_str::<Value>("not-json")
                    .expect_err("JSON should be invalid"),
            },
            AppServerError::Remote {
                method: "project/create".to_owned(),
                error: "method not found".to_owned(),
            },
            AppServerError::Timeout {
                method: "initialize".to_owned(),
            },
            AppServerError::Transport {
                message: "closed".to_owned(),
            },
            AppServerError::UnexpectedServerRequest {
                method: "thread/start".to_owned(),
            },
            AppServerError::ShutdownTimeout {
                stderr: "failed".to_owned(),
            },
        ];

        for error in errors {
            assert!(!error.to_string().is_empty());
            let _ = std::error::Error::source(&error);
        }
    }
}
