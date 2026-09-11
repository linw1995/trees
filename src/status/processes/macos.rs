use std::ffi::OsString;
use std::io;
use std::mem::{size_of, MaybeUninit};
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use snafu::{ensure, ResultExt};

use super::{InvalidSnafu, IssueCode, ProbeError, Process, RacedSnafu, ReadSnafu, Source};

pub(super) struct Native;

// T must be the plain C output structure corresponding to flavor.
unsafe fn pid_info<T>(pid: u32, flavor: i32) -> io::Result<T> {
    let mut value = MaybeUninit::<T>::uninit();
    let size = size_of::<T>() as i32;
    // The caller pairs the flavor with its ABI structure and provides its full size.
    let read =
        unsafe { libc::proc_pidinfo(pid as i32, flavor, 0, value.as_mut_ptr().cast(), size) };
    if read <= 0 {
        return Err(io::Error::last_os_error());
    }
    if read != size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "incomplete process metadata",
        ));
    }
    // libproc initialized the entire output structure on an exact-size success.
    Ok(unsafe { value.assume_init() })
}

fn identity(pid: u32) -> io::Result<libc::proc_bsdinfo> {
    // PROC_PIDTBSDINFO writes a proc_bsdinfo structure.
    unsafe { pid_info(pid, libc::PROC_PIDTBSDINFO) }
}

fn c_bytes(bytes: impl IntoIterator<Item = libc::c_char>) -> Vec<u8> {
    bytes
        .into_iter()
        .take_while(|&byte| byte != 0)
        .map(|byte| byte as u8)
        .collect()
}

impl Native {
    fn read_process(&self, pid: u32, uid: u32) -> Result<Option<Process>, ProbeError> {
        let before = identity(pid).context(ReadSnafu {
            code: IssueCode::UserUnreadable,
        })?;
        if before.pbi_status == libc::SZOMB || before.pbi_uid != uid {
            return Ok(None);
        }
        // PROC_PIDVNODEPATHINFO writes a proc_vnodepathinfo structure.
        let paths =
            unsafe { pid_info::<libc::proc_vnodepathinfo>(pid, libc::PROC_PIDVNODEPATHINFO) }
                .context(ReadSnafu {
                    code: IssueCode::CwdUnreadable,
                });
        let after = identity(pid).context(ReadSnafu {
            code: IssueCode::UserUnreadable,
        })?;
        if after.pbi_status == libc::SZOMB {
            return Ok(None);
        }
        ensure!(
            before.pbi_pid == after.pbi_pid
                && before.pbi_start_tvsec == after.pbi_start_tvsec
                && before.pbi_start_tvusec == after.pbi_start_tvusec
                && after.pbi_uid == uid,
            RacedSnafu
        );
        let paths = paths?;
        let bytes = c_bytes(paths.pvi_cdir.vip_path.into_iter().flatten());
        ensure!(
            !bytes.is_empty(),
            InvalidSnafu {
                code: IssueCode::CwdUnreadable
            }
        );
        let stat = paths.pvi_cdir.vip_vi.vi_stat;
        let cwd = super::physical_cwd(
            &PathBuf::from(OsString::from_vec(bytes)),
            u64::from(stat.vst_dev),
            stat.vst_ino,
        )?;
        let mut name = c_bytes(before.pbi_name);
        if name.is_empty() {
            name = c_bytes(before.pbi_comm);
        }
        Ok(Some(Process {
            pid,
            name: (!name.is_empty()).then(|| String::from_utf8_lossy(&name).into_owned()),
            cwd,
        }))
    }
}

impl Source for Native {
    fn pids(&self) -> Result<Vec<u32>, ProbeError> {
        const PROC_ALL_PIDS: u32 = 1;
        // A null buffer queries the required byte count without writing memory.
        let needed = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
        if needed <= 0 {
            return Err(io::Error::last_os_error()).context(ReadSnafu {
                code: IssueCode::EnumerationFailed,
            });
        }
        let mut capacity = needed as usize / size_of::<i32>() + 256;
        // Bounded growth handles a changing process list without waiting for quiescence.
        for _ in 0..3 {
            let mut pids = vec![0i32; capacity];
            let size = i32::try_from(std::mem::size_of_val(pids.as_slice()))
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "process list too large"))
                .context(ReadSnafu {
                    code: IssueCode::EnumerationFailed,
                })?;
            // The buffer contains capacity initialized pid_t slots and size is its byte length.
            let read =
                unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, pids.as_mut_ptr().cast(), size) };
            if read <= 0 {
                return Err(io::Error::last_os_error()).context(ReadSnafu {
                    code: IssueCode::EnumerationFailed,
                });
            }
            ensure!(
                read % size_of::<i32>() as i32 == 0 && read <= size,
                InvalidSnafu {
                    code: IssueCode::EnumerationFailed
                }
            );
            if read < size {
                pids.truncate(read as usize / size_of::<i32>());
                return Ok(pids
                    .into_iter()
                    .filter(|&pid| pid > 0)
                    .map(|pid| pid as u32)
                    .collect());
            }
            capacity *= 2;
        }
        InvalidSnafu {
            code: IssueCode::EnumerationFailed,
        }
        .fail()
    }

    fn process(&self, pid: u32, uid: u32) -> Result<Option<Process>, ProbeError> {
        match self.read_process(pid, uid) {
            Err(error) => match identity(pid) {
                Err(source) if source.raw_os_error() == Some(libc::ESRCH) => Ok(None),
                Ok(info) if info.pbi_status == libc::SZOMB => Ok(None),
                _ => Err(error),
            },
            result => result,
        }
    }
}
