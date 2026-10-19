//! One coordination namespace per OS account. Configuration roots, environment
//! profiles, caches and task TEMP directories must not split the same file lock.

use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;

pub(super) fn path() -> io::Result<PathBuf> {
    static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
    if let Some(directory) = DIRECTORY.get() {
        return Ok(directory.clone());
    }
    let home = account_home()?;
    if !home.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "OS account profile must be an absolute path",
        ));
    }
    #[cfg(windows)]
    let cache = home.join("AppData/Local");
    #[cfg(target_os = "macos")]
    let cache = home.join("Library/Caches");
    #[cfg(all(unix, not(target_os = "macos")))]
    let cache = home.join(".cache");
    let directory = crate::path_form::resolve_target(&cache.join("kanzei/file-locks"))?;
    Ok(DIRECTORY.get_or_init(|| directory).clone())
}

#[cfg(windows)]
fn account_home() -> io::Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Security::TOKEN_QUERY;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::UI::Shell::GetUserProfileDirectoryW;

    let mut token = std::ptr::null_mut();
    // SAFETY: a valid process pseudo-handle and writable token output are passed.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful OpenProcessToken transfers this owned handle to us.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut length = 0;
    // SAFETY: a null buffer queries the required length for this valid token.
    unsafe { GetUserProfileDirectoryW(token.as_raw_handle(), std::ptr::null_mut(), &mut length) };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0; length as usize];
    // SAFETY: the writable UTF-16 buffer has the size returned by the first call.
    if unsafe { GetUserProfileDirectoryW(token.as_raw_handle(), buffer.as_mut_ptr(), &mut length) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    let end = buffer.iter().position(|unit| *unit == 0).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "OS account profile is unterminated",
        )
    })?;
    Ok(PathBuf::from(OsString::from_wide(&buffer[..end])))
}

#[cfg(unix)]
fn account_home() -> io::Result<PathBuf> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;

    // SAFETY: sysconf takes a constant and does not retain any borrowed data.
    let length = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let length = usize::try_from(length)
        .ok()
        .filter(|size| *size > 0)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "OS account buffer size unavailable",
            )
        })?;
    let mut buffer = vec![0; length];
    let mut account = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    // SAFETY: all output storage is writable and remains alive through the copy.
    let status = unsafe {
        libc::getpwuid_r(
            libc::geteuid(),
            account.as_mut_ptr(),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    if result.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "OS account unavailable",
        ));
    }
    // SAFETY: getpwuid_r initialized this account and its strings in buffer.
    let account = unsafe { account.assume_init() };
    if account.pw_dir.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "OS account profile unavailable",
        ));
    }
    // SAFETY: pw_dir is a terminated string in the live lookup buffer.
    Ok(PathBuf::from(OsStr::from_bytes(unsafe {
        CStr::from_ptr(account.pw_dir).to_bytes()
    })))
}
