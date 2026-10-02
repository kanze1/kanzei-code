//! 验证 worker 必须脱离调用者的输出管道；仅重定向 stdio 仍会继承其他可继承句柄。
//! https://learn.microsoft.com/en-us/windows/win32/procthread/inheritance
use super::*;

#[cfg(not(windows))]
pub(super) fn spawn(job: &VerificationJob) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .arg("--verification-worker")
        .arg(&job.project)
        .arg(&job.id)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    command.spawn()?;
    Ok(())
}

#[cfg(windows)]
pub(super) fn spawn(job: &VerificationJob) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    #[repr(C)]
    struct StartupInfo {
        cb: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        x: u32,
        y: u32,
        x_size: u32,
        y_size: u32,
        x_count: u32,
        y_count: u32,
        fill: u32,
        flags: u32,
        show: u16,
        reserved_size: u16,
        reserved_bytes: *mut u8,
        stdin: isize,
        stdout: isize,
        stderr: isize,
    }
    #[repr(C)]
    struct ProcessInfo {
        process: isize,
        thread: isize,
        pid: u32,
        tid: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateProcessW(
            app: *const u16,
            command: *mut u16,
            process_attributes: *mut std::ffi::c_void,
            thread_attributes: *mut std::ffi::c_void,
            inherit: i32,
            flags: u32,
            environment: *mut std::ffi::c_void,
            cwd: *const u16,
            startup: *mut StartupInfo,
            info: *mut ProcessInfo,
        ) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    fn quote(value: &std::ffi::OsStr) -> Vec<u16> {
        let mut out = vec![b'"' as u16];
        let mut slashes = 0;
        for ch in value.encode_wide() {
            if ch == b'\\' as u16 {
                slashes += 1;
                continue;
            }
            out.extend(std::iter::repeat_n(
                b'\\' as u16,
                if ch == b'"' as u16 {
                    slashes * 2 + 1
                } else {
                    slashes
                },
            ));
            out.push(ch);
            slashes = 0;
        }
        out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
        out.push(b'"' as u16);
        out
    }
    let exe = std::env::current_exe()?;
    let app: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut command = Vec::new();
    for arg in [
        exe.as_os_str(),
        std::ffi::OsStr::new("--verification-worker"),
        job.project.as_os_str(),
        std::ffi::OsStr::new(&job.id),
    ] {
        if !command.is_empty() {
            command.push(b' ' as u16);
        }
        command.extend(quote(arg));
    }
    command.push(0);
    // SAFETY: 两个 repr(C) 结构按 Win32 定义初始化；宽字符串在调用期间有效。
    // inherit=FALSE 避免 Node/终端的捕获管道随 worker 延长寿命。两个返回句柄在这里关闭。
    unsafe {
        let mut startup: StartupInfo = std::mem::zeroed();
        startup.cb = std::mem::size_of::<StartupInfo>() as u32;
        let mut info: ProcessInfo = std::mem::zeroed();
        if CreateProcessW(
            app.as_ptr(),
            command.as_mut_ptr(),
            null_mut(),
            null_mut(),
            0,
            0x00000008 | 0x00000200,
            null_mut(),
            std::ptr::null(),
            &mut startup,
            &mut info,
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        CloseHandle(info.thread);
        CloseHandle(info.process);
    }
    Ok(())
}

/// 独立 worker 的进程树随 worker 退出收口，避免崩溃后遗留 Flutter/构建进程。
#[cfg(windows)]
pub(super) fn contain_worker() -> std::io::Result<()> {
    #[repr(C)]
    struct Basic {
        process_time: i64,
        job_time: i64,
        flags: u32,
        min_working: usize,
        max_working: usize,
        active: u32,
        affinity: usize,
        priority: u32,
        scheduling: u32,
    }
    #[repr(C)]
    struct Extended {
        basic: Basic,
        io: [u64; 6],
        process_memory: usize,
        job_memory: usize,
        peak_process: usize,
        peak_job: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateJobObjectW(attributes: *mut std::ffi::c_void, name: *const u16) -> isize;
        fn SetInformationJobObject(
            job: isize,
            class: i32,
            info: *const std::ffi::c_void,
            len: u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: isize, process: isize) -> i32;
        fn GetCurrentProcess() -> isize;
        fn CloseHandle(handle: isize) -> i32;
    }
    // SAFETY: 新建匿名 Job 仅绑定当前专用 worker；不影响调用方或其他应用。
    // 成功后句柄保留到进程退出由 OS 关闭，KILL_ON_JOB_CLOSE 同时终止残留子进程。
    unsafe {
        let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
        if job == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut info: Extended = std::mem::zeroed();
        info.basic.flags = 0x00002000;
        if SetInformationJobObject(
            job,
            9,
            &info as *const _ as *const _,
            std::mem::size_of::<Extended>() as u32,
        ) == 0
            || AssignProcessToJobObject(job, GetCurrentProcess()) == 0
        {
            let error = std::io::Error::last_os_error();
            CloseHandle(job);
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub(super) fn contain_worker() -> std::io::Result<()> {
    Ok(())
}
