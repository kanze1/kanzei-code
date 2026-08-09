//! 共享测试辅助(R-155 S8):内存库 + 会话引导。
//! 设计 §C:共享测试辅助建 #[cfg(test)] pub(crate) mod testutil。

use super::SessionStore;

pub(crate) fn store() -> SessionStore {
    let store = SessionStore::open_in_memory().unwrap();
    store
        .create_session("ses_test", "C:/project", None)
        .unwrap();
    store
}
