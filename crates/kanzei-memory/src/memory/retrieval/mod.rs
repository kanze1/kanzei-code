//! 记忆检索域(R-255 第三刀,从 store.rs 迁出)。
//!
//! 独立理由:检索是「怎么从记忆里找东西」的独立变更理由——FTS 候选集访问
//! (search_candidates)、意图词提取(intent_query)、召回观测(record_recall/recalls)、
//! 指纹精查(find_by_marker)互不相关。它与准入(admission)、生命周期(lifecycle)、
//! 收件箱(inbox)正交:改召回策略不必读懂准入策略(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):存储侧只产候选不做排序决策——`search_candidates` 的 bm25
//! 原样保留(FTS5 负值,越小越相关),ranking/状态加权/采纳率决策只属于检索门面
//! (SqliteMemoryIndex,D-366);shadow 条目不注入生产检索(硬排除,只有显式查
//! shadow 才可见);召回观测(record_recall)与命中追踪(record_hits)由检索门面在
//! 决策排序完成后调用,纯探测不记,避免污染观测。

mod recall;
mod search;

pub(crate) use search::{intent_query, segment_cjk};
