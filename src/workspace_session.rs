#[cfg(unix)]
mod signals;

use std::ffi::{OsStr, OsString};
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

use diesel::{Connection, OptionalExtension};
use snafu::{ensure, OptionExt, ResultExt, Snafu};

use crate::domain::{CanonicalPath, ClaimId};
use crate::workspace::AutomaticClaimResult;

#[derive(Debug, Clone)]
pub struct SessionIdentity {
    pub workspace_path: CanonicalPath,
    pub claim_id: ClaimId,
}

impl From<AutomaticClaimResult> for SessionIdentity {
    fn from(result: AutomaticClaimResult) -> Self {
        Self {
            workspace_path: result.workspace_path,
            claim_id: result.claim_id,
        }
    }
}

#[derive(Debug)]
pub struct ProgramOutcome(pub Result<ExitStatus, ProcessError>);

impl ProgramOutcome {
    pub fn can_release(&self) -> bool {
        !matches!(self.0, Err(ProcessError::Wait { .. }))
    }

    pub fn exit_code(&self, cleanup_succeeded: bool) -> u8 {
        let code = match &self.0 {
            Ok(status) => status_code(*status),
            Err(_) => 1,
        };
        if code == 0 && !cleanup_succeeded {
            1
        } else {
            code
        }
    }
}

fn status_code(status: ExitStatus) -> u8 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return (128 + signal) as u8;
        }
    }
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .unwrap_or(1)
}

#[derive(Debug)]
pub struct SessionReport {
    pub initial: ProgramOutcome,
    pub cleanup: Result<(), SessionError>,
}

impl SessionReport {
    pub fn exit_code(&self) -> u8 {
        self.initial.exit_code(self.cleanup.is_ok())
    }
}

