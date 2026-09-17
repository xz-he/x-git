//! A process tree is contained before Git's first instruction executes.
#[cfg(windows)]
mod windows {
    use std::{io, mem, ptr};
    use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
    pub struct Job(usize);
    impl Job {
        pub fn new() -> io::Result<Self> {
            let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Self(handle as usize);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const _,
                    mem::size_of_val(&limits) as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
        pub fn configure(&self, command: &mut portable_pty::CommandBuilder) {
            // self is held by the worker until after spawn and tree cleanup.
            unsafe {
                command.set_windows_job(self.0);
            }
        }
        pub fn terminate(&self) -> io::Result<()> {
            if unsafe { TerminateJobObject(self.0 as _, 1) } == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
        pub fn active(&self) -> io::Result<u32> {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { mem::zeroed() };
            if unsafe {
                QueryInformationJobObject(
                    self.0 as _,
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut _,
                    mem::size_of_val(&info) as u32,
                    ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(info.ActiveProcesses)
        }
    }
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0 as _);
            }
        }
    }
}
#[cfg(windows)]
pub use windows::Job;
#[cfg(not(windows))]
pub struct Job;
#[cfg(not(windows))]
impl Job {
    pub fn new() -> std::io::Result<Self> {
        Err(std::io::Error::other(
            "This terminal currently requires Windows ConPTY.",
        ))
    }
    pub fn configure(&self, _: &mut portable_pty::CommandBuilder) {}
    pub fn terminate(&self) -> std::io::Result<()> {
        Ok(())
    }
    pub fn active(&self) -> std::io::Result<u32> {
        Ok(0)
    }
}
