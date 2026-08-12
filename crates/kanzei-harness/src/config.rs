//! kanzei.toml:全局(~/.kanzei/)→ 项目(.kanzei/,从 cwd 向上发现),后者覆盖前者。
//! 配置本身以组件形式进入 harness(贡献权限规则),没有第二条 config→runtime 路径。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::permission::{is_structured_bash_resource, normalize_resource, Rule, BASH_ACTION};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct KanzeiConfig {
    #[serde(default)]
    pub models: ModelRoles,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    /// "env"(默认)| "off" | 代理地址。
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub profile: ProfileSection,
    #[serde(default)]
    pub permissions: PermissionsSection,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub cadence: Cadence,
    /// 向量检索通道(R-164):配置了 provider:model 才启用 dense/hybrid。
    /// 未配置时系统退化为 lexical 通道,功能完整(设计 §5 验收①)。
    #[serde(default)]
    pub embeddings: EmbeddingsSection,
}

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

/// 向量检索通道配置(R-164)。两个字段都带 serde default:
/// 旧配置没有 `[embeddings]` 节时通道关闭,检索退化为 lexical(设计 §5 验收①)。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EmbeddingsSection {
    /// provider 名(providers 表里的键,如 "ollama")。None = 通道关闭。
    #[serde(default)]
    pub provider: Option<String>,
    /// 模型名(如 "nomic-embed-text" / "text-embedding-3-small")。
    #[serde(default)]
    pub model: Option<String>,
}

impl EmbeddingsSection {
    /// 通道是否启用:provider 与 model 都配置了才生效。
    pub fn enabled(&self) -> bool {
        self.provider.as_deref().is_some_and(|p| !p.is_empty())
            && self.model.as_deref().is_some_and(|m| !m.is_empty())
    }
}

