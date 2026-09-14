use std::ffi::OsStr;
use std::io;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

use snafu::{ResultExt, Snafu};

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

pub fn run_program(identity: &SessionIdentity, program: &OsStr) -> ProgramOutcome {
    let mut command = Command::new(program);
    command.current_dir(identity.workspace_path.as_path());
    ProgramOutcome(run_child(&mut command, identity, program))
}

fn run_child(
    command: &mut Command,
    identity: &SessionIdentity,
    program: &OsStr,
) -> Result<ExitStatus, ProcessError> {
    let mut child = command.spawn().context(StartSnafu {
        program: PathBuf::from(program),
        workspace_path: identity.workspace_path.as_path(),
    })?;
    wait_for_exit(|| child.wait()).context(WaitSnafu { pid: child.id() })
}

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
        assert_eq!(
            run_program(&identity, OsStr::new("/usr/bin/true")).exit_code(true),
            0
        );
        let missing = run_program(&identity, OsStr::new("/nonexistent/trees-program"));
        assert!(matches!(missing.0, Err(ProcessError::Start { .. })));
        assert!(missing.can_release());
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 7"]);
        assert_eq!(
            run_child(&mut command, &identity, OsStr::new("/bin/sh"))
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
