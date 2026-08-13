//! kanzei-base:零依赖底层原语(R-208)。
//!
//! 承接原寄居在 kanzei-llm 的文件系统原语(atomic_file/FileLock,见
//! `docs/design/` 的 D-261 决策)。llm 是依赖图最底层,而 harness 不能反向依赖
//! llm——R-181 的跨进程 lease 契约在 harness、原语却在 llm,照单实施会撞依赖
//! 方向墙。拆出本 crate 后:llm/tools 从它取原语,harness 也可直接依赖。
//!
//! 本 crate 刻意保持零依赖:只放跨 crate 共享、无业务语义、纯 std 能实现的底座,
//! 不承担任何业务规则。

pub mod atomic_file;
