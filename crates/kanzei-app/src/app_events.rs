//! Business events and managed application state do not require a window.
//! AppHandle already supplies the shared Emitter, Manager and CommandArg
//! contracts; both desktop IPC and the native runtime inject this same owner.
pub(crate) type AppEvents = tauri::AppHandle;
