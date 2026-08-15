//! 运行时上限域(R-257 B5):Limits 结构与默认值方法。自 config.rs 原样迁出,
//! 零行为变更。

use serde::{Deserialize, Serialize};

/// 运行时上限与阈值。此前全部是散落在各 crate 里的硬编码常量,配置层没有任何入口——
/// 想调一个输出预算就得改代码重编译。
///
/// 每个字段都是 Option:None = 用内置默认(即改造前那个常量值),所以旧配置没有
/// `[limits]` 节时行为逐字节不变(conventions §4 向后兼容);层叠合并也照既有规矩来——
/// 项目层只覆盖它显式写了的那几个键,不会把没写的键打回默认值。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Limits {
    /// 主对话单次输出上限(tokens)
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// 子代理单次输出上限(tokens)
    #[serde(default)]
    pub subagent_max_tokens: Option<u32>,
    /// 单个子代理的墙钟上限(秒)
    #[serde(default)]
    pub subagent_timeout_secs: Option<u64>,
    /// R-173 汇总/复核屏障的墙钟上界(秒)。None = 由 subagent_timeout_secs 推导
    #[serde(default)]
    pub barrier_timeout_secs: Option<u64>,
    /// 轮内主动压缩的触发线:占上下文窗口的比例
    #[serde(default)]
    pub context_budget_ratio: Option<f64>,
    /// 压缩时末尾逐字保留的比例
    #[serde(default)]
    pub recent_verbatim_ratio: Option<f64>,
    /// 单轮最多派发多少个 task 子代理
    #[serde(default)]
    pub max_tasks_per_turn: Option<usize>,
    /// 单波最多并行多少个工具
    #[serde(default)]
    pub max_parallel_tools: Option<usize>,
    /// 传输层重试次数
    #[serde(default)]
    pub transport_retries: Option<u32>,
    /// 限流重试次数
    #[serde(default)]
    pub rate_limit_retries: Option<u32>,
    /// 流中断后重放本轮的次数上限
    #[serde(default)]
    pub stream_restarts: Option<u32>,
    /// R-236 B1:压缩触发的 headroom 预留(tokens)。触发线 = context_limit −
    /// max(max_tokens, 本值);对齐 opencode 的 `limit − max(output, buffer 20k)`,
    /// 替代旧的 context_budget_ratio 比例线(该键保留但不再被触发路径消费)。
    #[serde(default)]
    pub compact_buffer_tokens: Option<u64>,
    /// R-236 B4:prune(机械清理旧工具结果)保护窗——最近这么多 token 的工具
    /// 结果与最近两个用户轮逐字保留,不清。
    #[serde(default)]
    pub prune_protect_tokens: Option<u64>,
    /// R-236 B4:prune 最小收益门槛——可回收量低于此值就不做(不值得打破缓存前缀)。
    #[serde(default)]
    pub prune_min_gain_tokens: Option<u64>,
}

impl Limits {
    pub fn max_tokens(&self) -> u32 {
        self.max_tokens.unwrap_or(8192)
    }
    pub fn subagent_max_tokens(&self) -> u32 {
        self.subagent_max_tokens.unwrap_or(4096)
    }
    pub fn subagent_timeout_secs(&self) -> u64 {
        self.subagent_timeout_secs.unwrap_or(900)
    }
    /// R-173 屏障上界:默认由 `subagent_timeout_secs` 推导(×2),不另拍一个数。
    ///
    /// **外层必须永远宽于内层**——每个勘察子代理已被 `subagent_timeout_secs`
    /// 的墙钟包住(见 runner drive 的 task 派发),屏障只是"内层失效时"的兜底。
    /// 把本值配成小于等于子代理上界,会让屏障在子代理**正常工作**时就把它判成
    /// 超时,凭空制造假失败。所以显式配置也按下界夹紧(与本节 `max_parallel_tools`
    /// 的 `.max(1)`、`context_budget_ratio` 的 `.clamp()` 同一口径)。
    pub fn barrier_timeout_secs(&self) -> u64 {
        let inner = self.subagent_timeout_secs();
        self.barrier_timeout_secs
            .unwrap_or(inner.saturating_mul(2))
            .max(inner.saturating_add(1))
    }
    pub fn context_budget_ratio(&self) -> f64 {
        self.context_budget_ratio.unwrap_or(0.7).clamp(0.1, 0.95)
    }
    pub fn recent_verbatim_ratio(&self) -> f64 {
        self.recent_verbatim_ratio.unwrap_or(0.35).clamp(0.05, 0.9)
    }
    pub fn compact_buffer_tokens(&self) -> u64 {
        self.compact_buffer_tokens.unwrap_or(20_000).max(1_000)
    }
    pub fn prune_protect_tokens(&self) -> u64 {
        self.prune_protect_tokens.unwrap_or(40_000)
    }
    pub fn prune_min_gain_tokens(&self) -> u64 {
        self.prune_min_gain_tokens.unwrap_or(20_000)
    }
    pub fn max_tasks_per_turn(&self) -> usize {
        // R-174:默认从 8 上调到 16——用户要「远不止 8」(2026-08-10 看过 Claude Code
        // 的后台面板后定调)。编排勘察角色表共 8 名(5 勘察 + 3 复核),16 容得下完整
        // 角色表,又给模型自派 task 留出双倍并行余量。
        self.max_tasks_per_turn.unwrap_or(16).max(1)
    }
    pub fn max_parallel_tools(&self) -> usize {
        self.max_parallel_tools.unwrap_or(8).max(1)
    }
    pub fn transport_retries(&self) -> u32 {
        self.transport_retries.unwrap_or(2)
    }
    pub fn rate_limit_retries(&self) -> u32 {
        self.rate_limit_retries.unwrap_or(2)
    }
    pub fn stream_restarts(&self) -> u32 {
        self.stream_restarts.unwrap_or(2)
    }
}

/// kanzei.toml [limits] 节已知键名单(R-220 单源)。
pub(crate) const LIMITS_KEYS: &[&str] = &[
    "max_tokens",
    "subagent_max_tokens",
    "subagent_timeout_secs",
    "barrier_timeout_secs",
    "context_budget_ratio",
    "recent_verbatim_ratio",
    "max_tasks_per_turn",
    "max_parallel_tools",
    "transport_retries",
    "rate_limit_retries",
    "stream_restarts",
    "compact_buffer_tokens",
    "prune_protect_tokens",
    "prune_min_gain_tokens",
];
