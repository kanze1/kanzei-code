//! Windows kernel read-cache leases. A broken lease never authorizes reuse.
//!
//! RH leases close their handle on an I/O completion thread, acknowledging the
//! break before an atomic replacement proceeds. There is one completion owner
//! per collector, not one thread per file. Pending request buffers remain alive
//! until the completion owner has observed cancellation or a real break.
use std::cell::UnsafeCell;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Ioctl::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::System::IO::*;

pub(super) struct Backend(Arc<CompletionOwner>);
struct CompletionOwner {
    port: OwnedHandle,
    worker: Option<std::thread::JoinHandle<()>>,
}
struct Request {
    input: REQUEST_OPLOCK_INPUT_BUFFER,
    output: REQUEST_OPLOCK_OUTPUT_BUFFER,
    overlapped: OVERLAPPED,
}
struct Pending {
    state: Mutex<State>,
    completed: Condvar,
    event: OwnedHandle,
    request: UnsafeCell<Request>,
}
struct State {
    file: Option<OwnedHandle>,
    finished: bool,
}
// Only the kernel writes request/output memory while I/O is pending. Rust
// initializes it before submission and never reads it concurrently. The file
// handle and completion lifetime are synchronized by state/completed.
unsafe impl Send for Pending {}
unsafe impl Sync for Pending {}

pub(super) struct Lease {
    pending: Arc<Pending>,
    _owner: Arc<CompletionOwner>,
}
impl Backend {
    pub(super) fn new() -> io::Result<Self> {
        let port =
            unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, std::ptr::null_mut(), 0, 1) };
        if port.is_null() {
            return Err(io::Error::last_os_error());
        }
        let port = unsafe { OwnedHandle::from_raw_handle(port) };
        let raw = port.as_raw_handle() as usize;
        let worker = std::thread::Builder::new()
            .name("managed-read-leases".into())
            .spawn(move || {
                loop {
                    let mut bytes = 0;
                    let mut key = 0;
                    let mut overlapped = std::ptr::null_mut();
                    // A failed overlapped operation still delivers its completion;
                    // shutdown alone uses the null overlapped sentinel.
                    unsafe {
                        GetQueuedCompletionStatus(
                            raw as _,
                            &mut bytes,
                            &mut key,
                            &mut overlapped,
                            INFINITE,
                        );
                    }
                    if overlapped.is_null() {
                        break;
                    }
                    let pending = unsafe { Arc::from_raw(key as *const Pending) };
                    let mut state = pending
                        .state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    // Closing acknowledges RH breaks, including rename/delete.
                    state.file.take();
                    state.finished = true;
                    pending.completed.notify_all();
                }
            })?;
        Ok(Self(Arc::new(CompletionOwner {
            port,
            worker: Some(worker),
        })))
    }

    pub(super) fn acquire(&self, path: &Path) -> io::Result<Option<Lease>> {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // No data access is required by the notification handle. Delete access
        // is intentionally not shared: RH closes on a conflicting delete open,
        // so legacy MoveFileEx replacement does not encounter our open handle.
        let file = unsafe {
            CreateFileW(
                wide.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if file == INVALID_HANDLE_VALUE {
            return unavailable(path, io::Error::last_os_error());
        }
        let file = unsafe { OwnedHandle::from_raw_handle(file) };
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let event = unsafe { OwnedHandle::from_raw_handle(event) };
        let request = Request {
            input: REQUEST_OPLOCK_INPUT_BUFFER {
                StructureVersion: REQUEST_OPLOCK_CURRENT_VERSION as _,
                StructureLength: std::mem::size_of::<REQUEST_OPLOCK_INPUT_BUFFER>() as _,
                RequestedOplockLevel: OPLOCK_LEVEL_CACHE_READ | OPLOCK_LEVEL_CACHE_HANDLE,
                Flags: REQUEST_OPLOCK_INPUT_FLAG_REQUEST,
            },
            output: REQUEST_OPLOCK_OUTPUT_BUFFER::default(),
            overlapped: OVERLAPPED {
                hEvent: event.as_raw_handle(),
                ..Default::default()
            },
        };
        let pending = Arc::new(Pending {
            state: Mutex::new(State {
                file: Some(file),
                finished: false,
            }),
            completed: Condvar::new(),
            event,
            request: UnsafeCell::new(request),
        });
        let state = pending
            .state
            .lock()
            .map_err(|_| io::Error::other("lease state poisoned"))?;
        let file = state
            .file
            .as_ref()
            .expect("unsubmitted lease owns file")
            .as_raw_handle();
        let key = Arc::into_raw(pending.clone());
        let associated =
            unsafe { CreateIoCompletionPort(file, self.0.port.as_raw_handle(), key as usize, 0) };
        if associated.is_null() {
            let error = io::Error::last_os_error();
            unsafe {
                drop(Arc::from_raw(key));
            }
            return Err(error);
        }
        let request = unsafe { &mut *pending.request.get() };
        let accepted = unsafe {
            DeviceIoControl(
                file,
                FSCTL_REQUEST_OPLOCK,
                std::ptr::from_ref(&request.input).cast(),
                std::mem::size_of_val(&request.input) as _,
                std::ptr::from_mut(&mut request.output).cast(),
                std::mem::size_of_val(&request.output) as _,
                std::ptr::null_mut(),
                &mut request.overlapped,
            )
        };
        let error = io::Error::last_os_error();
        drop(state);
        if accepted == 0 && error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
            // A synchronously rejected request does not enqueue a completion.
            unsafe {
                drop(Arc::from_raw(key));
            }
            return unavailable(path, error);
        }
        Ok(Some(Lease {
            pending,
            _owner: self.0.clone(),
        }))
    }
}
fn unavailable(path: &Path, error: io::Error) -> io::Result<Option<Lease>> {
    match error.raw_os_error().map(|code| code as u32) {
        Some(
            ERROR_OPLOCK_NOT_GRANTED
            | ERROR_CANNOT_GRANT_REQUESTED_OPLOCK
            | ERROR_INVALID_FUNCTION
            | ERROR_NOT_SUPPORTED
            | ERROR_INVALID_PARAMETER
            | ERROR_SHARING_VIOLATION
            | ERROR_ACCESS_DENIED
            | ERROR_FILE_NOT_FOUND
            | ERROR_PATH_NOT_FOUND,
        ) => {
            tracing::debug!(path=%path.display(), %error, "managed path has no kernel read-cache lease; content will be read uncached");
            Ok(None)
        }
        _ => Err(error),
    }
}
impl Lease {
    pub(super) fn valid(&self) -> bool {
        unsafe { WaitForSingleObject(self.pending.event.as_raw_handle(), 0) == WAIT_TIMEOUT }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        // Cancellation completion owns the final kernel buffer access. Never
        // free buffers or close the port while that request is still pending.
        let mut state = self
            .pending
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(file) = &state.file {
            unsafe {
                CancelIoEx(
                    file.as_raw_handle(),
                    &(*self.pending.request.get()).overlapped,
                );
            }
        }
        while !state.finished {
            state = self
                .pending
                .completed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}
impl Drop for CompletionOwner {
    fn drop(&mut self) {
        unsafe {
            PostQueuedCompletionStatus(self.port.as_raw_handle(), 0, 0, std::ptr::null());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
