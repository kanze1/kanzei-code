//! App 端对共享体验事件契约的 crate-local 入口(R-284 B2)。
//!
//! 契约唯一实现位于 `kanzei_core::experience_events`；app 只保留这一层入口，
//! 避免 UI 运行事件与 memory/research 持久事实各自复制词表。

pub(crate) use kanzei_core::experience_events::*;
