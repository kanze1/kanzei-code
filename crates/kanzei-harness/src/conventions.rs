//! 通用开发规范(单源)。
//!
//! 所有项目默认注入 agent 上下文的**通用**开发规则,编译期内嵌于本 crate。
//! 项目特有的规则(分支策略、架构契约、构建发版等)放各自
//! `.kanzei/project/conventions.md`,注入时引擎把本模板与项目文件拼接。
//! 通用规则一律改这里,不要在项目文件里复制——复制必然漂移(D-279 跨项目不一致)。

/// 通用开发规范全文。
pub const DEFAULT_CONVENTIONS: &str = include_str!("../assets/default_conventions.md");
