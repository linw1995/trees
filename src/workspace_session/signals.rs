use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::{Child, ExitStatus};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Mutex, MutexGuard};

static SESSION: Mutex<()> = Mutex::new(());
static NOTIFY_FD: AtomicI32 = AtomicI32::new(-1);
static TERMINATE: AtomicBool = AtomicBool::new(false);

extern "C" fn notify(signal: libc::c_int) {
    #[cfg(target_os = "linux")]
    let errno = unsafe { libc::__errno_location() };
    #[cfg(target_os = "macos")]
    let errno = unsafe { libc::__error() };
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let saved_errno = unsafe { *errno };
    if signal == libc::SIGTERM {
        TERMINATE.store(true, Ordering::SeqCst);
    }
    let fd = NOTIFY_FD.load(Ordering::SeqCst);
    if fd >= 0 {
        // The nonblocking pipe only wakes normal control flow; no lifecycle work
        // or allocation is permitted in the signal handler.
        unsafe { libc::write(fd, (&signal as *const libc::c_int).cast(), 1) };
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    unsafe {
        *errno = saved_errno
    };
}

pub(super) struct Supervisor {
    previous: Vec<(libc::c_int, libc::sigaction)>,
    previous_mask: Option<libc::sigset_t>,
    read: OwnedFd,
    _write: OwnedFd,
    _session: MutexGuard<'static, ()>,
}

impl Supervisor {
    pub(super) fn new() -> io::Result<Self> {
        let session = SESSION.lock().unwrap_or_else(|error| error.into_inner());
        let mut fds = [-1; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // Both descriptors were freshly created and have exactly one owner.
        let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        for fd in [&read, &write] {
            if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0
                || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let mut supervisor = Self {
            previous: Vec::new(),
            previous_mask: None,
            read,
            _write: write,
            _session: session,
        };
        TERMINATE.store(false, Ordering::SeqCst);
        NOTIFY_FD.store(supervisor._write.as_raw_fd(), Ordering::SeqCst);
        for signal in [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGCHLD] {
            let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
            action.sa_sigaction = notify as *const () as usize;
            unsafe { libc::sigemptyset(&mut action.sa_mask) };
            let mut previous = unsafe { std::mem::zeroed() };
            if unsafe { libc::sigaction(signal, &action, &mut previous) } < 0 {
                return Err(io::Error::last_os_error());
            }
            supervisor.previous.push((signal, previous));
        }
        let mut managed = unsafe { std::mem::zeroed() };
        unsafe { libc::sigemptyset(&mut managed) };
        for (signal, _) in &supervisor.previous {
            unsafe { libc::sigaddset(&mut managed, *signal) };
        }
        let mut previous_mask = unsafe { std::mem::zeroed() };
        // Signal dispositions alone cannot wake the supervisor when the caller
        // blocked notifications. Restore the caller's mask when the session ends.
        let result =
            unsafe { libc::pthread_sigmask(libc::SIG_UNBLOCK, &managed, &mut previous_mask) };
        if result != 0 {
            return Err(io::Error::from_raw_os_error(result));
        }
        supervisor.previous_mask = Some(previous_mask);
        Ok(supervisor)
    }

    pub(super) fn wait(&self, child: &mut Child) -> io::Result<ExitStatus> {
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
            if TERMINATE.swap(false, Ordering::SeqCst) {
                // An unreaped child keeps its PID reserved even if it exits here.
                if unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) } < 0 {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(error);
                    }
                }
            }
            let mut descriptor = libc::pollfd {
                fd: self.read.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut descriptor, 1, -1) } < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            let mut buffer = [0_u8; 256];
            // Drain notifications before checking child state again. A full pipe
            // still wakes poll, and TERMINATE preserves a coalesced request.
            while unsafe {
                libc::read(
                    self.read.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            } > 0
            {}
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        for (signal, action) in self.previous.iter().rev() {
            unsafe { libc::sigaction(*signal, action, std::ptr::null_mut()) };
        }
        NOTIFY_FD.store(-1, Ordering::SeqCst);
        if let Some(mask) = &self.previous_mask {
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, mask, std::ptr::null_mut()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_the_callers_signal_mask_after_supervision() {
        unsafe {
            let mut before = std::mem::zeroed();
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_SETMASK, std::ptr::null(), &mut before),
                0
            );
            let mut blocked = before;
            libc::sigaddset(&mut blocked, libc::SIGCHLD);
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_SETMASK, &blocked, std::ptr::null_mut()),
                0
            );
            let supervisor = Supervisor::new().unwrap();
            let mut during = std::mem::zeroed();
            libc::pthread_sigmask(libc::SIG_SETMASK, std::ptr::null(), &mut during);
            drop(supervisor);
            let mut after = std::mem::zeroed();
            libc::pthread_sigmask(libc::SIG_SETMASK, std::ptr::null(), &mut after);
            libc::pthread_sigmask(libc::SIG_SETMASK, &before, std::ptr::null_mut());
            assert_eq!(libc::sigismember(&during, libc::SIGCHLD), 0);
            assert_eq!(libc::sigismember(&after, libc::SIGCHLD), 1);
        }
    }
}
