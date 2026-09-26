//! refgraph:从 Markdown 真源机械抽取引用关系的纯函数(R-368 B1 的最小子集)。
//!
//! 本批落地记忆知识图谱需要的部分(docs/design/memory_knowledge_graph.md):
//! - [`mentions`]:正文里的编号提及(ASCII 边界手工判定——regex crate 没有 look-around,
//!   而 `\b` 在 Unicode 下把中文算作 `\w`,「见R-123」会漏);
//! - [`tool_areas`]:工具名 / 关键词 → 代码区域的受控词表;
//! - [`memory_graph`]:记忆图谱投影(`collect_inputs` 负责全部 IO,`build_graph` 是纯函数)。
//!
//! 分词统一走 `kanzei_harness::refs::split_refs`,区域统一走 `kanzei_harness::areas::AreaRegistry`。
//! 图是只读投影(A-015),唯一新增的真源字段是记忆 frontmatter 的可选 `area:`。

pub mod memory_graph;
pub mod mentions;
pub mod tool_areas;

#[cfg(test)]
mod tests;

pub use memory_graph::{
    build_graph, collect_inputs, collect_inputs_from, input_fingerprint, memory_stores, GraphArea,
    GraphEdge, GraphInputs, GraphNode, GraphStats, MemoryGraph, MemoryInput, ProvenanceCounts,
    TrackerInput,
};
