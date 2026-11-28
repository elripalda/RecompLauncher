//! Start suspended so no compiler can escape ownership before the job is assigned.
use std::{os::windows::io::AsRawHandle, process::Child};
use windows::Win32::{Foundation::{CloseHandle, HANDLE}, System::{
    Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Thread32First, Thread32Next, THREADENTRY32, TH32CS_SNAPTHREAD},
    JobObjects::{AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject, TerminateJobObject, JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE},
    Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
}};

struct Handle(HANDLE);
impl Drop for Handle { fn drop(&mut self) { unsafe { let _ = CloseHandle(self.0); } } }

pub struct Job(Handle);
impl Job {
    pub fn attach(child: &Child) -> Result<Self, String> {
        let attach = || -> windows::core::Result<Self> {
            unsafe {
                let job = Self(Handle(CreateJobObjectW(None, windows::core::PCWSTR::null())?));
                let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(job.0.0, JobObjectExtendedLimitInformation, &limits as *const _ as _, std::mem::size_of_val(&limits) as u32)?;
                AssignProcessToJobObject(job.0.0, HANDLE(child.as_raw_handle()))?;
                let snapshot = Handle(CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD,0)?);
                let mut entry = THREADENTRY32 { dwSize:std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
                Thread32First(snapshot.0,&mut entry)?;
                loop {
                    if entry.th32OwnerProcessID == child.id() {
                        let thread = Handle(OpenThread(THREAD_SUSPEND_RESUME,false,entry.th32ThreadID)?);
                        if ResumeThread(thread.0) == u32::MAX { return Err(windows::core::Error::from_thread()); }
                        return Ok(job);
                    }
                    Thread32Next(snapshot.0,&mut entry)?;
                }
            }
        };
        attach().map_err(|e| format!("Couldn't safely start game preparation: {e}"))
    }
    pub fn stop(&self) { unsafe { let _ = TerminateJobObject(self.0.0,1); } }
}
