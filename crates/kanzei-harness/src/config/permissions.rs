//! 权限配置域(R-257 B5):ProfileSection/PermissionsSection/NonInteractive 结构与
//! KanzeiConfig 的权限相关方法(legacy bash 规则/非交互策略/启动告警)。
//! 自 config.rs 原样迁出,零行为变更。

use serde::{Deserialize, Serialize};

use crate::config::KanzeiConfig;
use crate::permission::{is_structured_bash_resource, normalize_resource, Rule, BASH_ACTION};
use crate::permission_persist::{is_wildcard_resource, rule_digest};

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
    /// `Limits::barrier_timeout_secs` 就栽在只加字段没接 merge,项目层设了却静默不生效
    /// (D-300 已补,`limits_全字段_层叠往返不丢值_且名单穷举` 在防复发)。
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

    pub(crate) fn parse(text: &str) -> Option<NonInteractive> {
        let key = text.trim().to_ascii_lowercase();
        Self::KNOWN
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
    }

    pub(crate) fn known_names() -> String {
        Self::KNOWN
            .iter()
            .map(|(name, _)| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(" / ")
    }
}

impl KanzeiConfig {
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
}

/// kanzei.toml [profile] / [permissions] 节已知键名单(R-220 单源)。
pub(crate) const PROFILE_KEYS: &[&str] = &["default"];
pub(crate) const PERMISSIONS_KEYS: &[&str] = &["rules", "non_interactive"];
pub(crate) const PERMISSION_RULE_KEYS: &[&str] = &["action", "resource", "effect"];