/// 验证与提交节奏(R-157):把 conventions §1.4 的节奏参数从提示词硬化成可调配置。
/// 每个字段都带 serde default,旧配置没有 `[cadence]` 节时行为与 §1.4 当前默认
/// 逐项一致(conventions §4 向后兼容);层叠合并照既有规矩——项目层只覆盖显式写的键。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Cadence {
    /// 全量测试触发档位。默认 EntryClose:条目关闭前一次(发版前 verify.ps1 是
    /// 独立硬门禁,不受本参数影响,见 A-010)。
    #[serde(default)]
    pub full_test: FullTestCadence,
    /// full_test == EveryNBatches 时的批次间隔 n。
    #[serde(default)]
    pub full_test_batches: Option<u32>,
    /// 定向测试:每次提交前必跑(默认)| off。
    #[serde(default)]
    pub targeted_test: TargetedTestCadence,
    /// 提交粒度:每条目一提交 | 每批一提交(默认,多批大条目按批提交)。
    #[serde(default)]
    pub commit: CommitCadence,
    /// push 频率:条目完成后 push(默认)| 每提交后 push | 定期(与 R-143 并轨)。
    #[serde(default)]
    pub push: PushCadence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FullTestCadence {
    /// 条目关闭前一次(§1.4 默认)。
    #[default]
    EntryClose,
    /// 每次提交前全量。
    EveryCommit,
    /// 每 n 批全量一次,间隔见 Cadence::full_test_batches。
    EveryNBatches,
    /// 只发版前跑(verify.ps1 硬门禁,本地开发不跑全量)。
    ReleaseOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetedTestCadence {
    /// 每次提交前必跑(§1.4 默认)。
    #[default]
    EveryCommit,
    /// 关闭定向测试(不推荐;改动面与验证匹配的判断交给模型)。
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitCadence {
    /// 多批大条目每批一提交(§1.4 默认)。
    #[default]
    PerBatch,
    /// 整条目一提交(复杂度小的条目适用)。
    PerEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PushCadence {
    /// 条目完成后 push(§1.4 默认)。
    #[default]
    PerEntry,
    /// 每提交后顺手 push。
    PerCommit,
    /// 定期自动 push(与 R-143 自举循环自动 push 并轨)。
    Periodic,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelRoles {
    pub primary: Option<String>,
    pub fast: Option<String>,
    /// 思考强度默认档:"off"(默认)| "low" | "medium" | "high"。
    /// 运行时可被桌面端的每进程选择覆盖;未配置时保持 off,行为与既有一致。
    #[serde(default)]
    pub reasoning: Option<String>,
    /// Codex Fast mode:同一模型使用更高消耗的 priority 服务档位。
    #[serde(default)]
    pub codex_fast_mode: Option<bool>,
    /// R-173:阶段编排派发的只读代理(勘察 + 复核)用哪条路由。
    ///
    /// 取值与 primary/fast 同一套解析(角色名或 `provider:model`)。
    /// `None` = 沿用 `fast`,与引入前逐字节一致。
    ///
    /// 单独一个键的理由:勘察/复核是**读密集、上下文短、要看懂代码**的活,
    /// 它该用哪个模型与「主对话用哪个」「机械检索用哪个」都不是同一个问题。
    /// 写死 fast 等于假定勘察是廉价活——用户跑 DeepSeek 时这个假设不成立,
    /// 白白让勘察质量降级。
    #[serde(default)]
    pub scout: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderConfig {
    /// "anthropic" | "openai" | "openai-responses"
    pub protocol: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// 直填 API key(个人工具的便利通道,优先于 api_key_env;明文存 toml,自担风险)。
    #[serde(default)]
    pub api_key: Option<String>,
    /// 特殊认证:"codex" = 复用 Codex CLI 登录态,"claude" = 复用 Claude Code 登录态。
    #[serde(default)]
    pub auth: Option<String>,
    /// 上下文窗口(token)。用于界面占用比例显示与(M2)压缩预检。
    #[serde(default)]
    pub context_limit: Option<u64>,
}

#[cfg(test)]
mod context_limit_tests {
    use super::*;

    /// D-173:`entry().or_insert()` 只在整条 provider 缺失时生效,用户配了同名
    /// provider 却漏写 context_limit 时补不上——实测用户的 deepseek/codex/anthropic
    /// 全都没有上限值,UI 占用比例与压缩预检因此全部失去基准。
    #[test]
    fn 用户配置里缺失的上下文上限会被按已知值回填() {
        let mut config = KanzeiConfig::default();
        for (name, base_url) in [
            ("deepseek", "https://api.deepseek.com"),
            ("codex", "https://chatgpt.com/backend-api/codex"),
            ("anthropic", "https://api.anthropic.com"),
        ] {
            config.providers.insert(
                name.into(),
                ProviderConfig {
                    protocol: "openai".into(),
                    base_url: base_url.into(),
                    api_key_env: None,
                    api_key: None,
                    auth: None,
                    context_limit: None,
                },
            );
        }
        // 用户显式写死的值不能被覆盖。
        config.providers.insert(
            "kimi".into(),
            ProviderConfig {
                protocol: "openai".into(),
                base_url: "https://api.moonshot.cn/v1".into(),
                api_key_env: None,
                api_key: None,
                auth: None,
                context_limit: Some(1_000_000),
            },
        );
        config.fill_defaults();

        assert_eq!(config.providers["deepseek"].context_limit, Some(1_000_000));
        assert_eq!(config.providers["codex"].context_limit, Some(272_000));
        assert_eq!(config.providers["anthropic"].context_limit, Some(200_000));
        assert_eq!(config.providers["kimi"].context_limit, Some(1_000_000));
        // 认不出来的 provider 保持 None——宁可显示"未知",也不编一个预算基准。
        assert_eq!(known_context_limit("mystery", "https://example.test"), None);
    }

    /// D-246:内置名单与 fill_defaults 实际回填必须一致,否则 UI 的「内置」标记
    /// 会漏标/错标——名单漂移比没有名单更糟。
    #[test]
    fn 内置名单与fill_defaults回填一致() {
        let mut config = KanzeiConfig::default();
        config.providers.clear();
        config.fill_defaults();
        for name in builtin_provider_names() {
            assert!(
                config.providers.contains_key(*name),
                "fill_defaults 未回填内置 {name}"
            );
        }
        // 名单里不该有 fill_defaults 不保证的键(用户自定义的不能误标内置)。
        assert_eq!(config.providers.len(), builtin_provider_names().len());
    }
}

/// 已知 provider 的上下文窗口。名字优先,其次看 base_url 主机名(用户可能改名)。
/// 认不出来就返回 None——宁可让 UI 显示"未知",也不编一个数字当预算基准。
fn known_context_limit(name: &str, base_url: &str) -> Option<u64> {
    let name = name.to_ascii_lowercase();
    for (needle, limit) in [
        ("anthropic", 200_000u64),
        ("claude", 200_000),
        ("codex", 272_000),
        ("openai", 272_000),
        ("deepseek", 1_000_000),
        ("moonshot", 128_000),
        ("kimi", 128_000),
        ("ollama", 32_000),
    ] {
        if name.contains(needle) || base_url.contains(needle) {
            return Some(limit);
        }
    }
    None
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProfileSection {
    pub default: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PermissionsSection {
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// 无 TTY 时(脚手架派发、CI)遇到 Ask 该怎么办。缺键 = [`NonInteractive::Deny`],
    /// 也就是**今天的行为**:EOF → Deny → 停机。
    ///
    /// 类型故意留成 `Option<String>` 而不是枚举:未知取值(来自更新版本的 kanzei,
    /// 或者手抖拼错)**不能炸掉启动**,只能 fail-closed 回落 deny 再产一条告警。
    /// 读的时候走 [`KanzeiConfig::non_interactive_policy`],别直接读这个字段。
    ///
    /// 本批只落 schema 与告警,**暂时没有消费者**——策略真正生效在后续批次。
    /// 三处接线(`unknown_keys` / `merge` / 告警)在本批一次做齐,各有一条命名测试守着:
    /// `Limits::barrier_timeout_secs` 就栽在只加字段没接 merge,项目层设了却静默不生效。
    #[serde(default)]
    pub non_interactive: Option<String>,
}

/// 无 TTY 时的 Ask 处置策略(R-183 内容①)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NonInteractive {
    /// 今天的行为:问不出来就停机。**缺省档**,也是所有解析失败的回落档。
    #[default]
    Deny,
    /// 不问,直接按规则集判定;Ask 当作 deny 回喂模型并继续本轮。
    RulesOnly,
    /// 同 `RulesOnly`,外加本次运行由操作员显式提供的 allowlist。
    AllowListed,
}

impl NonInteractive {
    /// 配置里认的取值。写在一处,解析与告警文案共用,免得两边漂移。
    const KNOWN: [(&'static str, NonInteractive); 3] = [
        ("deny", NonInteractive::Deny),
        ("rules_only", NonInteractive::RulesOnly),
        ("allow_listed", NonInteractive::AllowListed),
    ];

    fn parse(text: &str) -> Option<NonInteractive> {
        let key = text.trim().to_ascii_lowercase();
        Self::KNOWN
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
    }

    fn known_names() -> String {
        Self::KNOWN
            .iter()
            .map(|(name, _)| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(" / ")
    }
}

/// 解析后的模型指向。
#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub provider_name: String,
    pub provider: ProviderConfig,
    pub model: String,
}

impl KanzeiConfig {
    /// 全局 + 项目层叠加载。语法/类型错误返回错误(配置错误要炸在启动,不能静默);
    /// 未知字段宽容忽略但产生告警(D-084:schema 向后兼容,新旧二进制可共用同一文件)。
    pub fn load(cwd: &Path) -> anyhow::Result<KanzeiConfig> {
        let (config, warnings) = Self::load_with_warnings(cwd)?;
        for warning in &warnings {
            tracing::warn!("{warning}");
        }
        Ok(config)
    }

    /// 同 load,但把未知字段告警交给调用方展示(CLI stderr / 桌面 kz:status)。
    ///
    /// 语义不变的**发现式**入口:从 cwd 向上找项目根,再委托 [`Self::load_with_warnings_at_root`]。
    /// 显式主根(`--project-root` / `KANZEI_PROJECT_ROOT`)的调用方请直接用 at_root 版本,
    /// 别在这里绕一圈——cwd 一旦指向 worktree,发现出来的就是 worktree 自己的 `.kanzei` 副本。
    pub fn load_with_warnings(cwd: &Path) -> anyhow::Result<(KanzeiConfig, Vec<String>)> {
        let root = discover_project_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
        Self::load_with_warnings_at_root(&root)
    }

    /// 显式主根加载:直接叠加全局 `kanzei_home()/kanzei.toml` 与
    /// `project_root/.kanzei/kanzei.toml`,**不做任何根发现**。
    ///
    /// 与 [`Self::load`] 的区别只有一条:根从哪来。这里的根是调用方说了算的,
    /// 因此在 worktree 里也能读到主根那份配置(D-267:worktree 里 `.kanzei` 是
    /// 被 git checkout 出来的**分支副本**,发现式取根会读到过期的那一份)。
    pub fn load_at_root(project_root: &Path) -> anyhow::Result<KanzeiConfig> {
        let (config, warnings) = Self::load_with_warnings_at_root(project_root)?;
        for warning in &warnings {
            tracing::warn!("{warning}");
        }
        Ok(config)
    }

    /// 同 [`Self::load_at_root`],但把未知字段告警交给调用方展示。
    pub fn load_with_warnings_at_root(
        project_root: &Path,
    ) -> anyhow::Result<(KanzeiConfig, Vec<String>)> {
        let mut config = KanzeiConfig::default();
        let mut warnings = Vec::new();
        if let Some(home) = crate::home::kanzei_home() {
            merge_file(&mut config, &home.join("kanzei.toml"), &mut warnings)?;
        }
        merge_file(
            &mut config,
            &project_root.join(".kanzei").join("kanzei.toml"),
            &mut warnings,
        )?;
        config.fill_defaults();
        // 三处接线之三:非交互策略键认不出来时的 fail-closed 告警。要在**层叠之后**打,
        // 因为最终生效的是合并结果——全局写对了、项目层写错了,该报的是项目层那个值。
        warnings.extend(config.non_interactive_policy_warning());
        Ok((config, warnings))
    }

    /// 找出升级到结构化 bash 资源前遗留的裸命令规则；只读，不修改配置。
    pub fn legacy_bash_rules(&self) -> Vec<&Rule> {
        self.permissions
            .rules
            .iter()
            .filter(|rule| {
                rule.action == BASH_ACTION && !is_structured_bash_resource(&rule.resource)
            })
            .collect()
    }

    /// 裸 bash 规则中需要降级为逐次询问的规则；显式 `bash/*` 放行另行提示。
    /// deny 规则是仍然生效的护栏,不该被算作"将逐次询问"(D-139)。
    pub fn legacy_bash_rules_needing_downgrade(&self) -> Vec<&Rule> {
        self.legacy_bash_rules()
            .into_iter()
            .filter(|rule| {
                !is_wildcard_resource(&rule.resource)
                    && rule.effect != crate::permission::Effect::Deny
            })
            .collect()
    }

    /// **结构化**但无法证明「没被路径规范化改写过」的 bash 规则(D-269 收敛路径)。
    ///
    /// 背景:修复前 drive.rs 落盘规则时对 bash 资源也跑了 `normalize_resource`,而那是**路径**
    /// 语义——`\`→`/`、折 `//` 与 `/./`、弹 `..` 前一段、Windows 整串小写。于是已落盘的结构化
    /// 规则分成三类:
    /// - 命令里有 `\`(JSON 里的 `\"` 转义)→ 变成 `/"`,整串**不再是合法 JSON**,
    ///   [`Self::legacy_bash_rules`] 已经把它们算进去了;
    /// - 命令里有大写(`Get-Content`、`-SkipTests`)→ 被整串小写,**仍是合法 JSON**,于是
    ///   `legacy_bash_rules` 一条都认不出来:用户零告警、零指引,只看到命令莫名其妙又开始逐次询问。
    ///   本函数补的就是这一类。
    /// - 本来就全小写、也没有会被折叠的分隔符 → 规范化是恒等,规则今天仍然命中。
    ///
    /// **判据只能过宽,这是可证明的**:落盘的是 `P = N(J)`,而 `N` 幂等,所以 `N(P) == P` 对
    /// **每一条**历史规则都成立——单看 `P` 反推 `J` 就是求非单射函数的原像,答案是一个类不是一个点
    /// (与 D-269 拒绝"反解迁移"是同一个理由)。因此这里取 `N(P) == P` 作判据:
    /// - **零漏报**:凡被改写过的规则必然满足它,一条都不会漏;
    /// - **会多报**:天生全小写的规则(`git status --short`)也满足它,而那种规则其实还活着。
    ///
    /// 多报是可接受的,因为告警给出的动作是**有条件的**——"下次这条命令又来问你时重新授权一次"。
    /// 规则还活着的用户永远等不到那个询问,也就不需要做任何事。
    ///
    /// 唯一能**证明**没被改写的一类要排掉:整串一个分隔符都没有。`normalize_resource`
    /// 对无分隔符的串直接原样返回(连 Windows 小写那步都走不到),所以 `J` 无分隔符 ⇒
    /// `P = N(J) = J` ⇒ 规则今天照样命中。这类不点名。
    pub fn structured_bash_rules_possibly_stale(&self) -> Vec<&Rule> {
        self.permissions
            .rules
            .iter()
            .filter(|rule| {
                rule.action == BASH_ACTION
                    && rule.effect != crate::permission::Effect::Deny
                    && !is_wildcard_resource(&rule.resource)
                    && is_structured_bash_resource(&rule.resource)
                    && (rule.resource.contains('/') || rule.resource.contains('\\'))
                    && normalize_resource(&rule.resource) == rule.resource
            })
            .collect()
    }

    /// 无 TTY 时的 Ask 处置策略。缺键、空串、无法识别的取值一律 **fail-closed 回落
    /// [`NonInteractive::Deny`]** —— 也就是今天的行为,旧配置逐字节不变。
    pub fn non_interactive_policy(&self) -> NonInteractive {
        self.permissions
            .non_interactive
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .and_then(NonInteractive::parse)
            .unwrap_or_default()
    }

    /// 非交互策略键写了但认不出来时的告警。**必须有**:悄悄回落到 deny 而不吭声,
    /// 用户会以为自己已经开了 rules_only,实际每次都停机,还归不到因。
    pub fn non_interactive_policy_warning(&self) -> Option<String> {
        let raw = self.permissions.non_interactive.as_deref()?;
        if raw.trim().is_empty() || NonInteractive::parse(raw).is_some() {
            return None;
        }
        Some(format!(
            "permissions.non_interactive = `{raw}` 无法识别，已 fail-closed 回落到 `deny`(无 TTY 时遇到询问即停机)；\
             可用取值：{}。",
            NonInteractive::known_names()
        ))
    }

    /// 显式 `bash/* = allow` 保持全量放行语义，启动时必须明确告知用户。
    pub fn explicit_bash_wildcard_allows(&self) -> Vec<&Rule> {
        self.permissions
            .rules
            .iter()
            .filter(|rule| {
                rule.action == BASH_ACTION
                    && is_wildcard_resource(&rule.resource)
                    && rule.effect == crate::permission::Effect::Allow
            })
            .collect()
    }

    /// 启动告警(D-139):文案必须由**实际评估结果**推导,而不是按规则形态猜。
    ///
    /// 原实现按规则形态分别计数各说各话:legacy 规则与显式 `bash/*` 并存时
    /// last-match-wins 让一切直接放行,告警却照样说"将逐次询问"——在安全边界上
    /// 给出错误告知。现在先用代表性命令跑一遍 Ruleset::evaluate,以真实判定为准。
    ///
    /// **yolo 判据修正(F8 ①,D-139 以新形态复发的必修点)**:光看"探针评估成 Allow"
    /// 推不出"全量放行"。探针是一条**具体**命令,用户完全可能只授权过它自己——那时
    /// 告警会把"你授权过 git status"说成"你把 bash 全放开了",又是一条假话。
    /// 改成两个条件同时成立才敢说 yolo:探针 Allow **且** 确实存在显式 `bash/*` 放行规则。
    /// 探针 Allow 但没有 `*` 规则时,如实说"是这条探针命令被某条规则直接命中,其余仍会询问"。
    pub fn bash_permission_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let mut ruleset = crate::permission::Ruleset::default();
        for rule in &self.permissions.rules {
            ruleset.push(rule.clone());
        }
        // 代表性命令:一条普通只读命令即可探明"默认会不会问"。
        let probe = serde_json::json!({ "command": "git status", "workdir": "." }).to_string();
        let probe_allowed =
            ruleset.evaluate(BASH_ACTION, &probe) == crate::permission::Effect::Allow;

        let legacy = self.legacy_bash_rules_needing_downgrade();
        let stale = self.structured_bash_rules_possibly_stale();
        let wildcard_count = self.explicit_bash_wildcard_allows().len();

        if probe_allowed && wildcard_count > 0 {
            // 无论有多少条 legacy 规则,实际结果就是全量放行——必须如实说。
            warnings.push(format!(
                "检测到 bash 权限最终判定为全量放行(yolo)，来自 {wildcard_count} 条显式 bash/* 放行规则；\
                 不会再逐次询问，请确认这是有意设置。"
            ));
            if !legacy.is_empty() {
                warnings.push(format!(
                    "另有 {} 条旧 bash 规则被上述放行覆盖(last-match-wins)，实际不生效。",
                    legacy.len()
                ));
            }
            return warnings;
        }
        if probe_allowed {
            // 探针命中了某条具体规则,但没有整体放行规则——说清范围,别冒充 yolo。
            warnings.push(
                "探针命令 `git status` 已被某条已有 bash 规则直接放行，但配置里没有整体 bash/* 放行规则；\
                 其余命令仍会逐次询问。"
                    .to_string(),
            );
        }
        // D-269 收敛路径:两类失效各说各的,并且都给**可执行的动作**,不是"请重新选择作用域"
        // 这种没有落点的话。用户看到的症状是同一句"命令又开始问了",指引必须能直接照做。
        if !legacy.is_empty() {
            warnings.push(format!(
                "检测到 {} 条旧 bash 权限规则(升级前的裸命令形态，如 {})：\
                 它们对今天的结构化请求恒不命中，等于已经失效。\
                 这些命令会重新逐次询问，遇到时按一次「总是允许」就好，新规则会覆盖旧的；\
                 旧条目留在配置里不影响判定，想清爽可以自行删掉。",
                legacy.len(),
                rule_digest(&legacy)
            ));
        }
        if !stale.is_empty() {
            warnings.push(format!(
                "另有 {} 条结构化 bash 规则可能是修复前写下的(如 {})：\
                 那时命令文本会被路径规范化整串小写(`Get-Content` → `get-content`)，\
                 改写过的规则再也对不上原命令。无法从改写后的文本反推原文，所以这里只能按最宽的判据点名，\
                 其中确实还有效的那些你不会遇到询问；\
                 真遇到某条命令又开始问，同样按一次「总是允许」即可。",
                stale.len(),
                rule_digest(&stale)
            ));
        }
        warnings
    }

    pub fn fill_defaults(&mut self) {
        self.providers
            .entry("anthropic".into())
            .or_insert(ProviderConfig {
                protocol: "anthropic".into(),
                base_url: "https://api.anthropic.com".into(),
                api_key_env: Some("ANTHROPIC_API_KEY".into()),
                api_key: None,
                auth: None,
                context_limit: Some(200_000),
            });
        self.providers
            .entry("ollama".into())
            .or_insert(ProviderConfig {
                protocol: "openai".into(),
                base_url: "http://127.0.0.1:11434/v1".into(),
                api_key_env: None,
                api_key: None,
                auth: None,
                context_limit: Some(32_000),
            });
        // Codex 订阅通道:复用 Codex CLI 登录态,零配置可用。
        self.providers
            .entry("codex".into())
            .or_insert(ProviderConfig {
                protocol: "openai-responses".into(),
                base_url: "https://chatgpt.com/backend-api/codex".into(),
                api_key_env: None,
                api_key: None,
                auth: Some("codex".into()),
                context_limit: Some(272_000),
            });
        // Claude Code 订阅通道:复用 Claude Code 登录态,零配置可用。
        self.providers
            .entry("claude".into())
            .or_insert(ProviderConfig {
                protocol: "anthropic".into(),
                base_url: "https://api.anthropic.com".into(),
                api_key_env: None,
                api_key: None,
                auth: Some("claude".into()),
                context_limit: Some(200_000),
            });
        // DeepSeek 直连(OpenAI 兼容)。
        self.providers
            .entry("deepseek".into())
            .or_insert(ProviderConfig {
                protocol: "openai".into(),
                base_url: "https://api.deepseek.com".into(),
                api_key_env: Some("DEEPSEEK_API_KEY".into()),
                api_key: None,
                auth: None,
                context_limit: Some(1_000_000),
            });
        // context_limit 逐字段兜底(D-173)。
        //
        // 上面全是 `entry().or_insert()`:用户只要在 kanzei.toml 里写了同名 provider,
        // 整条默认就不再生效,**单个缺失字段也补不上**。实测后果是用户配置里的
        // deepseek/codex/anthropic 全都没有 context_limit,于是 UI 的占用比例、
        // 运行器的压缩预检统统失去基准,只能等 provider 报 overflow 才被动补救。
        // 这里按 provider 名与 base_url 主机名回填一个已知值;用户显式写了就不动。
        for (name, provider) in self.providers.iter_mut() {
            if provider.context_limit.is_some() {
                continue;
            }
            let host = provider.base_url.to_ascii_lowercase();
            provider.context_limit = known_context_limit(name, &host);
        }
        if self.models.primary.is_none() {
            self.models.primary = Some("codex:gpt-5.6-luna".into());
        }
        if self.models.fast.is_none() {
            self.models.fast = Some("ollama:qwen3.5:4b".into());
        }
        if self.models.codex_fast_mode.is_none()
            && self.models.primary.as_deref() == Some("codex:gpt-5.6-luna")
        {
            self.models.codex_fast_mode = Some(true);
        }
    }

    /// "primary"/"fast"(角色)或 "provider:model"(直指)→ ResolvedModel。
    /// Codex Fast mode 的唯一判据:开关打开 **且** 本次真正解析到的供应商是 codex。
    /// 判据只有这一份——此前 11 个 RunnerConfig 构造点里 9 个硬写 None,于是"用 codex
    /// 就该生效"在子代理、压缩纪要、记忆整理、文件标注这些路径上全部失效(用户实测提出)。
    /// 任何新增的构造点都必须调它,不要再就地抄条件。
    pub fn service_tier_for(&self, resolved: &ResolvedModel) -> Option<String> {
        (self.models.codex_fast_mode.unwrap_or(false)
            && resolved.provider.auth.as_deref() == Some("codex"))
        .then(|| "priority".to_string())
    }

    pub fn resolve_model(&self, reference: &str) -> anyhow::Result<ResolvedModel> {
        let spec = match reference {
            "primary" => self.models.primary.as_deref().unwrap_or_default(),
            "fast" => self.models.fast.as_deref().unwrap_or_default(),
            direct => direct,
        };
        let (provider_name, model) = spec.split_once(':').ok_or_else(|| {
            anyhow::anyhow!(
                "model reference `{spec}` must be `provider:model` (from `{reference}`)"
            )
        })?;
        let provider = self.providers.get(provider_name).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown provider `{provider_name}`; configured: {}",
                self.providers
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        Ok(ResolvedModel {
            provider_name: provider_name.to_string(),
            provider: provider.clone(),
            model: model.to_string(),
        })
    }

    pub fn default_profile(&self) -> crate::defs::ProfileKind {
        self.profile
            .default
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(crate::defs::ProfileKind::Dev)
    }
}

/// fill_defaults 无条件回填的内置 provider 名单(R-184 P6 / D-246)。
/// 与 fill_defaults 中的五个 `entry().or_insert()` 保持同步;UI 据此把内置
/// provider 的删除入口换成「内置」标记,避免「删了重开又回来」的误导。
/// 这是只读元数据,不参与配置解析。
pub fn builtin_provider_names() -> &'static [&'static str] {
    &["anthropic", "ollama", "codex", "claude", "deepseek"]
}

/// 内置 provider 的出厂 `context_limit`(取自 fill_defaults 本身,不另立名单——
/// 名单漂移比没有名单更糟,见 D-246)。
///
/// D-288:设置页保存时把**每个** provider 的 context_limit 都写进用户 toml,于是
/// 一次「保存」就把当时的出厂默认冻成了用户配置。deepseek 的出厂值后来从 128k
/// 改成 1M,用户那份 toml 却永远停在 128000——`fill_defaults` 只补 `None`,不会
/// 覆盖已有值,所以内置默认再怎么改都追不上。设置页据此判断「这个数只是出厂默认」,
/// 相同就不落盘,留空即跟随内置。
pub fn builtin_context_limit(name: &str) -> Option<u64> {
    let mut config = KanzeiConfig::default();
    config.providers.clear();
    config.fill_defaults();
    config.providers.get(name).and_then(|p| p.context_limit)
}

fn merge_file(
    config: &mut KanzeiConfig,
    path: &Path,
    warnings: &mut Vec<String>,
) -> anyhow::Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
    let layer: KanzeiConfig = toml::from_str(&text)
        .map_err(|e| anyhow::anyhow!("invalid config {}: {e}", path.display()))?;
    // 未知键宽容忽略但必须可见:拼错的键静默失效比报错更难排查。
    if let Ok(raw) = toml::from_str::<toml::Value>(&text) {
        for key in unknown_keys(&raw) {
            warnings.push(format!(
                "{}: 未知配置项 `{key}` 已忽略(可能是拼写错误,或来自更新版本的 kanzei)",
                path.display()
            ));
        }
        // D-245:cadence 层叠。字段非 Option,必须用 raw 表显式键驱动 overlay,
        // 否则「项目层只写一个键」会把其余字段打回默认(见 overlay_cadence)。
        let cadence_written: std::collections::HashSet<&str> = raw
            .get("cadence")
            .and_then(|v| v.as_table())
            .map(|table| table.keys().map(String::as_str).collect())
            .unwrap_or_default();
        if !cadence_written.is_empty() {
            overlay_cadence(&mut config.cadence, &layer.cadence, &cadence_written);
        }
    }
    merge(config, layer);
    Ok(())
}

/// D-245:cadence 逐键 overlay。字段非 Option,「没写」与「显式写成默认值」在
/// merge 层不可区分(serde default 把两者都落成默认),所以由调用方把 raw toml
/// `[cadence]` 表里**显式出现的键**传进来,只覆盖这些——与 [limits] 的
/// 「项目层只覆盖显式写的键」同一套层叠语义,避免项目层只调一个 full_test
/// 就把其余字段全部打回默认。
fn overlay_cadence(base: &mut Cadence, layer: &Cadence, written: &std::collections::HashSet<&str>) {
    if written.contains("full_test") {
        base.full_test = layer.full_test;
    }
    if written.contains("full_test_batches") {
        base.full_test_batches = layer.full_test_batches;
    }
    if written.contains("targeted_test") {
        base.targeted_test = layer.targeted_test;
    }
    if written.contains("commit") {
        base.commit = layer.commit;
    }
    if written.contains("push") {
        base.push = layer.push;
    }
}

/// `*` 通配资源判定:全仓统一按 trim 后比较,避免两处判定不一致(D-139)。
fn is_wildcard_resource(resource: &str) -> bool {
    resource.trim() == "*"
}

/// 告警里点名规则用的摘要:取前两条的命令文本,各截 40 个**字符**(不是字节——
/// 命令里有中文,按字节切会在多字节中间断开而 panic),其余折成"等 N 条"。
fn rule_digest(rules: &[&Rule]) -> String {
    fn 命令(resource: &str) -> String {
        let text = serde_json::from_str::<serde_json::Value>(resource)
            .ok()
            .and_then(|json| json.get("command")?.as_str().map(str::to_string))
            .unwrap_or_else(|| resource.to_string());
        let mut 头: String = text.chars().take(40).collect();
        if text.chars().count() > 40 {
            头.push('…');
        }
        头
    }
    let 前两条: Vec<String> = rules.iter().take(2).map(|r| 命令(&r.resource)).collect();
    let mut out = 前两条
        .iter()
        .map(|c| format!("`{c}`"))
        .collect::<Vec<_>>()
        .join("、");
    if rules.len() > 前两条.len() {
        out.push_str(&format!(" 等，共 {} 条", rules.len()));
    }
    out
}

/// 列出 schema 未识别的键路径。schema 变更时同步维护;
/// `unknown_keys_schema_matches_struct` 测试守护两者不漂移。
fn unknown_keys(value: &toml::Value) -> Vec<String> {
    fn check(table: &toml::Value, path: &str, known: &[&str], out: &mut Vec<String>) {
        let Some(table) = table.as_table() else {
            return;
        };
        for key in table.keys() {
            if !known.contains(&key.as_str()) {
                out.push(if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                });
            }
        }
    }
    let mut out = Vec::new();
    check(
        value,
        "",
        &[
            "models",
            "providers",
            "proxy",
            "profile",
            "permissions",
            "limits",
            "cadence",
            "embeddings",
        ],
        &mut out,
    );
    if let Some(models) = value.get("models") {
        check(
            models,
            "models",
            &["primary", "fast", "reasoning", "codex_fast_mode", "scout"],
            &mut out,
        );
    }
    if let Some(embeddings) = value.get("embeddings") {
        check(embeddings, "embeddings", &["provider", "model"], &mut out);
    }
    if let Some(limits) = value.get("limits") {
        check(
            limits,
            "limits",
            &[
                "max_tokens",
                "subagent_max_tokens",
                "subagent_timeout_secs",
                "context_budget_ratio",
                "recent_verbatim_ratio",
                "max_tasks_per_turn",
                "max_parallel_tools",
                "transport_retries",
                "rate_limit_retries",
                "stream_restarts",
            ],
            &mut out,
        );
    }
    if let Some(providers) = value.get("providers").and_then(|p| p.as_table()) {
        for (name, provider) in providers {
            check(
                provider,
                &format!("providers.{name}"),
                &[
                    "protocol",
                    "base_url",
                    "api_key_env",
                    "api_key",
                    "auth",
                    "context_limit",
                ],
                &mut out,
            );
        }
    }
    if let Some(profile) = value.get("profile") {
        check(profile, "profile", &["default"], &mut out);
    }
    if let Some(cadence) = value.get("cadence") {
        check(
            cadence,
            "cadence",
            &[
                "full_test",
                "full_test_batches",
                "targeted_test",
                "commit",
                "push",
            ],
            &mut out,
        );
    }
    if let Some(permissions) = value.get("permissions") {
        // 三处接线之一:新键要进这份已知键清单,否则用户一写就收到"未知配置项"假告警。
        check(
            permissions,
            "permissions",
            &["rules", "non_interactive"],
            &mut out,
        );
        if let Some(rules) = permissions.get("rules").and_then(|r| r.as_array()) {
            for (index, rule) in rules.iter().enumerate() {
                check(
                    rule,
                    &format!("permissions.rules[{index}]"),
                    &["action", "resource", "effect"],
                    &mut out,
                );
            }
        }
    }
    out
}

/// 标量覆盖、map 合并、规则追加(后层排后 → last-match-wins 自然让后层优先)。
fn merge(base: &mut KanzeiConfig, layer: KanzeiConfig) {
    if layer.models.primary.is_some() {
        base.models.primary = layer.models.primary;
    }
    if layer.models.fast.is_some() {
        base.models.fast = layer.models.fast;
    }
    // reasoning 曾被漏掉:项目级设的思考强度会被静默忽略,而 primary/fast 却生效,
    // 表现为"同一份配置里有的键管用有的不管用"——最难查的那种。
    if layer.models.reasoning.is_some() {
        base.models.reasoning = layer.models.reasoning;
    }
    if layer.models.codex_fast_mode.is_some() {
        base.models.codex_fast_mode = layer.models.codex_fast_mode;
    }
    if layer.models.scout.is_some() {
        base.models.scout = layer.models.scout;
    }
    base.providers.extend(layer.providers);
    if layer.proxy.is_some() {
        base.proxy = layer.proxy;
    }
    if layer.profile.default.is_some() {
        base.profile.default = layer.profile.default;
    }
    base.permissions.rules.extend(layer.permissions.rules);
    // 三处接线之二:标量键要按 [limits] 的同一套规矩做 overlay —— 项目层写了才覆盖,
    // 没写就保持全局层的值。只加字段不接这一行,项目层设了会静默不生效
    // (`Limits::barrier_timeout_secs` 就是这么漏的)。
    if layer.permissions.non_interactive.is_some() {
        base.permissions.non_interactive = layer.permissions.non_interactive;
    }
    // [limits] 逐字段覆盖:项目层只写了哪几个键就只覆盖哪几个,没写的保持全局层的值。
    // 整节替换会让"项目里只调一个 max_tokens"把其余全部打回默认——正是 reasoning
    // 那次漏合并留下的教训。
    macro_rules! overlay {
        ($($field:ident),+ $(,)?) => {
            $(if layer.limits.$field.is_some() { base.limits.$field = layer.limits.$field; })+
        };
    }
    overlay!(
        max_tokens,
        subagent_max_tokens,
        subagent_timeout_secs,
        context_budget_ratio,
        recent_verbatim_ratio,
        max_tasks_per_turn,
        max_parallel_tools,
        transport_retries,
        rate_limit_retries,
        stream_restarts,
    );
    // [embeddings] 逐字段覆盖(与 [limits] 同规:项目层只覆盖写了的那几个键)。
    if layer.embeddings.provider.is_some() {
        base.embeddings.provider = layer.embeddings.provider;
    }
    if layer.embeddings.model.is_some() {
        base.embeddings.model = layer.embeddings.model;
    }
}

/// "总是允许"的持久化:向项目配置追加 allow 规则(后来的规则 last-match-wins)。
/// 文本级追加(D-083):toml_edit 保留注释、排版与未知字段,不做整文件 round-trip。
pub fn append_allow_rule(
    project_root: &Path,
    action: &str,
    resource: &str,
) -> anyhow::Result<PathBuf> {
    let path = project_root.join(".kanzei").join("kanzei.toml");
    let text = if path.is_file() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };
    // 语义预检:类型错误在这里明确报出,而不是把规则追进一个坏文件。
    toml::from_str::<KanzeiConfig>(&text)
        .map_err(|e| anyhow::anyhow!("invalid {}: {e}", path.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid {}: {e}", path.display()))?;
    let permissions = doc.entry("permissions").or_insert(toml_edit::table());
    let Some(permissions) = permissions.as_table_mut() else {
        anyhow::bail!("{}: `permissions` 不是表,无法追加规则", path.display());
    };
    permissions.set_implicit(true);
    let rules = permissions
        .entry("rules")
        .or_insert(toml_edit::Item::ArrayOfTables(
            toml_edit::ArrayOfTables::new(),
        ));
    let Some(rules) = rules.as_array_of_tables_mut() else {
        anyhow::bail!(
            "{}: `permissions.rules` 不是数组表,无法追加规则",
            path.display()
        );
    };
    let mut rule = toml_edit::Table::new();
    rule.insert("action", toml_edit::value(action));
    rule.insert("resource", toml_edit::value(resource));
    rule.insert("effect", toml_edit::value("allow"));
    rules.push(rule);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, doc.to_string())?;
    Ok(path)
}

/// "总是允许"时保留具体资源，避免把一个命令的授权扩大为首词通配。
/// bash 的 shell/解释器语义无法靠首词推断安全边界；调用方仍可对
/// 用户明确配置的整体 `*` 使用 yolo 语义。
pub fn generalize_resource(action: &str, resource: &str) -> String {
    let _ = action;
    resource.to_string()
}

/// 从 cwd 向上找 `.kanzei/kanzei.toml`。
pub fn discover_project_config(cwd: &Path) -> Option<PathBuf> {
    discover_project_root(cwd).map(|root| root.join(".kanzei").join("kanzei.toml"))
}

/// R-178 P2:五层模型解析链的前三层(引用层)合并——本轮直选 → 线持久选择 → agent 默认。
///
/// 五层链(design §3.4):
/// ① 本轮直选(`run_prompt` 的 `model` 参数 / CLI `KANZEI_MODEL`);
/// ② 线/进程持久选择(`ProcessHandle.model`,state.db `processes` 表,重启后恢复);
/// ③ agent 定义的默认引用(通常 `"primary"` / `"fast"` 角色);
/// ④ 项目 `[models]` / 全局 `[models]` 层叠——由 `KanzeiConfig::load_with_warnings`
///    (全局 merge → 项目覆盖)在 `config.models` 里完成;
/// ⑤ 内置默认——由 `fill_defaults()` 兜底。
///
/// 本函数只负责 ①②③ 的**引用合并**:返回最终要交给 `resolve_model` 的模型引用串。
/// ④⑤ 是 `resolve_model` 内部的事(角色 → provider:model → [providers]),
/// 本函数不掺和。CLI 与桌面共用这一份,保证「同一真源」(验收②)。
///
/// 空串 / 纯空白视为「未设」,逐层回落。
pub fn resolve_model_chain(
    run_override: Option<&str>,
    process_model: Option<&str>,
    agent_model: &str,
) -> String {
    run_override
        .filter(|value| !value.trim().is_empty())
        .or_else(|| process_model.filter(|value| !value.trim().is_empty()))
        .map(str::to_string)
        .unwrap_or_else(|| agent_model.to_string())
}

/// 项目根 = 向上**最近**的含 `.kanzei/` 或 `.git/` 的目录;都没有则 cwd 本身。
///
/// 两条约束都是踩出来的,别再退回去:
/// ① `.kanzei` 不许无视距离压过 `.git`。原实现撞到任何 `.kanzei` 就立即返回,`.git`
///    只记 fallback 且要等循环走完才用,于是 `~/Documents/某仓库`(有 .git、没 .kanzei)
///    会一路走到 HOME,仓库自己的 `.git` 被丢掉。
/// ② HOME 自己的 `.kanzei` 不算项目标记——它是**全局**配置根(kanzei.toml、memory、
///    app.json),必然存在,于是成了 HOME 下所有无标记目录的磁铁。实测后果:`~/.kanzei`
///    里已经躺着 `project/` 与 `state.db` 这类只该出现在项目里的产物。
///    HOME 的 `.git`(dotfiles 仓库)仍然算标记,那是货真价实的仓库。
pub fn discover_project_root(cwd: &Path) -> Option<PathBuf> {
    discover_project_root_with_home(cwd, dirs::home_dir().as_deref())
}

/// 显式主根优先于发现式取根:参数 > 环境变量 > 发现式(现状)。
///
/// R-182 内容②。`explicit` 由调用方按「`--project-root` 参数 > `KANZEI_PROJECT_ROOT`
/// 环境变量」合成后传进来;为 `None` 时逐字节退回今天的发现式行为
/// (`discover_project_root(cwd)`,兜底 cwd 本身)。
///
/// 实测背景(D-267):两棵 worktree 相隔 10 秒各跑一次 `kz defect add`,**都拿到 D-267**——
/// `.kanzei/project/*.md` 被 git 跟踪,`git worktree add` 把它们 checkout 成分支副本,
/// 发现式取根在 worktree 里第一层就命中那份副本,两条线各自在自己的副本上分配编号。
/// 显式主根就是这条路的出口。
///
/// **两个根是正交的两件事,别再混**(D-187 的教训):
/// - `KANZEI_PROJECT_ROOT` 改的是**项目根**——`.kanzei/project/*.md`、state.db、项目记忆;
/// - `KANZEI_HOME` 改的是**全局根**——`~/.kanzei/kanzei.toml`、全局记忆、app.json。
///   设了其中一个不会影响另一个。
///
/// **不做 canonicalize**:与 run.rs 同源的理由——Windows 上 `canonicalize` 产出
/// `\\?\C:\…` 形态,用户已经写下的绝对路径权限规则会一夜之间集体失配。
pub fn resolve_project_root(explicit: Option<&Path>, cwd: &Path) -> anyhow::Result<PathBuf> {
    let Some(explicit) = explicit else {
        return Ok(discover_project_root(cwd).unwrap_or_else(|| cwd.to_path_buf()));
    };
    if !explicit.exists() {
        anyhow::bail!(
            "显式主根(--project-root / KANZEI_PROJECT_ROOT)指向的路径不存在: {}",
            explicit.display()
        );
    }
    if !explicit.is_dir() {
        anyhow::bail!(
            "显式主根(--project-root / KANZEI_PROJECT_ROOT)不是目录: {}",
            explicit.display()
        );
    }
    // worktree 的 `.git` 是**文件**不是目录,所以这里目录/文件都算标记;
    // `.kanzei` 则必须是目录(托管文档挂在它下面)。
    let has_marker = explicit.join(".kanzei").is_dir() || explicit.join(".git").exists();
    if !has_marker {
        anyhow::bail!(
            "显式主根(--project-root / KANZEI_PROJECT_ROOT)不像项目根:{} 下既没有 .kanzei 目录也没有 .git。\n\
             主根是放 .kanzei/project/*.md 与 state.db 的那个目录;确实想把它当项目,就先 mkdir .kanzei。",
            explicit.display()
        );
    }
    Ok(explicit.to_path_buf())
}

/// 目录比较用的形态:剥 Windows 扩展长度前缀、统一分隔符、折叠 `.` / `..`、去尾分隔符,
/// Windows 上再小写。
///
/// 裸 `==` 比较不够(D-194):`dirs::home_dir()` 给 `C:\Users\kanzei`,而走上来的祖先
/// 可能是 `c:\users\kanzei`(shell 里键入的大小写)或 `\\?\C:\Users\kanzei`(canonicalize
/// 的产物)——任一形态对不上,HOME 判断就静默失效,`~/.kanzei` 立刻变回项目根磁铁。
/// 同一个坑 kanzei-core 的 `session_identity` 已经踩过一次(同一项目裂成两条会话线)。
/// 这里是纯比较、不进哈希,所以可以比那边更狠:分隔符也一并统一。
///
/// **`.` / `..` 必须折叠**,而且这是 R-182 新入口打开的洞、不是历史遗留:根从
/// `current_dir()` 来的时候不可能带 `.`/`..` 段,`--project-root` / `KANZEI_PROJECT_ROOT`
/// 收的却是用户任意书写的路径串。`C:\Users\kanzei\.` 与 `C:\Users\kanzei\Documents\..`
/// 在文件系统看来就是 HOME,`resolve_project_root` 的标记校验对它们照样成立
/// (HOME 下有 `.kanzei`),于是两道拦截**全部静默通过**,project 级 state.db 被写进
/// `~/.kanzei`——实测发生过。
///
/// 折叠**复用 `permission::normalize_resource`**(权限决策的同一份实现,D-050),不另写
/// 第二份:两份词法折叠一旦漂移,就是"权限那边算出来的路径"和"取根这边算出来的路径"
/// 指向两个地方。
///
/// **本函数自身仍然是纯词法的**:不碰文件系统、不解符号链接,也不会把用户写的裸路径
/// 变成 `\\?\` 形态。词法折叠单独用赢不了别名(见 [`is_same_dir`]),但它是**永远可用**
/// 的那一层:路径不存在或读不到时 `canonicalize` 给不出身份,这里的结论就是最后的兜底。
fn dir_key(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let stripped = raw
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| raw.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or_else(|| raw.to_string());
    // 分隔符与大小写只在 Windows 上等价;Linux 下 `C:` 与 `c:` 是两个目录,
    // 归一过头会把不同路径判成同一个。
    #[cfg(windows)]
    let unified = stripped.replace('/', "\\").to_lowercase();
    #[cfg(not(windows))]
    let unified = stripped;
    let key = normalize_resource(&unified);
    key.trim_end_matches(['\\', '/']).to_string()
}

/// 两个路径串指的是不是**同一个目录**。
///
/// D-194 补漏二:**词法规则补不完**,别再往 [`dir_key`] 里加规则了。实测(Windows 11)
/// 下面这些写法在磁盘上就是同一个目录,而纯词法折叠一条都认不出来:
/// - `C:\Users\kanzei.` —— Windows 剥掉末段的尾随点;`kanzei.` 对折叠来说是个普通段名,
///   既不是 `.` 也不是 `..`,于是 `c:/users/kanzei.` ≠ `c:/users/kanzei`。
/// - `\\localhost\C$\Users\kanzei` / `\\127.0.0.1\C$\Users\kanzei` —— 归一成
///   `//localhost/c$/users/kanzei`,与盘符形态永不相等。这**不纯是对抗构造**:网络/漫游
///   profile 下 UNC 本来就是合法写法。
/// - 符号链接、junction、`subst` 虚拟盘、8.3 短名 —— 凡「词法不同、文件系统同一」的别名,
///   再补多少条词法规则也补不完。
///
/// 所以改用**文件系统身份**:两侧各做一次 `std::fs::canonicalize`。实测它把 junction、
/// 8.3 短名、`subst` 虚拟盘、尾随点、大小写、`\\?\` 前缀一律解成同一个 `\\?\C:\…` 形态。
///
/// **这与「显式主根不做 canonicalize」不矛盾——两条说的是不同的事。**
/// 那条顾虑(见 [`resolve_project_root`] 与测试 `显式主根不做canonicalize`)反对的是把
/// canonicalize 的产物**当作项目根存下去 / 传下去**:`\\?\` 形态会让用户已经写在配置里的
/// 绝对路径权限规则集体失配。而这里的 canonicalize **只活在本次相等判断内部**——不返回、
/// 不存储、不进哈希、不传给任何下游;`resolve_project_root` 返回的仍是用户写下的原串,
/// 下游看到的形态一个字节没变(那条测试原样保留,正是用来钉住存储/传播侧没变的)。
///
/// canonicalize 失败(路径不存在、无读权限)时**回落到词法折叠**——拿不到身份不等于放行。
fn is_same_dir(a: &Path, b: &Path) -> bool {
    // ① 词法层永远先跑:它不需要路径存在,也是 canonicalize 失败时的兜底。
    //    只加不减——加上身份层之后,今天能认出来的写法一条都不会变得认不出来。
    if dir_key(a) == dir_key(b) {
        return true;
    }
    let (Ok(ca), Ok(cb)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) else {
        return false;
    };
    // ② 身份层:同一命名空间内 canonicalize 就是唯一名字,别名到这里全部坍缩。
    if dir_key(&ca) == dir_key(&cb) {
        return true;
    }
    // ③ 跨命名空间:canonicalize **不**把 UNC 归到盘符形态(实测
    //    `\\localhost\C$\Users\kanzei` → `\\?\UNC\localhost\C$\Users\kanzei`),所以 ② 在
    //    「一侧 UNC、一侧盘符」时必然判不等。只在这种情况下再走一层判据。
    if !is_unc_key(&dir_key(&ca)) && !is_unc_key(&dir_key(&cb)) {
        return false;
    }
    same_dir_by_volume_metadata(&ca, &cb)
}

/// [`dir_key`] 产出的 UNC 形态(`\\host\share\…` → `//host/share/…`)。
fn is_unc_key(key: &str) -> bool {
    key.starts_with("//")
}

/// 跨命名空间的同一性:比对目录**自身**的卷级元数据(创建时间 + 修改时间,均 100ns 精度)。
/// 这两个属性存在卷上,透过盘符还是透过 UNC 读到的是同一份——实测一致。
///
/// **诚实说明它不是句柄级身份**:句柄级身份要 `GetFileInformationByHandle` 的
/// `(volume_serial_number, file_index)`,std 里对应 `windows_by_handle`,**至今未稳定**
/// (rustc 1.97 实测 E0658),而本 crate 不引入 winapi 依赖去换这一处判据。
///
/// 判错方向是可陈述的:元数据相等 → 判成同一个目录 → **多拦一个 HOME**(更严,可见地报错);
/// 元数据不等 → 判成不同 → 退回 ② 的结论。因此误判只会偏保守,不会偏放行。
/// 唯一会偏放行的窗口是「两次读之间 HOME 自身的 mtime 被别的进程改掉」,所以读两轮:
/// 第一轮不等就整组重读一次,那个窗口需要连续两次都撞上才成立。
fn same_dir_by_volume_metadata(a: &Path, b: &Path) -> bool {
    fn fingerprint(path: &Path) -> Option<(Option<std::time::SystemTime>, std::time::SystemTime)> {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_dir() {
            return None;
        }
        Some((meta.created().ok(), meta.modified().ok()?))
    }
    for _ in 0..2 {
        let (Some(fa), Some(fb)) = (fingerprint(a), fingerprint(b)) else {
            return false;
        };
        if fa == fb {
            return true;
        }
    }
    false
}

/// 解析出的项目根是不是 HOME 本身。
///
/// D-189 让 HOME 的 `.kanzei` 不再把子目录吸上去,但在 HOME 里**直接**开跑这条路还通着:
/// 一路向上找不到任何标记时兜底返回 cwd,而 cwd 就是 HOME。此时项目级产物(state.db、
/// project/、memory/)会落进 `~/.kanzei`——那是全局配置根,两边数据就此混在一起
/// (D-186 的残留正是这么来的)。调用方拿这个判据在开跑前拦下来。
///
/// 相等判断委托 [`is_same_dir`](词法折叠 + 文件系统身份),别名形态在那里说明。
pub fn is_home_root(root: &Path) -> bool {
    dirs::home_dir().is_some_and(|home| is_same_dir(&home, root))
}

fn discover_project_root_with_home(cwd: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let home_key = home.map(dir_key);
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let is_home = home_key.as_ref().is_some_and(|h| *h == dir_key(d));
        if (!is_home && d.join(".kanzei").is_dir()) || d.join(".git").is_dir() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    Some(cwd.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_model_resolution() {
        let mut c = KanzeiConfig::default();
        c.fill_defaults();
        let m = c.resolve_model("primary").unwrap();
        assert_eq!(m.provider_name, "codex");
        assert_eq!(m.model, "gpt-5.6-luna");
        // 默认 primary 是 Luna 时,Codex Fast mode 默认开启(R-158)。
        assert_eq!(c.models.codex_fast_mode, Some(true));
        let claude = c.resolve_model("claude:claude-sonnet-4-6").unwrap();
        assert_eq!(claude.provider.protocol, "anthropic");
        assert_eq!(claude.provider.auth.as_deref(), Some("claude"));
        let m = c.resolve_model("fast").unwrap();
        assert_eq!(m.provider_name, "ollama");
        let m = c.resolve_model("ollama:llama3.3").unwrap();
        assert_eq!(m.model, "llama3.3");
        assert!(c.resolve_model("nope").is_err());
    }

    /// R-178 P2 五层链 ①②③:本轮直选 → 线持久选择 → agent 默认,逐层缺省回落。
    /// (④项目/全局 [models] 与 ⑤内置默认由 load_with_warnings + fill_defaults 承担,
    /// 见 merge_layers 与 defaults_and_model_resolution。)
    #[test]
    fn 五层链引用层逐层缺省回落() {
        // ① 本轮直选存在 → 用它(即使线持久与 agent 默认都设了)。
        assert_eq!(
            resolve_model_chain(Some("claude:claude-sonnet-4-6"), Some("a:b"), "primary"),
            "claude:claude-sonnet-4-6"
        );
        // ① 缺省 → ② 线持久选择。
        assert_eq!(resolve_model_chain(None, Some("a:b"), "primary"), "a:b");
        // ①② 都缺省 → ③ agent 默认。
        assert_eq!(resolve_model_chain(None, None, "primary"), "primary");
        // 空串视为未设,照样回落。
        assert_eq!(
            resolve_model_chain(Some("   "), Some("a:b"), "primary"),
            "a:b"
        );
        assert_eq!(
            resolve_model_chain(None, Some("  "), "fast"),
            "fast",
            "空白线持久值不得覆盖 agent 默认"
        );
    }

    /// R-173:勘察/复核路由可配,缺省沿用 fast(旧配置行为逐字节不变)。
    #[test]
    fn 勘察路由可配且缺省沿用fast() {
        // 旧配置没有这个键 → None,调用方回退 fast。
        let old: KanzeiConfig = toml::from_str("[models]\nprimary = \"a:b\"\n").unwrap();
        assert_eq!(old.models.scout, None, "缺省必须是 None,不能替用户拍板");

        // 显式配置能解析成真实模型(与 primary/fast 同一套解析,没有第二套)。
        let mut c: KanzeiConfig =
            toml::from_str("[models]\nscout = \"claude:claude-sonnet-4-6\"\n").unwrap();
        c.fill_defaults();
        let resolved = c
            .resolve_model(c.models.scout.as_deref().unwrap())
            .expect("scout 取值必须走既有 resolve_model");
        assert_eq!(resolved.model, "claude-sonnet-4-6");
        // 角色名同样能用。
        let role: KanzeiConfig = toml::from_str("[models]\nscout = \"primary\"\n").unwrap();
        assert_eq!(role.models.scout.as_deref(), Some("primary"));

        // 层叠:项目层写了才覆盖,没写不打回默认(reasoning 那次漏合并的教训)。
        let mut base: KanzeiConfig =
            toml::from_str("[models]\nprimary = \"a:b\"\nscout = \"fast\"\n").unwrap();
        let layer: KanzeiConfig = toml::from_str("[models]\nprimary = \"c:d\"\n").unwrap();
        merge(&mut base, layer);
        assert_eq!(
            base.models.scout.as_deref(),
            Some("fast"),
            "项目层没写 scout 时必须保住全局层的值"
        );
        let layer2: KanzeiConfig = toml::from_str("[models]\nscout = \"primary\"\n").unwrap();
        merge(&mut base, layer2);
        assert_eq!(base.models.scout.as_deref(), Some("primary"));

        // 未知键体检不得把 scout 报成拼错(设置页透传不丢字段)。
        let value: toml::Value = toml::from_str("[models]\nscout = \"primary\"\n").unwrap();
        let warnings = unknown_keys(&value);
        assert!(
            !warnings.iter().any(|w| w.contains("scout")),
            "scout 是已知键,不该被报成未知: {warnings:?}"
        );
    }

    #[test]
    fn embeddings_缺节关闭_配置后启用_旧配置行为不变() {
        // 旧配置没有 [embeddings] → 通道关闭,检索退化为 lexical(验收①精神)。
        let empty: KanzeiConfig = toml::from_str("").unwrap();
        assert!(!empty.embeddings.enabled());

        // 只配 provider 不配 model → 仍关闭(缺一半不算启用)。
        let half: KanzeiConfig = toml::from_str("[embeddings]\nprovider = \"ollama\"\n").unwrap();
        assert!(!half.embeddings.enabled());

        // 配全 → 启用。
        let full: KanzeiConfig =
            toml::from_str("[embeddings]\nprovider = \"ollama\"\nmodel = \"nomic-embed-text\"\n")
                .unwrap();
        assert!(full.embeddings.enabled());
        assert_eq!(full.embeddings.provider.as_deref(), Some("ollama"));
        assert_eq!(full.embeddings.model.as_deref(), Some("nomic-embed-text"));
    }

    #[test]
    fn limits_缺节等于内置默认_项目层只覆盖它写了的那几个键() {
        // 旧配置没有 [limits] 时必须逐值等于改造前的硬编码常量。
        let empty: KanzeiConfig = toml::from_str("").unwrap();
        assert_eq!(empty.limits.max_tokens(), 8192);
        assert_eq!(empty.limits.subagent_timeout_secs(), 900);
        assert_eq!(empty.limits.context_budget_ratio(), 0.7);
        assert_eq!(empty.limits.max_tasks_per_turn(), 16);

        // 项目层只写一个键,不能把全局层其余的键打回默认(reasoning 那次漏合并的教训)。
        let mut base: KanzeiConfig =
            toml::from_str("[limits]\nmax_tokens = 4096\nsubagent_timeout_secs = 300\n").unwrap();
        let layer: KanzeiConfig = toml::from_str("[limits]\nmax_tokens = 16384\n").unwrap();
        merge(&mut base, layer);
        assert_eq!(base.limits.max_tokens(), 16384, "项目层写了的键要覆盖");
        assert_eq!(
            base.limits.subagent_timeout_secs(),
            300,
            "没写的键必须保住全局层的值"
        );

        // 离谱取值被夹住,不至于把运行时配崩。
        let wild: KanzeiConfig =
            toml::from_str("[limits]\ncontext_budget_ratio = 9.0\nmax_tasks_per_turn = 0\n")
                .unwrap();
        assert_eq!(wild.limits.context_budget_ratio(), 0.95);
        assert_eq!(wild.limits.max_tasks_per_turn(), 1);
    }

    /// R-173:屏障上界由子代理上界推导,且**永远宽于内层**。
    /// 配窄了会在子代理正常工作时误判超时,所以下界被夹住。
    #[test]
    fn 屏障上界由子代理上界推导且永远宽于内层() {
        let empty: KanzeiConfig = toml::from_str("").unwrap();
        assert_eq!(empty.limits.subagent_timeout_secs(), 900);
        assert_eq!(empty.limits.barrier_timeout_secs(), 1800, "默认 = 内层 ×2");

        // 跟着内层走:调小子代理上界,屏障默认值同步收窄,不用两处各配一遍。
        let derived: KanzeiConfig =
            toml::from_str("[limits]\nsubagent_timeout_secs = 60\n").unwrap();
        assert_eq!(derived.limits.barrier_timeout_secs(), 120);

        // 显式配置生效。
        let explicit: KanzeiConfig =
            toml::from_str("[limits]\nsubagent_timeout_secs = 60\nbarrier_timeout_secs = 300\n")
                .unwrap();
        assert_eq!(explicit.limits.barrier_timeout_secs(), 300);

        // 配得比内层还窄 → 夹到内层之上,屏障不会在子代理仍合法运行时误伤。
        let narrow: KanzeiConfig =
            toml::from_str("[limits]\nsubagent_timeout_secs = 900\nbarrier_timeout_secs = 10\n")
                .unwrap();
        assert!(
            narrow.limits.barrier_timeout_secs() > narrow.limits.subagent_timeout_secs(),
            "屏障上界必须严格宽于子代理上界"
        );
    }

    #[test]
    fn fast_mode_只看开关与本次解析到的供应商() {
        // R-158 验收③的判据此前只是两处就地抄写的表达式,一个测试都没有,
        // 于是另外 9 个构造点硬写 None 也没人发现(用户实测提出)。
        let mut c = KanzeiConfig::default();
        c.fill_defaults();
        c.providers.insert(
            "mock".into(),
            ProviderConfig {
                protocol: "openai".into(),
                base_url: "x".into(),
                api_key_env: None,
                api_key: None,
                auth: None,
                context_limit: None,
            },
        );
        let codex = c.resolve_model("codex:gpt-5.6-luna").unwrap();
        let other = c.resolve_model("mock:whatever").unwrap();

        c.models.codex_fast_mode = Some(true);
        assert_eq!(c.service_tier_for(&codex).as_deref(), Some("priority"));
        assert_eq!(c.service_tier_for(&other), None, "非 codex 供应商一律不发");

        c.models.codex_fast_mode = Some(false);
        assert_eq!(c.service_tier_for(&codex), None, "开关关掉就不发");
        c.models.codex_fast_mode = None;
        assert_eq!(c.service_tier_for(&codex), None, "缺字段按关闭处理");
    }

    #[test]
    fn bash_always_allow_keeps_exact_command() {
        assert_eq!(generalize_resource("bash", "git status"), "git status");
        assert_eq!(generalize_resource("write", "notes.md"), "notes.md");
    }

    #[test]
    fn bash_告警必须与实际评估一致() {
        // D-139:原实现按规则形态分别计数,混合形态下说假话。
        let parse = |text: &str| -> KanzeiConfig { toml::from_str(text).unwrap() };

        // ① 只有 legacy 规则:确实会逐次询问。
        let only_legacy = parse(
            "[[permissions.rules]]\naction = \"bash\"\nresource = \"git status\"\neffect = \"allow\"\n",
        );
        let w = only_legacy.bash_permission_warnings();
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("逐次询问"), "{w:?}");

        // ② legacy + 显式 bash/* allow 并存:last-match-wins 实际全量放行,
        //    绝不能再说"将逐次询问"——这正是原实现的谎言。
        let mixed = parse(
            "[[permissions.rules]]\naction = \"bash\"\nresource = \"git status\"\neffect = \"allow\"\n\
             [[permissions.rules]]\naction = \"bash\"\nresource = \"*\"\neffect = \"allow\"\n",
        );
        let w = mixed.bash_permission_warnings();
        assert!(w[0].contains("全量放行"), "混合形态必须如实说放行: {w:?}");
        assert!(
            !w.iter().any(|line| line.contains("将逐次询问")),
            "混合形态不得再说会询问: {w:?}"
        );
        assert!(w[1].contains("不生效"), "应说明 legacy 规则被覆盖: {w:?}");

        // ③ deny 是仍然生效的护栏,不该被算进"将逐次询问"的计数。
        let with_deny = parse(
            "[[permissions.rules]]\naction = \"bash\"\nresource = \"rm *\"\neffect = \"deny\"\n",
        );
        assert!(
            with_deny.bash_permission_warnings().is_empty(),
            "deny 护栏不该触发降级告警: {:?}",
            with_deny.bash_permission_warnings()
        );

        // ④ 空白包裹的通配与裸通配判定必须一致。
        let padded = parse(
            "[[permissions.rules]]\naction = \"bash\"\nresource = \" * \"\neffect = \"allow\"\n",
        );
        assert_eq!(padded.explicit_bash_wildcard_allows().len(), 1);
    }

    /// F8 ①(D-139 的新形态):**探针 Allow ≠ yolo**。探针是一条具体命令,用户完全可能
    /// 只授权过它自己;老判据会把"你授权过 git status"说成"bash 已经全放开",又一句假话。
    #[test]
    fn 探针被单条规则命中时不得冒充全量放行() {
        // 规则内容与探针逐字节相同:探针必然 Allow,但配置里没有任何 `bash/*`。
        let probe = serde_json::json!({ "command": "git status", "workdir": "." }).to_string();
        let text = format!(
            "[[permissions.rules]]\naction = \"bash\"\nresource = '{probe}'\neffect = \"allow\"\n"
        );
        let config: KanzeiConfig = toml::from_str(&text).unwrap();

        // 先钉住前提:这条配置下探针的**实际评估结果**确实是 Allow,
        // 否则这条用例根本没走到被修的那个分支。
        let mut ruleset = crate::permission::Ruleset::default();
        for rule in &config.permissions.rules {
            ruleset.push(rule.clone());
        }
        assert_eq!(
            ruleset.evaluate(BASH_ACTION, &probe),
            crate::permission::Effect::Allow
        );
        assert_eq!(config.explicit_bash_wildcard_allows().len(), 0);

        let w = config.bash_permission_warnings();
        assert!(
            !w.iter().any(|line| line.contains("全量放行")),
            "没有 bash/* 规则就不许说全量放行: {w:?}"
        );
        assert!(
            w.iter().any(|line| line.contains("其余命令仍会逐次询问")),
            "必须如实说清范围: {w:?}"
        );
    }

    /// D-269 收敛路径(验收⑥):停止对 bash 资源做路径规范化之后,**已落盘的结构化规则
    /// 里有一类是可解析的**——命令文本被 Windows 整串小写改写过,JSON 还合法,于是
    /// `legacy_bash_rules` 一条都认不出来:用户零告警、零指引,只看到命令又开始逐次询问。
    ///
    /// 夹具用的是本机真实配置里的形态(9 条非结构化 / 7 条被折成不可解析 / 5 条可解析,
    /// 其中 3 条命令原文有大写因而已经失效)。
    #[test]
    fn 被小写折坏的结构化规则也要进告警() {
        let text = r#"
# ① 非结构化(裸命令):legacy_bash_rules 认得
[[permissions.rules]]
action = "bash"
resource = "Get-ChildItem *"
effect = "allow"

# ② 结构化但被 `\` → `/` 折成不可解析:legacy_bash_rules 也认得
[[permissions.rules]]
action = "bash"
resource = '{"command":"git commit -m /"整理/"","workdir":"c:/proj"}'
effect = "allow"

# ③ 结构化、可解析、但命令原文有大写被整串小写:老判据完全认不出来
[[permissions.rules]]
action = "bash"
resource = '{"command":"(get-content /x/main.rs).count","workdir":"c:/proj"}'
effect = "allow"

# ④ 结构化、可解析、天生全小写:其实还活着,判据只能宽报(理由见函数注释)
[[permissions.rules]]
action = "bash"
resource = '{"command":"git status --short","workdir":"c:/proj"}'
effect = "allow"

# ⑤ deny 是仍然生效的护栏,不算失效(D-139)
[[permissions.rules]]
action = "bash"
resource = '{"command":"rm -rf /","workdir":"c:/proj"}'
effect = "deny"
"#;
        let config: KanzeiConfig = toml::from_str(text).unwrap();

        // ③④ 正是老判据漏掉的那一类:它们是合法结构化资源,legacy 那边一条不收。
        let legacy = config.legacy_bash_rules_needing_downgrade();
        assert_eq!(legacy.len(), 2, "①② 归 legacy: {legacy:?}");

        let stale = config.structured_bash_rules_possibly_stale();
        assert_eq!(stale.len(), 2, "③④ 归新判据,deny 的 ⑤ 不算: {stale:?}");
        assert!(stale.iter().any(|r| r.resource.contains("get-content")));

        let w = config.bash_permission_warnings();
        let text = w.join("\n");
        // 两类各说各的,数目都要出现。
        assert!(text.contains("2 条旧 bash 权限规则"), "{w:?}");
        assert!(text.contains("2 条结构化 bash 规则"), "{w:?}");
        // 可执行的动作:说清失效 + 下次遇到重新授权一次即可。
        assert!(text.contains("总是允许"), "必须给出可照做的动作: {w:?}");
        // 点名到具体命令,而不是只报一个数字。
        assert!(text.contains("Get-ChildItem *"), "{w:?}");
        assert!(text.contains("get-content"), "{w:?}");
        // 不许再出现那句没有落点的老文案。
        assert!(!text.contains("请重新选择精确作用域"), "{w:?}");
    }

    /// F8 ⑥ 接线之三:非交互策略键缺席 = `deny` = **今天的行为**,旧配置逐字节不变。
    #[test]
    fn 非交互策略缺键等于deny且旧配置行为不变() {
        let empty: KanzeiConfig = toml::from_str("").unwrap();
        assert_eq!(empty.non_interactive_policy(), NonInteractive::Deny);
        assert!(empty.non_interactive_policy_warning().is_none());

        // 只有 rules 的老配置同样不变,也不产额外告警。
        let old: KanzeiConfig = toml::from_str(
            "[[permissions.rules]]\naction = \"bash\"\nresource = \"*\"\neffect = \"allow\"\n",
        )
        .unwrap();
        assert_eq!(old.non_interactive_policy(), NonInteractive::Deny);
        assert!(old.non_interactive_policy_warning().is_none());

        for (写法, 期望) in [
            ("deny", NonInteractive::Deny),
            ("rules_only", NonInteractive::RulesOnly),
            ("allow_listed", NonInteractive::AllowListed),
            ("  Rules_Only  ", NonInteractive::RulesOnly),
        ] {
            let c: KanzeiConfig =
                toml::from_str(&format!("[permissions]\nnon_interactive = \"{写法}\"\n")).unwrap();
            assert_eq!(c.non_interactive_policy(), 期望, "写法 {写法:?}");
            assert!(
                c.non_interactive_policy_warning().is_none(),
                "写法 {写法:?}"
            );
        }
    }

    /// F8 ⑥ 接线之三(续):认不出来的取值 **fail-closed 回落 deny 并且必须出声**。
    /// 悄悄回落最坏——用户以为开了 rules_only,实际每次停机,还归不到因。
    #[test]
    fn 非交互策略非法取值fail_closed回落deny并告警() {
        for 写法 in ["rulesonly", "yolo", "true", "1"] {
            let c: KanzeiConfig =
                toml::from_str(&format!("[permissions]\nnon_interactive = \"{写法}\"\n")).unwrap();
            assert_eq!(
                c.non_interactive_policy(),
                NonInteractive::Deny,
                "非法取值 {写法:?} 必须回落 deny"
            );
            let warning = c
                .non_interactive_policy_warning()
                .unwrap_or_else(|| panic!("非法取值 {写法:?} 必须产告警"));
            assert!(warning.contains(写法), "告警要点名原值: {warning}");
            assert!(warning.contains("fail-closed"), "{warning}");
            assert!(warning.contains("rules_only"), "要列出可用取值: {warning}");
        }
        // 空串按"没写"处理:不回落告警,行为等于缺键。
        let 空: KanzeiConfig = toml::from_str("[permissions]\nnon_interactive = \"  \"\n").unwrap();
        assert_eq!(空.non_interactive_policy(), NonInteractive::Deny);
        assert!(空.non_interactive_policy_warning().is_none());
    }

    /// F8 ⑥ 接线之二:`merge` 的 overlay。只加字段不接这一行,项目层设了会静默不生效
    /// (`Limits::barrier_timeout_secs` 的前车之鉴)。
    #[test]
    fn 非交互策略项目层覆盖全局层() {
        let mut base: KanzeiConfig =
            toml::from_str("[permissions]\nnon_interactive = \"deny\"\n").unwrap();
        let layer: KanzeiConfig =
            toml::from_str("[permissions]\nnon_interactive = \"rules_only\"\n").unwrap();
        merge(&mut base, layer);
        assert_eq!(base.non_interactive_policy(), NonInteractive::RulesOnly);

        // 反向:项目层没写这个键,不能把全局层的值打回默认。
        let mut base: KanzeiConfig =
            toml::from_str("[permissions]\nnon_interactive = \"allow_listed\"\n").unwrap();
        let layer: KanzeiConfig = toml::from_str(
            "[[permissions.rules]]\naction = \"bash\"\nresource = \"*\"\neffect = \"allow\"\n",
        )
        .unwrap();
        merge(&mut base, layer);
        assert_eq!(base.non_interactive_policy(), NonInteractive::AllowListed);
        assert_eq!(base.permissions.rules.len(), 1, "rules 仍然是追加语义");
    }

    /// 序列化顺序陷阱:TOML 里标量键必须排在**数组表之前**,否则 `non_interactive` 会被
    /// 当成 `[[permissions.rules]]` 最后一项的字段,写出去的文件读回来就变了个意思。
    /// `unknown_keys_schema_matches_struct` 只覆盖了缺省(None)那种情况,盖不住这条。
    #[test]
    fn 非交互策略键与规则数组同时存在时能原样round_trip() {
        let mut config = KanzeiConfig::default();
        config.permissions.non_interactive = Some("rules_only".into());
        config.permissions.rules.push(Rule {
            action: "bash".into(),
            resource: "*".into(),
            effect: crate::permission::Effect::Allow,
        });
        let text = toml::to_string_pretty(&config).unwrap();
        let back: KanzeiConfig = toml::from_str(&text)
            .unwrap_or_else(|e| panic!("序列化产物读不回来(标量排在数组表之后?): {e}\n{text}"));
        assert_eq!(back.non_interactive_policy(), NonInteractive::RulesOnly);
        assert_eq!(back.permissions.rules.len(), 1);
        let raw: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(unknown_keys(&raw), Vec::<String>::new());
    }

    /// F8 ⑥ 接线之一:新键要进 `unknown_keys` 的已知清单,否则一写就收到"未知配置项"假告警。
    #[test]
    fn 非交互策略键不产生未知配置项假告警() {
        let raw: toml::Value =
            toml::from_str("[permissions]\nnon_interactive = \"rules_only\"\n").unwrap();
        assert_eq!(unknown_keys(&raw), Vec::<String>::new());
        // 同一节里拼错的键仍然要报出来,清单不是把整节放行。
        let raw: toml::Value =
            toml::from_str("[permissions]\nnon_interactiv = \"rules_only\"\n").unwrap();
        assert_eq!(unknown_keys(&raw), vec!["permissions.non_interactiv"]);
    }

    #[test]
    fn unknown_fields_are_tolerated_and_reported() {
        // D-084:新版本写入的配置节不能炸掉旧二进制;未知键忽略但可见。
        let text = r#"
[models]
primary = "anthropic:claude-sonnet-5"
new_feature_field = "from-a-newer-kanzei"

[future_section]
whatever = 1

[providers.kimi]
protocol = "openai"
base_url = "https://api.moonshot.cn/v1"
typo_fielt = true
"#;
        let config: KanzeiConfig = toml::from_str(text).unwrap();
        assert_eq!(
            config.models.primary.as_deref(),
            Some("anthropic:claude-sonnet-5")
        );
        assert!(config.providers.contains_key("kimi"));
        let raw: toml::Value = toml::from_str(text).unwrap();
        let mut unknown = unknown_keys(&raw);
        unknown.sort();
        assert_eq!(
            unknown,
            vec![
                "future_section",
                "models.new_feature_field",
                "providers.kimi.typo_fielt"
            ]
        );
    }

    #[test]
    fn unknown_keys_schema_matches_struct() {
        // 守护 unknown_keys 的手写清单与结构体不漂移:全量序列化后必须零告警。
        let mut config = KanzeiConfig::default();
        config.fill_defaults();
        config.models.reasoning = Some("high".into());
        config.proxy = Some("http://127.0.0.1:7890".into());
        config.profile.default = Some("dev".into());
        config.permissions.rules.push(Rule {
            action: "bash".into(),
            resource: "git status".into(),
            effect: crate::permission::Effect::Allow,
        });
        let raw: toml::Value = toml::from_str(&toml::to_string_pretty(&config).unwrap()).unwrap();
        assert_eq!(unknown_keys(&raw), Vec::<String>::new());
    }

    #[test]
    fn append_allow_rule_preserves_comments_and_unknown_fields() {
        // D-083:追加规则不得抹掉用户手写的注释、排版与未知字段。
        let root = std::env::temp_dir().join(format!(
            "kanzei-d083-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        let path = root.join(".kanzei").join("kanzei.toml");
        std::fs::write(
            &path,
            "# 我的配置,别动我的注释\n[models]\nprimary = \"anthropic:claude-sonnet-5\" # 行尾注释\n\n[future_section]\nkeep_me = true\n",
        )
        .unwrap();
        append_allow_rule(&root, "bash", "git status").unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        for expected in [
            "# 我的配置,别动我的注释",
            "# 行尾注释",
            "[future_section]",
            "keep_me = true",
        ] {
            assert!(
                saved.contains(expected),
                "missing preserved text: {expected}\n---\n{saved}"
            );
        }
        let config: KanzeiConfig = toml::from_str(&saved).unwrap();
        assert_eq!(config.permissions.rules.len(), 1);
        assert_eq!(config.permissions.rules[0].resource, "git status");
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 造一棵 `<tmp>/home/{.kanzei, projects/repo/{.git, src}}`,用来验证项目根解析。
    fn project_root_fixture(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kanzei-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("home").join(".kanzei")).unwrap();
        std::fs::create_dir_all(root.join("home").join("projects").join("repo").join(".git"))
            .unwrap();
        std::fs::create_dir_all(root.join("home").join("projects").join("repo").join("src"))
            .unwrap();
        root
    }

    #[test]
    fn nearest_git_wins_over_a_farther_kanzei() {
        // 更近的 `.git` 必须赢过更远的 `.kanzei`,否则仓库自己的根被丢掉、
        // 一路解析到 HOME(注释一直写的是"最近",实现却不是)。
        let root = project_root_fixture("project-root-nearest");
        let home = root.join("home");
        let repo = home.join("projects").join("repo");
        assert_eq!(
            discover_project_root_with_home(&repo.join("src"), Some(&home)),
            Some(repo.clone())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn home_global_config_dir_is_not_a_project_marker() {
        // `~/.kanzei` 是全局配置根、必然存在。它若算项目标记,HOME 下所有无标记目录
        // 都会解析到 HOME,项目级产物(project/、state.db)就会漏进全局目录。
        let root = project_root_fixture("project-root-home");
        let home = root.join("home");
        let plain = home.join("scratch");
        std::fs::create_dir_all(&plain).unwrap();
        // 一对前后对照:同一棵目录树,只有"这个目录是不是 HOME"这一个变量。
        // (不断言具体落点——临时目录本身就在真实 HOME 之下,再往上必然还有标记。)
        assert_eq!(
            discover_project_root_with_home(&plain, None),
            Some(home.clone()),
            "不认 HOME 时,~/.kanzei 就是把 HOME 变成项目根的那块磁铁"
        );
        assert_ne!(
            discover_project_root_with_home(&plain, Some(&home)),
            Some(home.clone()),
            "HOME 的 .kanzei 是全局配置根,不该被当成项目标记"
        );
        // 但 HOME 之外的 `.kanzei` 仍然是标记。
        let marked = home.join("projects");
        std::fs::create_dir_all(marked.join(".kanzei")).unwrap();
        assert_eq!(
            discover_project_root_with_home(&marked.join("nested"), Some(&home)),
            Some(marked)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// D-194:HOME 判断必须扛得住同一目录的不同写法,否则 D-189 的排除等于没做。
    #[test]
    fn home_marker_exclusion_survives_path_form_differences() {
        let root = project_root_fixture("project-root-home-forms");
        let home = root.join("home");
        let plain = home.join("scratch");
        std::fs::create_dir_all(&plain).unwrap();

        // 这些写法在 Windows 上指的是同一个目录:扩展长度前缀(canonicalize 的产物)、
        // 正斜杠、末尾分隔符、不同大小写(shell 里键入的)。任何一种没被归一,
        // `~/.kanzei` 就重新变回项目根磁铁。
        let mut variants = vec![PathBuf::from(format!(r"\\?\{}", home.display()))];
        #[cfg(windows)]
        variants.extend([
            PathBuf::from(home.display().to_string().replace('\\', "/")),
            PathBuf::from(format!("{}\\", home.display())),
            PathBuf::from(home.display().to_string().to_lowercase()),
        ]);
        for variant in variants {
            assert_ne!(
                discover_project_root_with_home(&plain, Some(&variant)),
                Some(home.clone()),
                "HOME 写成 {} 时排除失效",
                variant.display()
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// D-194:真实 HOME 必须被 `is_home_root` 认出来——CLI 靠它在开跑前拦下
    /// "项目级产物落进全局配置根"。
    #[test]
    fn is_home_root_recognizes_real_home_in_any_form() {
        let Some(home) = dirs::home_dir() else {
            return; // 无 HOME 的环境跳过,不是被测行为。
        };
        assert!(is_home_root(&home));
        #[cfg(windows)]
        {
            assert!(is_home_root(&PathBuf::from(format!(
                "{}\\",
                home.display()
            ))));
            assert!(is_home_root(&PathBuf::from(
                home.display().to_string().replace('\\', "/")
            )));
            assert!(is_home_root(&PathBuf::from(
                home.display().to_string().to_uppercase()
            )));
            // canonicalize 的产物形态:剥了 `\\?\` 才认得出来。
            assert!(is_home_root(&PathBuf::from(format!(
                r"\\?\{}",
                home.display()
            ))));
        }
        assert!(!is_home_root(&home.join("projects")));
    }

    /// D-194 补漏:`dir_key` 不折叠 `.` / `..` 时,`C:\Users\kanzei\.` 这类写法让 HOME
    /// 拦截静默失效——而 `resolve_project_root` 的标记校验对它照样成立(HOME 下有
    /// `.kanzei`),两道拦截一起被绕过,project 级 state.db 被写进全局配置根 `~/.kanzei`
    /// (实测发生过)。
    ///
    /// 这条路是 R-182 的显式主根入口打开的:在那之前根恒来自 `current_dir()`,不含
    /// `.`/`..` 段,写不出这种串;新入口收的正是用户任意书写的路径。
    #[test]
    fn is_home_root_folds_dot_and_dotdot_segments() {
        let Some(home) = dirs::home_dir() else {
            return; // 无 HOME 的环境跳过,不是被测行为。
        };
        let sep = std::path::MAIN_SEPARATOR;
        let text = home.display().to_string();
        let mut forms = vec![
            // 尾随 `.`:文件系统里就是 HOME 自己。
            PathBuf::from(format!("{text}{sep}.")),
            // 下一级再 `..` 弹回来。折叠是纯词法的,所以那一级存不存在都一样。
            PathBuf::from(format!("{text}{sep}Documents{sep}..")),
            // `.` 后面还跟着尾分隔符。
            PathBuf::from(format!("{text}{sep}.{sep}")),
            // 多段叠加,一路弹回 HOME。
            PathBuf::from(format!("{text}{sep}a{sep}..{sep}.{sep}b{sep}..")),
        ];
        #[cfg(windows)]
        {
            let slash = text.replace('\\', "/");
            forms.push(PathBuf::from(format!("{slash}/./")));
            forms.push(PathBuf::from(format!("{slash}/Documents/..")));
            // 大小写 + `.` 段 + `\\?\` 前缀三者叠加,任一环节漏了都拦不住。
            forms.push(PathBuf::from(format!(
                r"\\?\{}\.",
                text.to_lowercase().trim_end_matches('\\')
            )));
        }
        for form in forms {
            assert!(
                is_home_root(&form),
                "含 . / .. 的写法必须被认成 HOME: {}",
                form.display()
            );
        }
    }

    /// 折叠不许过头:路径里带 `.` 的**合法目录名**(`v1.0`、`.config`)不是 `.` 段,
    /// 正常项目根不能被误拦;`..` 也必须真的向上一级,而不是被吞掉。
    #[test]
    fn dir_key_keeps_dotted_directory_names() {
        let app = PathBuf::from(r"C:\proj\v1.0\app");
        // `v1.0` / `.config` 是目录名,不是 `.` 段:各自都还在。
        assert_ne!(dir_key(&app), dir_key(&PathBuf::from(r"C:\proj\v1.0")));
        assert_ne!(
            dir_key(&app),
            dir_key(&PathBuf::from(r"C:\proj\v1.0\app\.config"))
        );
        assert_ne!(dir_key(&app), dir_key(&PathBuf::from(r"C:\proj\app")));
        // 而真正的 `.` 段确实被折掉。
        assert_eq!(
            dir_key(&app),
            dir_key(&PathBuf::from(r"C:\proj\v1.0\app\."))
        );
        assert_eq!(
            dir_key(&app),
            dir_key(&PathBuf::from(r"C:\proj\v1.0\app\sub\.."))
        );

        let Some(home) = dirs::home_dir() else {
            return;
        };
        // 正常项目根(哪怕就在 HOME 底下、哪怕名字里带点)不被误判成 HOME。
        assert!(!is_home_root(&home.join("proj").join("v1.0").join("app")));
        assert!(!is_home_root(&home.join("v1.0")));
        assert!(!is_home_root(&home.join(".config")));
        // `..` 真的向上一级:HOME 的父目录不是 HOME。
        assert!(!is_home_root(&home.join("..")));
        assert!(!is_home_root(&home.join("a").join("..").join("..")));
    }

    /// 造一个空的临时夹具根,名字带 pid + 纳秒,避免同进程并发用例互踩。
    fn 临时夹具根(标签: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kanzei-{标签}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// `C:\x\y` → `\\localhost\C$\x\y`(本机管理共享)。拿不到盘符就返回 None。
    #[cfg(windows)]
    fn 管理共享形态(path: &Path, host: &str) -> Option<PathBuf> {
        let text = path.display().to_string();
        let bytes = text.as_bytes();
        if bytes.len() < 3 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
            return None;
        }
        Some(PathBuf::from(format!(
            r"\\{host}\{}${}",
            &text[..1],
            &text[2..]
        )))
    }

    /// D-194 补漏三(对抗复核实测推翻了词法折叠):**「词法不同、文件系统同一」的别名补不完**。
    ///
    /// 复核实测出的两条现成绕过:`C:\Users\kanzei.`(Windows 剥掉末段尾随点)与
    /// `\\localhost\C$\Users\kanzei`(UNC,漫游/网络 profile 下本来就是合法写法);
    /// 再往下还有符号链接、junction、`subst` 虚拟盘、8.3 短名。本条用**真夹具**证明
    /// 身份层认得出它们,并且**每条都先断言词法层认不出**——否则用例证明不了身份层在干活。
    ///
    /// 夹具全部造在系统临时目录,一个字节都不碰真 HOME;造不出来的形态跳过并打印原因。
    #[test]
    #[cfg(windows)]
    fn 文件系统别名一律被认成同一个目录() {
        let root = 临时夹具根("d194-alias");
        // 名字取长一点,8.3 短名才有 `~1` 形态可测。
        let 目标 = root.join("homedirectory");
        std::fs::create_dir_all(目标.join(".kanzei")).unwrap();
        // 对照组:名字里带点的**合法**项目根,不能被误拦。
        let 带点项目根 = root.join("v1.0").join("app");
        std::fs::create_dir_all(&带点项目根).unwrap();

        let mut 跳过: Vec<String> = Vec::new();
        let 断言别名 = |形态: &Path, 名称: &str| {
            assert_ne!(
                dir_key(&目标),
                dir_key(形态),
                "夹具失效:{名称} 在词法上必须与目标不同,否则这条用例证明不了身份层"
            );
            assert!(
                is_same_dir(&目标, 形态),
                "{名称} 在磁盘上就是同一个目录,必须被认出来: {}",
                形态.display()
            );
        };

        // ① 尾随点:Windows 剥掉末段的尾随点,`homedirectory.` 就是 `homedirectory`。
        断言别名(&PathBuf::from(format!("{}.", 目标.display())), "尾随点");

        // ② junction:不需要管理员权限,mklink /J 即可。
        let junction = root.join("junc");
        let 造junction = std::process::Command::new("cmd")
            .args([
                "/c",
                "mklink",
                "/J",
                &junction.display().to_string(),
                &目标.display().to_string(),
            ])
            .output();
        match 造junction {
            Ok(_) if junction.is_dir() => 断言别名(&junction, "junction"),
            Ok(out) => 跳过.push(format!(
                "junction: mklink /J 没造出来({})",
                String::from_utf8_lossy(&out.stdout).trim()
            )),
            Err(e) => 跳过.push(format!("junction: 起不了 cmd({e})")),
        }

        // ③ 符号链接:Windows 上要管理员或开发者模式,普通会话大概率造不出来。
        let 符号链接 = root.join("symd");
        match std::os::windows::fs::symlink_dir(&目标, &符号链接) {
            Ok(()) => 断言别名(&符号链接, "符号链接"),
            Err(e) => 跳过.push(format!(
                "符号链接: {e}(Windows 建目录符号链接需管理员或开发者模式)"
            )),
        }

        // ④ 8.3 短名:卷上可能已经关掉 8dot3name 生成。
        let 短名 = root.join("HOMEDI~1");
        if 短名.is_dir() {
            断言别名(&短名, "8.3 短名");
        } else {
            跳过.push("8.3 短名: 本卷未生成 8dot3 名(fsutil 8dot3name 已关)".into());
        }

        // ⑤ UNC 管理共享:两种主机写法各一遍;非管理员会话访问 C$ 会失败,失败即跳过。
        for host in ["localhost", "127.0.0.1"] {
            match 管理共享形态(&目标, host) {
                Some(unc) if unc.is_dir() => 断言别名(&unc, &format!(r"UNC \\{host}\C$")),
                Some(unc) => 跳过.push(format!("UNC {}: 不可达(需要管理共享权限)", unc.display())),
                None => 跳过.push(format!("UNC {host}: 夹具不在盘符路径上")),
            }
        }

        // 反向:别名判定不许过头——不同的目录仍然是不同的目录。
        assert!(!is_same_dir(&目标, &带点项目根), "带点的合法项目根被误拦");
        assert!(!is_same_dir(&目标, &root.join("v1.0")));
        assert!(!is_same_dir(&目标, &root), "父目录不是同一个目录");
        assert!(
            !is_same_dir(&目标, &目标.join(".kanzei")),
            "子目录不是同一个目录"
        );

        if !跳过.is_empty() {
            eprintln!("文件系统别名用例跳过的形态: {跳过:#?}");
        }
        // junction 先单独删,避免递归删除时穿到目标里去。
        let _ = std::fs::remove_dir(&junction);
        let _ = std::fs::remove_dir(&符号链接);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 复核实测报上来的那两条绕过,直接打在**真 HOME**上(只读:比较不写任何东西)。
    /// 这是红线用例——它红就意味着 `KANZEI_PROJECT_ROOT` 又能把项目产物写进 `~/.kanzei`。
    #[test]
    #[cfg(windows)]
    fn 真home的尾随点与unc写法不再绕过拦截() {
        let Some(home) = dirs::home_dir() else {
            return; // 无 HOME 的环境跳过,不是被测行为。
        };
        let 尾随点 = PathBuf::from(format!("{}.", home.display()));
        assert_ne!(dir_key(&home), dir_key(&尾随点), "夹具失效:词法上本该不同");
        assert!(
            is_home_root(&尾随点),
            "`{}` 在磁盘上就是 HOME,必须被拦下",
            尾随点.display()
        );

        let mut 测到的unc = 0usize;
        for host in ["localhost", "127.0.0.1"] {
            let Some(unc) = 管理共享形态(&home, host) else {
                continue;
            };
            if !unc.is_dir() {
                eprintln!("跳过 {}(需要管理共享权限)", unc.display());
                continue;
            }
            assert_ne!(dir_key(&home), dir_key(&unc), "夹具失效:词法上本该不同");
            assert!(is_home_root(&unc), "UNC 写法必须被拦下: {}", unc.display());
            测到的unc += 1;
        }
        if 测到的unc == 0 {
            eprintln!("本机管理共享不可达,UNC 形态未覆盖(见同名夹具用例)");
        }
    }

    /// 拿不到文件系统身份时**回落词法折叠**,不因为 canonicalize 失败就当成两个目录。
    /// (路径不存在这条在 CLI 里其实到不了 `is_home_root`——`resolve_project_root`
    /// 先报"显式主根指向的路径不存在";这里钉的是守卫本身的兜底行为。)
    #[test]
    fn canonicalize失败时回落到词法折叠() {
        let 不存在 = std::env::temp_dir().join(format!(
            "kanzei-d194-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(!不存在.exists());
        assert!(std::fs::canonicalize(&不存在).is_err());
        assert!(is_same_dir(&不存在, &不存在.join(".")));
        assert!(is_same_dir(&不存在, &不存在.join("x").join("..")));
        assert!(!is_same_dir(&不存在, &不存在.join("x")));
    }

    /// R-182 内容②:`load_at_root` 是**显式**入口——给它哪个根就读哪个根,
    /// 一步都不许向上发现;发现式的老入口 `load_with_warnings` 语义原样不变。
    #[test]
    fn load_at_root不做根发现() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-r182-at-root-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sub = root.join("sub");
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        std::fs::create_dir_all(sub.join(".kanzei")).unwrap();
        std::fs::write(
            root.join(".kanzei").join("kanzei.toml"),
            "[models]\nprimary = \"mock:root-model\"\n",
        )
        .unwrap();
        std::fs::write(
            sub.join(".kanzei").join("kanzei.toml"),
            "[models]\nprimary = \"mock:sub-model\"\n",
        )
        .unwrap();

        // 显式入口:传 root 就读 root 那份,哪怕 cwd 概念上在 sub 里。
        let (at_root, _) = KanzeiConfig::load_with_warnings_at_root(&root).unwrap();
        assert_eq!(at_root.models.primary.as_deref(), Some("mock:root-model"));
        assert_eq!(
            KanzeiConfig::load_at_root(&root)
                .unwrap()
                .models
                .primary
                .as_deref(),
            Some("mock:root-model")
        );
        // 发现式老入口:从 sub 出发仍然命中 sub 自己的 `.kanzei`(行为一字不改)。
        let (discovered, _) = KanzeiConfig::load_with_warnings(&sub).unwrap();
        assert_eq!(discovered.models.primary.as_deref(), Some("mock:sub-model"));

        std::fs::remove_dir_all(root).unwrap();
    }

    /// 优先级定死:参数 > 环境变量 > 发现式。本函数只看「显式还是没有」这一层;
    /// 参数与环境变量的先后由 CLI 侧合成(main.rs 的 `explicit_main_root`)。
    #[test]
    fn resolve_project_root显式优先() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-r182-resolve-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sub = root.join("sub");
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        std::fs::create_dir_all(sub.join(".kanzei")).unwrap();

        // 显式给了根:cwd 在 sub 里也照样返回 root。
        assert_eq!(
            resolve_project_root(Some(&root), &sub).unwrap(),
            root.clone()
        );
        // 没给:逐字节退回 discover_project_root——这同时证明本批没去改它。
        assert_eq!(
            resolve_project_root(None, &sub).unwrap(),
            discover_project_root(&sub).unwrap()
        );
        assert_eq!(resolve_project_root(None, &sub).unwrap(), sub.clone());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn 显式主根必须是真项目根() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-r182-marker-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let empty = root.join("empty");
        let file = root.join("a-file.txt");
        let worktree = root.join("worktree");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(&file, "not a directory").unwrap();
        // worktree 的 `.git` 是文件,必须照样算标记。
        std::fs::write(worktree.join(".git"), "gitdir: ../repo/.git/worktrees/w\n").unwrap();

        for bad in [root.join("does-not-exist"), empty.clone(), file.clone()] {
            let error = resolve_project_root(Some(&bad), &root)
                .unwrap_err()
                .to_string();
            // 错误必须点名来源键名,否则用户不知道该去改哪个开关/变量。
            assert!(
                error.contains("--project-root") && error.contains("KANZEI_PROJECT_ROOT"),
                "错误文本要点名来源: {error}"
            );
        }
        assert_eq!(
            resolve_project_root(Some(&worktree), &root).unwrap(),
            worktree
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    /// 不做 canonicalize:`\\?\` 形态会让用户已写的绝对路径权限规则一夜失配。
    #[test]
    fn 显式主根不做canonicalize() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-r182-nocanon-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();

        let mut forms = vec![PathBuf::from(format!(
            "{}{}",
            root.display(),
            std::path::MAIN_SEPARATOR
        ))];
        #[cfg(windows)]
        forms.push(PathBuf::from(
            root.display().to_string().to_lowercase().replace('\\', "/"),
        ));
        for form in forms {
            let resolved = resolve_project_root(Some(&form), &root).unwrap();
            assert!(
                !resolved.display().to_string().starts_with(r"\\?\"),
                "不该 canonicalize: {}",
                resolved.display()
            );
            // 原样返回:用户写下什么就是什么。
            assert_eq!(resolved, form);
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn append_allow_rule_preserves_structured_bash_scope() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-d051-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let resource =
            r#"{"command":"git status > .kanzei/project/requirements.md","workdir":"C:/project"}"#;
        let path = append_allow_rule(&root, "bash", resource).unwrap();
        let saved: KanzeiConfig = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved.permissions.rules.len(), 1);
        assert_eq!(saved.permissions.rules[0].action, "bash");
        assert_eq!(saved.permissions.rules[0].resource, resource);
        assert_eq!(
            saved.permissions.rules[0].effect,
            crate::permission::Effect::Allow
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn legacy_bash_rules_are_detected_without_rewriting_config() {
        let config: KanzeiConfig = toml::from_str(
            r#"
[[permissions.rules]]
 action = "bash"
 resource = "git status"
 effect = "allow"
[[permissions.rules]]
 action = "bash"
 resource = "*"
 effect = "allow"
[[permissions.rules]]
 action = "bash"
 resource = '{"command":"git status","workdir":"C:/project"}'
 effect = "allow"
[[permissions.rules]]
 action = "write"
 resource = "notes.md"
 effect = "allow"
"#,
        )
        .unwrap();
        let legacy = config.legacy_bash_rules();
        assert_eq!(legacy.len(), 2);
        assert_eq!(legacy[0].resource, "git status");
        assert_eq!(config.legacy_bash_rules_needing_downgrade().len(), 1);
        assert_eq!(config.explicit_bash_wildcard_allows().len(), 1);
        // D-139:本夹具是混合形态(legacy + 显式 bash/* allow),last-match-wins 的
        // 真实行为是全量放行。旧断言同时要求"将逐次询问"与"将全量放行",把互相矛盾的
        // 两句话固化进了回归网——正是它让 D-122 带着假修复通过验收。
        let warnings = config.bash_permission_warnings();
        assert!(warnings[0].contains("全量放行"), "{warnings:?}");
        assert!(
            !warnings.iter().any(|line| line.contains("将逐次询问")),
            "实际全量放行时不得声称会询问: {warnings:?}"
        );
        assert!(
            warnings[1].contains("不生效"),
            "应说明 legacy 被覆盖: {warnings:?}"
        );
        assert_eq!(config.permissions.rules.len(), 4);
    }
    #[test]
    fn merge_layers() {
        let mut base: KanzeiConfig = toml::from_str(
            r#"
[models]
primary = "anthropic:claude-sonnet-5"
[[permissions.rules]]
action = "bash"
resource = "*"
effect = "allow"
"#,
        )
        .unwrap();
        let layer: KanzeiConfig = toml::from_str(
            r#"
[models]
primary = "kimi:kimi-k2"
[providers.kimi]
protocol = "openai"
base_url = "https://api.moonshot.cn/v1"
api_key_env = "MOONSHOT_API_KEY"
[[permissions.rules]]
action = "bash"
resource = "rm *"
effect = "deny"
"#,
        )
        .unwrap();
        merge(&mut base, layer);
        assert_eq!(base.models.primary.as_deref(), Some("kimi:kimi-k2"));
        assert_eq!(base.permissions.rules.len(), 2);
        assert!(base.providers.contains_key("kimi"));
    }

    // D-245 验收②:merge_file 必须把 [cadence] 的显式键逐项覆盖进 KanzeiConfig。
    // 此前 merge() 没有 cadence 分支,文件里写了也到不了运行时——config.cadence
    // 恒为默认(复现实证:全仓 grep 除 settings/config 定义外零消费方)。
    #[test]
    fn cadence_层叠合并_显式键覆盖_缺键保持全局() {
        // 全局层:full_test=release_only + push=per_entry(显式写 per_entry)。
        let global = temp_config_dir();
        std::fs::write(
            global.join("kanzei.toml"),
            "[cadence]\nfull_test = \"release_only\"\npush = \"per_entry\"\n",
        )
        .unwrap();
        // 项目层:只显式写 full_test=every_commit,其余键不得被打回默认。
        let project = temp_config_dir();
        std::fs::create_dir_all(project.join(".kanzei")).unwrap();
        std::fs::write(
            project.join(".kanzei").join("kanzei.toml"),
            "[cadence]\nfull_test = \"every_commit\"\n",
        )
        .unwrap();
        let config = load_two_layer(&global, &project);
        // 项目层覆盖了 full_test。
        assert_eq!(config.cadence.full_test, FullTestCadence::EveryCommit);
        // 全局层的 push=per_entry 保持——项目层没写 push,不得被默认值(per_batch)覆盖。
        assert_eq!(config.cadence.push, PushCadence::PerEntry);
        // 全局层的默认仍生效:targeted_test 没在任一层显式写 → §1.4 默认。
        assert_eq!(
            config.cadence.targeted_test,
            TargetedTestCadence::EveryCommit
        );
        std::fs::remove_dir_all(global).ok();
        std::fs::remove_dir_all(project).ok();
    }

    fn temp_config_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-config-cadence-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn load_two_layer(global: &std::path::Path, project: &std::path::Path) -> KanzeiConfig {
        // 模拟 load_with_warnings_at_root 的全局+项目层叠,但两个目录都显式可控。
        let mut config = KanzeiConfig::default();
        let mut warnings = Vec::new();
        merge_file(&mut config, &global.join("kanzei.toml"), &mut warnings).unwrap();
        merge_file(
            &mut config,
            &project.join(".kanzei").join("kanzei.toml"),
            &mut warnings,
        )
        .unwrap();
        config
    }

    #[test]
    fn 合并覆盖全部_models_字段_未设的键不清空() {
        // reasoning 曾被漏掉:同一个 [models] 表里 primary 生效而 reasoning 不生效,
        // 是最难查的那类不一致。
        let mut base: KanzeiConfig = toml::from_str(
            r#"
[models]
primary = "anthropic:claude-sonnet-5"
fast = "ollama:qwen3"
reasoning = "low"
"#,
        )
        .unwrap();
        let layer: KanzeiConfig = toml::from_str(
            r#"
[models]
reasoning = "high"
"#,
        )
        .unwrap();
        merge(&mut base, layer);
        assert_eq!(
            base.models.reasoning.as_deref(),
            Some("high"),
            "项目级思考强度未生效"
        );
        // 层里没设的键不得被清空——空 [models] 表不该抹掉全局配置。
        assert_eq!(
            base.models.primary.as_deref(),
            Some("anthropic:claude-sonnet-5")
        );
        assert_eq!(base.models.fast.as_deref(), Some("ollama:qwen3"));
    }

    #[test]
    fn cadence_缺节等于_conventions_1_4_默认() {
        // 验收②:旧 kanzei.toml 没有 [cadence] 节时,行为必须与 conventions §1.4
        // 当前默认逐项一致(serde default 兜底)。
        let empty: KanzeiConfig = toml::from_str("").unwrap();
        assert_eq!(empty.cadence.full_test, FullTestCadence::EntryClose);
        assert_eq!(empty.cadence.full_test_batches, None);
        assert_eq!(
            empty.cadence.targeted_test,
            TargetedTestCadence::EveryCommit
        );
        assert_eq!(empty.cadence.commit, CommitCadence::PerBatch);
        assert_eq!(empty.cadence.push, PushCadence::PerEntry);

        // 只写 [models] 的旧配置同样不引入 cadence 行为变化。
        let old: KanzeiConfig =
            toml::from_str("[models]\nprimary = \"anthropic:claude-sonnet-5\"\n").unwrap();
        assert_eq!(old.cadence.full_test, FullTestCadence::EntryClose);
        assert_eq!(old.cadence.targeted_test, TargetedTestCadence::EveryCommit);
        assert_eq!(old.cadence.push, PushCadence::PerEntry);
    }

    #[test]
    fn cadence_各档位解析与序列化() {
        // 验收①的配置侧:所有档位都能从 toml 解析并 round-trip 回原样。
        let config: KanzeiConfig = toml::from_str(
            "[cadence]\nfull_test = \"every_n_batches\"\nfull_test_batches = 3\n\
             targeted_test = \"off\"\ncommit = \"per_entry\"\npush = \"periodic\"\n",
        )
        .unwrap();
        assert_eq!(config.cadence.full_test, FullTestCadence::EveryNBatches);
        assert_eq!(config.cadence.full_test_batches, Some(3));
        assert_eq!(config.cadence.targeted_test, TargetedTestCadence::Off);
        assert_eq!(config.cadence.commit, CommitCadence::PerEntry);
        assert_eq!(config.cadence.push, PushCadence::Periodic);

        // release_only / every_commit 两种极端档位。
        let config: KanzeiConfig =
            toml::from_str("[cadence]\nfull_test = \"release_only\"\n").unwrap();
        assert_eq!(config.cadence.full_test, FullTestCadence::ReleaseOnly);
        let config: KanzeiConfig =
            toml::from_str("[cadence]\nfull_test = \"every_commit\"\n").unwrap();
        assert_eq!(config.cadence.full_test, FullTestCadence::EveryCommit);

        // 序列化 round-trip:设置页保存后重读不丢字段。
        let round: KanzeiConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(round.cadence.full_test, FullTestCadence::EveryCommit);
        assert_eq!(
            round.cadence.targeted_test,
            TargetedTestCadence::EveryCommit
        );

        // 非法档位要报错,不能静默吞成默认——否则设置页拼错字用户还以为是默认生效。
        assert!(toml::from_str::<KanzeiConfig>("[cadence]\nfull_test = \"daily\"\n").is_err());
    }
}