pub enum SessionEvent<'a> {
    InitialFailed(&'a ProcessError),
    ReleaseFailed(&'a SessionError),
    Recovering,
}

pub fn run(
    identity: &SessionIdentity,
    program: &OsStr,
    mut report: impl FnMut(SessionEvent<'_>),
) -> SessionReport {
    let supervisor = match ProcessSupervisor::new() {
        Ok(supervisor) => supervisor,
        Err(error) => {
            report(SessionEvent::InitialFailed(&error));
            return SessionReport {
                initial: ProgramOutcome(Err(error)),
                cleanup: release_original(identity),
            };
        }
    };
    let initial = ProgramOutcome(supervisor.run(identity, program, false));
    if let Err(error) = &initial.0 {
        report(SessionEvent::InitialFailed(error));
    }
    let cleanup = cleanup_after(&initial, || recover(identity, &supervisor, &mut report));
    SessionReport { initial, cleanup }
}

fn cleanup_after(
    initial: &ProgramOutcome,
    cleanup: impl FnOnce() -> Result<(), SessionError>,
) -> Result<(), SessionError> {
    ensure!(initial.can_release(), UnconfirmedChildSnafu);
    cleanup()
}

fn recover(
    identity: &SessionIdentity,
    supervisor: &ProcessSupervisor,
    report: &mut impl FnMut(SessionEvent<'_>),
) -> Result<(), SessionError> {
    loop {
        match release_original(identity) {
            Ok(()) => return Ok(()),
            Err(error) => report(SessionEvent::ReleaseFailed(&error)),
        }
        let active = {
            let mut connection = crate::database::open_read_only()?;
            original_claim_active(&mut connection, identity)?
        };
        if !active {
            return Ok(());
        }
        ensure!(
            io::stdin().is_terminal() && io::stdout().is_terminal(),
            NoninteractiveSnafu
        );
        let shell = std::env::var_os("SHELL")
            .filter(|value| !value.is_empty())
            .context(RecoveryShellSnafu)?;
        report(SessionEvent::Recovering);
        supervisor
            .run(identity, &shell, true)
            .context(RecoveryProcessSnafu)?;
    }
}

fn release_original(identity: &SessionIdentity) -> Result<(), SessionError> {
    let mut connection = crate::database::open_existing()?;
    crate::workspace::release_automatic_workspace(
        &mut connection,
        &identity.workspace_path,
        identity.claim_id,
    )
    .context(ReleaseSnafu)?;
    Ok(())
}

fn original_claim_active(
    connection: &mut diesel::sqlite::SqliteConnection,
    identity: &SessionIdentity,
) -> Result<bool, SessionError> {
    connection
        .transaction::<_, diesel::result::Error, _>(|connection| {
            let claim = crate::storage::find_workspace_claim_by_id(connection, &identity.claim_id)
                .optional()?;
            let Some(claim) = claim else {
                return Ok(false);
            };
            let workspace = crate::storage::find_workspace(connection, &claim.workspace_id)?;
            Ok(workspace.canonical_path == identity.workspace_path)
        })
        .context(OwnershipSnafu)
}

#[derive(Debug, Snafu)]
pub enum SessionError {
    #[snafu(display("interactive recovery requires terminal input and output"))]
    Noninteractive,
    #[snafu(display("$SHELL is unset or empty; cannot open a recovery shell"))]
    RecoveryShell,
    #[snafu(display("recovery shell failed: {source}"))]
    RecoveryProcess { source: ProcessError },
    #[snafu(transparent)]
    Database {
        source: crate::database::DatabaseError,
    },
    #[snafu(display("failed to release workspace: {source}"))]
    Release {
        source: crate::workspace::WorkspaceError,
    },
    #[snafu(display("failed to verify workspace ownership: {source}"))]
    Ownership { source: diesel::result::Error },
    #[snafu(display("workspace retained because child termination is unconfirmed"))]
    UnconfirmedChild,
}

struct ProcessSupervisor {
    #[cfg(unix)]
    signals: signals::Supervisor,
}

impl ProcessSupervisor {
    fn new() -> Result<Self, ProcessError> {
        Ok(Self {
            #[cfg(unix)]
            signals: signals::Supervisor::new().context(SupervisionSnafu)?,
        })
    }

    fn run(
        &self,
        identity: &SessionIdentity,
        program: &OsStr,
        interactive: bool,
    ) -> Result<ExitStatus, ProcessError> {
        let mut command = Command::new(program);
        command.current_dir(identity.workspace_path.as_path());
        command.env("TREES_RELEASE_ON_EXIT", "1");
        let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
            .into_iter()
            .filter_map(std::env::var_os)
            .find(|value| !value.is_empty());
        let utf8 = locale
            .as_deref()
            .and_then(OsStr::to_str)
            .and_then(|locale| locale.split_once('.'))
            .map(|(_, encoding)| encoding.split('@').next().unwrap_or_default())
            .is_some_and(|encoding| {
                encoding.eq_ignore_ascii_case("UTF-8") || encoding.eq_ignore_ascii_case("UTF8")
            });
        let mut prompt = OsString::from(if utf8 {
            "[♻️] "
        } else {
            "[trees:release-on-exit] "
        });
        prompt.push(std::env::var_os("PS1").unwrap_or_else(|| OsString::from("$ ")));
        command.env("PS1", prompt);
        if interactive {
            command.arg("-i");
        }
        self.run_child(&mut command, identity, program)
    }

    fn run_child(
        &self,
        command: &mut Command,
        identity: &SessionIdentity,
        program: &OsStr,
    ) -> Result<ExitStatus, ProcessError> {
        let mut child = command.spawn().context(StartSnafu {
            program: PathBuf::from(program),
            workspace_path: identity.workspace_path.as_path(),
        })?;
        #[cfg(unix)]
        let result = self.signals.wait(&mut child);
        #[cfg(not(unix))]
        let result = wait_for_exit(|| child.wait());
        result.context(WaitSnafu { pid: child.id() })
    }
}

#[cfg(any(not(unix), test))]
fn wait_for_exit(mut wait: impl FnMut() -> io::Result<ExitStatus>) -> io::Result<ExitStatus> {
    loop {
        match wait() {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

#[derive(Debug, Snafu)]
pub enum ProcessError {
    #[snafu(display("failed to install process supervision: {source}"))]
    Supervision { source: io::Error },
    #[snafu(display("failed to start {} in {}: {source}", program.display(), workspace_path.display()))]
    Start {
        program: PathBuf,
        workspace_path: PathBuf,
        source: io::Error,
    },
    #[snafu(display("cannot confirm termination of process {pid}: {source}"))]
    Wait { pid: u32, source: io::Error },
}

#[cfg(test)]
mod tests {
    use super::*;
    use snafu::IntoError;
    use std::error::Error;

    #[test]
    fn preserves_process_sources_and_release_eligibility() {
        let start = ProgramOutcome(Err(StartSnafu {
            program: "missing",
            workspace_path: "/tmp",
        }
        .into_error(io::Error::from(io::ErrorKind::NotFound))));
        assert!(start.can_release());
        assert_eq!(start.exit_code(true), 1);
        assert!(start.0.as_ref().unwrap_err().source().is_some());
        let wait = ProgramOutcome(Err(
            WaitSnafu { pid: 123_u32 }.into_error(io::Error::from(io::ErrorKind::Other))
        ));
        assert!(!wait.can_release());
        assert_eq!(wait.exit_code(true), 1);
    }

    #[test]
    fn uncertain_child_state_never_invokes_release() {
        let initial = ProgramOutcome(Err(
            WaitSnafu { pid: 123_u32 }.into_error(io::ErrorKind::Other.into())
        ));
        let result = cleanup_after(&initial, || panic!("release must not run"));
        assert!(matches!(result, Err(SessionError::UnconfirmedChild)));
    }

    #[cfg(unix)]
    #[test]
    fn waits_through_interruptions_and_preserves_wait_errors() {
        use std::os::unix::process::ExitStatusExt;
        let mut calls = 0;
        let status = wait_for_exit(|| {
            calls += 1;
            if calls == 1 {
                Err(io::ErrorKind::Interrupted.into())
            } else {
                Ok(ExitStatus::from_raw(7 << 8))
            }
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(status.code(), Some(7));
        let error = wait_for_exit(|| Err(io::ErrorKind::Other.into())).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
    }

    #[cfg(unix)]
    #[test]
    fn launches_direct_children_and_classifies_missing_programs() {
        let identity = SessionIdentity {
            workspace_path: CanonicalPath::resolve(std::env::temp_dir()).unwrap(),
            claim_id: ClaimId::new(),
        };
        let supervisor = ProcessSupervisor::new().unwrap();
        assert_eq!(
            ProgramOutcome(supervisor.run(&identity, OsStr::new("true"), false)).exit_code(true),
            0
        );
        let missing = ProgramOutcome(supervisor.run(
            &identity,
            OsStr::new("/nonexistent/trees-program"),
            false,
        ));
        assert!(matches!(missing.0, Err(ProcessError::Start { .. })));
        assert!(missing.can_release());
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 7"]);
        assert_eq!(
            supervisor
                .run_child(&mut command, &identity, OsStr::new("/bin/sh"))
                .unwrap()
                .code(),
            Some(7)
        );
    }

    #[cfg(unix)]
    #[test]
    fn preserves_initial_status_and_reports_cleanup_failure() {
        use std::os::unix::process::ExitStatusExt;
        for (raw, success, failure) in [(0, 0, 1), (7 << 8, 7, 7), (libc::SIGINT, 130, 130)] {
            let outcome = ProgramOutcome(Ok(ExitStatus::from_raw(raw)));
            assert!(outcome.can_release());
            assert_eq!(outcome.exit_code(true), success);
            assert_eq!(outcome.exit_code(false), failure);
        }
    }
}
