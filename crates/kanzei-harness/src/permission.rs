//! 权限 Ruleset:有序规则,last-match-wins,无匹配默认 Ask(照搬 opencode V2 语义)。

use serde::{Deserialize, Serialize};

/// bash 工具的权限 action 名。**这是 D-269 全部分流判据的唯一出处**——
/// [`normalize_resource_for_action`] 与 [`resource_match_for_action`] 都靠它决定
/// 「这条资源是 shell 文本还是文件路径」。
///
/// 它与 `kanzei_tools::BashTool` 之间是**跨 crate 的字面量耦合**:`Tool::action()` 默认
/// 返回 `Tool::name()`,所以把 bash 工具改名(或给它单独实现一个 `action()`)会让这里
/// 静默走进路径分支——也就是静默重新打开 D-269,而且没有任何测试会红。
///
/// 本 crate 是 `kanzei-tools` 的**上游**,拿不到 `BashTool` 来断言,所以钉住这条耦合的
/// 用例放在下游:`crates/kanzei/tests/bash_action_literal.rs`。改这里的值 = 必须同时改那条用例。
pub const BASH_ACTION: &str = "bash";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub action: String,
    pub resource: String,
    pub effect: Effect,
}

/// 硬 deny 的资源族与它的合法替代路径(D-173)。
///
/// 硬 deny 只说"不准这样写",不说"那该怎么写"。缺了后半句就是能力死区:
/// 模型确认没有合法通道后不会停手,而是去找旁路(shell 重定向、.NET
/// WriteAllText、另起解释器),于是安全边界形同虚设。因此每一条硬 deny
/// 都必须显式声明它对应的专用工具;确实还没实现的,也必须显式声明为
/// `required_tool: None`,让拒绝理由如实说成"能力未实现"而不是编一个不存在的工具。
#[derive(Debug, Clone)]
pub struct ManagedResource {
    pub action: String,
    pub resource: String,
    /// 合法写通道的工具名;None = 该资源族目前没有任何合法写通道。
    pub required_tool: Option<String>,
    /// 给模型的一句话说明(为什么托管、该怎么做)。
    pub note: Option<String>,
    /// true = 这是一条有意的能力开关说明,不是“专用工具尚未实现”。
    ///
    /// F11 的 tracker 写入开关会拒绝同一个 tracker 工具里的写动作,但读取仍然
    /// 合法,也不存在另一个替代工具。若仍把 `required_tool: None` 解释成能力缺口,
    /// 模型会收到完全错误的“记录 defect 后跳过”指引。
    pub note_only: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Ruleset {
    rules: Vec<Rule>,
    hard_denies: Vec<Rule>,
    managed: Vec<ManagedResource>,
}

impl Ruleset {
    pub fn new(rules: Vec<Rule>) -> Self {
        Ruleset {
            rules,
            hard_denies: Vec::new(),
            managed: Vec::new(),
        }
    }

    pub fn push(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// 添加不可被普通配置规则覆盖的 deny，用于 profile 的安全边界。
    pub fn push_hard_deny(&mut self, rule: Rule) {
        debug_assert_eq!(rule.effect, Effect::Deny);
        self.hard_denies.push(rule);
    }

    /// 硬 deny + 合法替代路径声明。resolve 时会校验 required_tool 真的注册了。
    pub fn push_managed_hard_deny(
        &mut self,
        rule: Rule,
        required_tool: Option<&str>,
        note: Option<&str>,
    ) {
        self.managed.push(ManagedResource {
            action: rule.action.clone(),
            resource: rule.resource.clone(),
            required_tool: required_tool.map(str::to_string),
            note: note.map(str::to_string),
            note_only: false,
        });
        self.push_hard_deny(rule);
    }

    /// 普通 deny + 精确拒绝理由。
    ///
    /// 与 managed hard deny 不同,它不声明替代工具、也不把能力说成未实现;
    /// 后续普通 allow 仍可按 last-match-wins 覆盖它。用于显式用户开关。
    pub fn push_denial_note(&mut self, rule: Rule, note: &str) {
        debug_assert_eq!(rule.effect, Effect::Deny);
        self.managed.push(ManagedResource {
            action: rule.action.clone(),
            resource: rule.resource.clone(),
            required_tool: None,
            note: Some(note.to_string()),
            note_only: true,
        });
        self.push(rule);
    }

    pub fn managed_resources(&self) -> &[ManagedResource] {
        &self.managed
    }

    /// 命中的托管资源族(用于把拒绝理由换成"该走哪个工具")。
    pub fn managed_for(&self, action: &str, resource: &str) -> Option<&ManagedResource> {
        self.managed.iter().find(|m| {
            wildcard_match(&m.action, action)
                && resource_match_for_action(action, &m.resource, resource)
        })
    }

    pub fn extend(&mut self, rules: impl IntoIterator<Item = Rule>) {
        self.rules.extend(rules);
    }

    /// 硬 deny 优先；普通规则保持 last-match-wins，无匹配 → Ask。
    pub fn evaluate(&self, action: &str, resource: &str) -> Effect {
        if self.hard_denies.iter().any(|r| {
            wildcard_match(&r.action, action)
                && resource_match_for_action(action, &r.resource, resource)
        }) {
            return Effect::Deny;
        }
        let matched = self.rules.iter().rev().find(|r| {
            wildcard_match(&r.action, action)
                && resource_match_for_action(action, &r.resource, resource)
        });
        let Some(rule) = matched else {
            return Effect::Ask;
        };
        if rule.effect == Effect::Allow
            && command_chaining_escapes(action, resource, &rule.resource)
        {
            // D-051:通配规则默认降级 Ask。R-198:若规则是「程序名+参数前缀」
            // 形态且命令安全匹配(程序名一致、无 shell 结构、参数前缀通配命中),
            // 则放行——这是显式的中间档位,不再是整串通配的隐患。
            if !(action == BASH_ACTION && bash_prefix_match(&rule.resource, resource)) {
                return Effect::Ask;
            }
        }
        rule.effect
    }

    /// R-183:与 [`evaluate`] 同一判定,额外返回命中的普通规则原文(last-match-wins)。
    ///
    /// - 硬 deny 优先,此时无普通规则可归属 → `(Deny, None)`;
    /// - 无普通规则匹配 → `(Ask, None)`;
    /// - 命中普通规则 → `(effect, Some(rule))`;D-051 把 allow 降级为 Ask 时规则
    ///   仍如实返回(它确实是"命中的规则",只是被 chaining 防线挡了)。
    pub fn evaluate_with_rule(&self, action: &str, resource: &str) -> (Effect, Option<&Rule>) {
        if self.hard_denies.iter().any(|r| {
            wildcard_match(&r.action, action)
                && resource_match_for_action(action, &r.resource, resource)
        }) {
            return (Effect::Deny, None);
        }
        let matched = self.rules.iter().rev().find(|r| {
            wildcard_match(&r.action, action)
                && resource_match_for_action(action, &r.resource, resource)
        });
        let Some(rule) = matched else {
            return (Effect::Ask, None);
        };
        if rule.effect == Effect::Allow
            && command_chaining_escapes(action, resource, &rule.resource)
        {
            // D-051:通配规则默认降级 Ask。R-198:若规则是「程序名+参数前缀」
            // 形态且命令安全匹配(程序名一致、无 shell 结构、参数前缀通配命中),
            // 则放行——这是显式的中间档位,不再是整串通配的隐患。
            if !(action == BASH_ACTION && bash_prefix_match(&rule.resource, resource)) {
                return (Effect::Ask, Some(rule));
            }
        }
        (rule.effect, Some(rule))
    }

    /// 某 action 是否被整体 deny(resource "*")——materialize 时直接摘掉该工具。
    pub fn action_fully_denied(&self, action: &str) -> bool {
        matches!(self.evaluate(action, "*"), Effect::Deny)
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }
}

/// 路径类资源的规范化:统一分隔符,消解 `.` 与 `..` 段,去掉重复斜杠。
/// Windows 下同时做大小写折叠,覆盖盘符、UNC 与大小写变体;无路径分隔符的
/// 资源(bash 命令等)原样返回。权限决策和文件工具落点都必须使用本函数(D-050)。
pub fn normalize_resource(resource: &str) -> String {
    if !resource.contains('/') && !resource.contains('\\') {
        return resource.to_string();
    }
    let resource = resource.replace('\\', "/");
    let absolute = resource.starts_with('/') || is_windows_drive_path(&resource);
    let unc = resource.starts_with("//");
    let prefix = if is_windows_drive_path(&resource) {
        resource[..2].to_string()
    } else if unc {
        "//".to_string()
    } else {
        String::new()
    };
    let body = if is_windows_drive_path(&resource) {
        &resource[2..]
    } else if unc {
        resource.trim_start_matches('/')
    } else {
        resource.as_str()
    };
    let mut segments: Vec<&str> = Vec::new();
    for segment in body.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if matches!(segments.last(), Some(&last) if last != "..") {
                    segments.pop();
                } else if !absolute {
                    segments.push("..");
                }
            }
            other => segments.push(other),
        }
    }
    let mut joined = segments.join("/");
    if cfg!(windows) {
        joined = joined.to_lowercase();
    }
    let trailing = resource.ends_with('/') && !joined.is_empty();
    let result = if unc {
        format!("//{joined}")
    } else if is_windows_drive_path(&resource) {
        if cfg!(windows) {
            format!("{}/{joined}", prefix.to_lowercase())
        } else {
            format!("{prefix}/{joined}")
        }
    } else if absolute {
        format!("/{joined}")
    } else {
        joined
    };
    if trailing {
        format!("{result}/")
    } else {
        result
    }
}

fn is_windows_drive_path(resource: &str) -> bool {
    resource.len() >= 2
        && resource.as_bytes()[1] == b':'
        && resource.as_bytes()[0].is_ascii_alphabetic()
}

/// 按 action 选择资源**规范化**语义:`bash` 的资源是 shell 文本(`BashTool::resources_with_ctx`
/// 造出的 `{"command":…,"workdir":…}` JSON 串,其中 workdir 已单独规范化过),**原样返回**;
/// 其余 action 仍走 `normalize_resource`(D-050 的路径语义,一字不动)。
///
/// D-269:`normalize_resource` 为路径语义**故意设计成非单射**——弹掉 `..` 的前一段、折叠
/// `//` 与 `/./`、`\`→`/`、Windows 下整串小写。把它施加到 bash 文本上,一条规则准入的就不再是
/// 一条命令,而是该命令在 `normalize_resource` 下的**整个原像类**:在已批准命令的任一 `/` 处
/// 写成 `T/../`(T 为任意不含 `/` 的串)即可把任意 shell 语句藏进 T,规范化后与原命令逐字节
/// 相等,判定 Allow,而 `bash.rs` 执行的是未经规范化的原始命令文本。
///
/// 论证方向必须写对:授权需要的是「一个 pattern 只准入一个 value」的**单射性**,不是
/// 「一个 value 只算出一个规范化结果」的函数性——后者对任何确定性函数都恒成立,与授权无关。
/// 因此这里不做任何「两侧都规范化后比较」的兼容处理:那只会把准入集从 `{V : N(V)==P}` 换成
/// `{V : N(V)==N(P)}`,原像类一个不少。
pub fn normalize_resource_for_action(action: &str, resource: &str) -> String {
    if action == BASH_ACTION {
        return resource.to_string();
    }
    normalize_resource(resource)
}

/// bash 资源的**结构化**形态判定:`BashTool::resources_with_ctx` 产出的
/// `{"command":…,"workdir":…}` JSON 串。判据只有一条(全仓唯一实现,`config.rs` 的
/// 历史规则分类也走它,免得两处判据漂移):可解析成 JSON 且同时带 `command` 与 `workdir`。
pub fn is_structured_bash_resource(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .is_some_and(|json| json.get("command").is_some() && json.get("workdir").is_some())
}

/// 按 action 选择资源语义；bash 命令是 shell 语法文本，不能因其中出现 `/` 就走文件路径规范化。
pub fn resource_match_for_action(action: &str, pattern: &str, value: &str) -> bool {
    if action == BASH_ACTION {
        if pattern == "*" {
            return wildcard_match(pattern, value);
        }
        // 旧的纯字符串规则不得授权结构化请求:两者语义不同,`./scripts/release.ps1` 这类
        // pattern 一旦被允许去匹配 `{"command":…}` 串,授权范围就变成了整个 JSON 文本空间。
        return if is_structured_bash_resource(value) && !is_structured_bash_resource(pattern) {
            false
        } else {
            wildcard_match(pattern, value)
        };
    }
    resource_match(pattern, value)
}

/// 按资源类型匹配权限规则；路径资源先经过统一规范化，避免会话规则
/// 与 Ruleset::evaluate 使用不同的路径语义(D-050)。
pub fn resource_match(pattern: &str, value: &str) -> bool {
    if pattern.contains('/')
        || pattern.contains('\\')
        || value.contains('/')
        || value.contains('\\')
    {
        wildcard_match(&normalize_resource(pattern), &normalize_resource(value))
    } else {
        wildcard_match(pattern, value)
    }
}

/// 非整体 bash 通配规则无法表达命令内部的 shell/解释器语义；不自动放行，
/// 只有显式资源 `*` 的整体放行不降级(D-051)。
fn command_chaining_escapes(action: &str, resource: &str, pattern: &str) -> bool {
    action == BASH_ACTION && pattern != "*" && pattern.contains('*') && !resource.is_empty()
}

/// R-198：bash 规则「程序名 + 参数前缀」白名单匹配（不再整串通配）。
///
/// 规则形态如 `node scripts/*.mjs`、`cargo build*`：程序名精确比较，参数部分
/// 走通配。命令串里出现 shell 链接/重定向/子 shell（`;` `&&` `||` `|` `>` `<`
/// `$(` 反引号 `&` 等）时**不匹配**——前缀规则只放行"这一个程序以这些参数
/// 直跑"，不能掩护 `git status; rm -rf /`（D-051 防线在解析层保留）。
///
/// 结构化 bash 资源（JSON）不走本函数：JSON 整串语义由 `resource_match_for_action`
/// 的既有路径处理，这里只服务纯字符串命令形态（验收③ 两形态各走各的路径）。
pub fn bash_prefix_match(pattern: &str, command: &str) -> bool {
    if pattern.trim().is_empty() || pattern == "*" || is_structured_bash_resource(command) {
        return false;
    }
    // 程序名 = 命令的第一个 token；参数 = 剩余部分。
    let Some((cmd_prog, cmd_rest)) = split_first_token(command) else {
        return false;
    };
    // 程序名精确匹配（Windows 上命令大小写不敏感,统一小写比较）。
    let Some((pat_prog, pat_rest)) = split_first_token(pattern) else {
        return false;
    };
    if !cmd_prog.eq_ignore_ascii_case(&pat_prog) {
        return false;
    }
    // 命令里出现 shell 结构 → 前缀规则不适用（D-051 防线）。
    if has_shell_meta(&cmd_rest) {
        return false;
    }
    wildcard_match(&pat_rest, &cmd_rest)
}

/// 取文本的第一个 token（程序名）与剩余部分。引号包裹的程序名（如
/// `"C:\Program Files\node.exe"`）按一个整体处理。
fn split_first_token(text: &str) -> Option<(String, String)> {
    let text = text.trim_start();
    if text.is_empty() {
        return None;
    }
    let mut quote = None;
    let mut end = 0usize;
    for (idx, ch) in text.char_indices() {
        match ch {
            '"' | '\'' => {
                if quote == Some(ch) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(ch);
                }
                end = idx + ch.len_utf8();
            }
            c if c.is_whitespace() && quote.is_none() => {
                return Some((
                    text[..idx].to_string(),
                    text[idx..].trim_start().to_string(),
                ));
            }
            _ => end = idx + ch.len_utf8(),
        }
    }
    Some((text[..end].to_string(), String::new()))
}

/// 命令剩余部分是否含 shell 链接/重定向/子 shell/历史展开字符。
/// 只要引号外出现这些字符就判定为"不是单程序直跑",前缀规则不匹配。
fn has_shell_meta(rest: &str) -> bool {
    let mut quote = None;
    for ch in rest.chars() {
        match ch {
            '"' | '\'' => {
                if quote == Some(ch) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(ch);
                }
            }
            ';' | '&' | '|' | '>' | '<' | '`' | '!' | '$' | '(' | ')' if quote.is_none() => {
                return true;
            }
            _ => {}
        }
    }
    false
}

/// `*` 通配(匹配任意串,含空);其余字符逐字比较,大小写敏感。
pub fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let val: Vec<char> = value.chars().collect();
    // 经典迭代回溯算法,O(n*m) 上界但实际线性。
    let (mut p, mut v) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while v < val.len() {
        if p < pat.len() && (pat[p] == val[v]) {
            p += 1;
            v += 1;
        } else if p < pat.len() && pat[p] == '*' {
            star = Some(p);
            mark = v;
            p += 1;
        } else if let Some(s) = star {
            p = s + 1;
            mark += 1;
            v = mark;
        } else {
            return false;
        }
    }
    while p < pat.len() && pat[p] == '*' {
        p += 1;
    }
    p == pat.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_basics() {
        assert!(wildcard_match("*", "anything"));
        assert!(wildcard_match("git *", "git status"));
        assert!(wildcard_match("git*", "git"));
        assert!(!wildcard_match("git *", "cargo build"));
        assert!(wildcard_match(
            "*.kanzei/project/*",
            "x/.kanzei/project/requirements.md"
        ));
    }

    #[test]
    fn last_match_wins_default_ask() {
        let rs = Ruleset::new(vec![
            Rule {
                action: "bash".into(),
                resource: "*".into(),
                effect: Effect::Allow,
            },
            Rule {
                action: "bash".into(),
                resource: "rm *".into(),
                effect: Effect::Deny,
            },
        ]);
        assert_eq!(rs.evaluate("bash", "git status"), Effect::Allow);
        assert_eq!(rs.evaluate("bash", "rm -rf /"), Effect::Deny);
        assert_eq!(rs.evaluate("write", "a.txt"), Effect::Ask);
    }

    #[test]
    fn 路径规范化消解穿越与冗余段() {
        // 两个 .. 正好退回项目根,落点是 src/main.rs——已在 research 目录之外
        assert_eq!(
            normalize_resource(".kanzei/research/../../src/main.rs"),
            "src/main.rs"
        );
        // 越过起点的 .. 必须保留,不能被消解成看似合法的相对路径
        assert_eq!(normalize_resource("a/../../etc/passwd"), "../etc/passwd");
        assert_eq!(
            normalize_resource(".kanzei/./project/goals.md"),
            ".kanzei/project/goals.md"
        );
        assert_eq!(
            normalize_resource(".kanzei//project/goals.md"),
            ".kanzei/project/goals.md"
        );
        assert_eq!(normalize_resource("/a/b/../c"), "/a/c");
        // 无分隔符的资源(bash 命令)原样返回
        assert_eq!(normalize_resource("git status"), "git status");
    }

    #[test]
    fn 穿越路径不再命中收窄规则() {
        // research 模式:先整体 deny 写,再放行 research 目录
        let rs = Ruleset::new(vec![
            Rule {
                action: "write".into(),
                resource: "*".into(),
                effect: Effect::Deny,
            },
            Rule {
                action: "write".into(),
                resource: "*.kanzei/research/*".into(),
                effect: Effect::Allow,
            },
        ]);
        assert_eq!(
            rs.evaluate("write", &normalize_resource(".kanzei/research/notes.md")),
            Effect::Allow
        );
        // 借 .. 穿出 research 目录必须回到 deny
        assert_eq!(
            rs.evaluate(
                "write",
                &normalize_resource(".kanzei/research/../../src/main.rs")
            ),
            Effect::Deny
        );
    }

    #[test]
    fn 项目文档硬deny不被冗余段绕过() {
        let rs = Ruleset::new(vec![
            Rule {
                action: "write".into(),
                resource: "*".into(),
                effect: Effect::Ask,
            },
            Rule {
                action: "write".into(),
                resource: "*.kanzei/project/*".into(),
                effect: Effect::Deny,
            },
        ]);
        for path in [
            ".kanzei/project/goals.md",
            ".kanzei/./project/goals.md",
            ".kanzei//project/goals.md",
            "x/.kanzei/project/../project/goals.md",
        ] {
            assert_eq!(
                rs.evaluate("write", &normalize_resource(path)),
                Effect::Deny,
                "path={path}"
            );
        }
    }

    #[test]
    fn windows_path_variants_use_same_policy_resource() {
        let rs = Ruleset::new(vec![
            Rule {
                action: "write".into(),
                resource: "*".into(),
                effect: Effect::Ask,
            },
            Rule {
                action: "write".into(),
                resource: "*.kanzei/project/*".into(),
                effect: Effect::Deny,
            },
        ]);
        for path in [
            r".KANZEI\project\requirements.md",
            r".kanzei/./PROJECT/requirements.md",
            r"C:\Workspace\.KANZEI\project\requirements.md",
            r"c:\workspace\.kanzei\project\requirements.md",
            r"\\SERVER\Share\.KANZEI\PROJECT\requirements.md",
        ] {
            let normalized = normalize_resource(path);
            assert_eq!(
                rs.evaluate("write", &normalized),
                Effect::Deny,
                "path={path}"
            );
        }
    }

    #[test]
    fn session_rule_resource_match_reuses_path_normalization() {
        let pattern = r"*.KANZEI\PROJECT\*";
        let value = normalize_resource(r".kanzei/./project/requirements.md");
        assert!(resource_match(pattern, &value));
        assert!(resource_match(
            pattern,
            &normalize_resource(r".KANZEI\project\..\project\requirements.md")
        ));
    }

    #[test]
    fn windows_paths_normalize_separator_and_parent_segments() {
        assert_eq!(
            normalize_resource(r"C:\Workspace\.KANZEI\project\..\project\goals.md"),
            if cfg!(windows) {
                "c:/workspace/.kanzei/project/goals.md"
            } else {
                "C:/Workspace/.KANZEI/project/goals.md"
            }
        );
        assert_eq!(
            normalize_resource(r"\\SERVER\Share\.KANZEI\project\goals.md"),
            if cfg!(windows) {
                "//server/share/.kanzei/project/goals.md"
            } else {
                "//SERVER/Share/.KANZEI/project/goals.md"
            }
        );
    }
    #[test]
    fn 前缀通配不放行未明确授权的命令() {
        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: "git *".into(),
            effect: Effect::Allow,
        }]);
        // R-198:程序名+参数前缀规则——`git status`(无 shell 结构、程序名精确
        // 匹配)放行;重定向、解释器入口、别名、其它程序仍 Ask(D-051 防线)。
        assert_eq!(
            rs.evaluate("bash", "git status"),
            Effect::Allow,
            "R-198:git * 应放行 git status(程序名+参数前缀白名单)"
        );
        for command in [
            "git status > .kanzei/project/requirements.md", // 重定向
            "git -c alias.x=!calc x",                       // 别名/历史展开
            "python -c open_secret",                        // 其它程序
            "pwsh -Command Set-Content secret x",           // 其它程序
            "git status; rm -rf /",                         // 命令链接
        ] {
            assert_eq!(
                rs.evaluate("bash", command),
                Effect::Ask,
                "command={command}"
            );
        }
    }

    #[test]
    fn 显式整体放行不受串联降级影响() {
        // yolo 是用户明确选择的整体放行,不应被降级。
        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: "*".into(),
            effect: Effect::Allow,
        }]);
        assert_eq!(rs.evaluate("bash", "git status; rm -rf ~"), Effect::Allow);
    }

    #[test]
    fn bash_resources_keep_shell_text_opaque_during_matching() {
        let resource =
            r#"{"command":"git status > .kanzei/project/requirements.md","workdir":"subdir"}"#;
        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: resource.into(),
            effect: Effect::Allow,
        }]);
        assert_eq!(rs.evaluate("bash", resource), Effect::Allow);
        assert_eq!(
            rs.evaluate(
                "bash",
                r#"{"command":"git status > .kanzei/project/requirements.md","workdir":"other"}"#
            ),
            Effect::Ask
        );
    }
    #[test]
    fn legacy_bash_rules_do_not_authorize_structured_resources() {
        let rs = Ruleset::new(vec![
            Rule {
                action: "bash".into(),
                resource: "git status".into(),
                effect: Effect::Allow,
            },
            Rule {
                action: "bash".into(),
                resource: "*".into(),
                effect: Effect::Allow,
            },
        ]);
        let structured = r#"{"command":"git status","workdir":"C:/project"}"#;
        assert_eq!(rs.evaluate("bash", structured), Effect::Allow);
        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: "git status".into(),
            effect: Effect::Allow,
        }]);
        assert_eq!(rs.evaluate("bash", structured), Effect::Ask);
    }

    /// R-198 验收①:程序名+参数前缀规则放行匹配命令。
    #[test]
    fn 前缀白名单_放行匹配命令() {
        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: "node scripts/*.mjs".into(),
            effect: Effect::Allow,
        }]);
        assert_eq!(
            rs.evaluate("bash", "node scripts/e2e-smoke.mjs"),
            Effect::Allow,
            "验收①:node scripts/*.mjs 应放行 node scripts/e2e-smoke.mjs"
        );
        // 参数前缀通配:cargo build* 放行 cargo build --release。
        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: "cargo build*".into(),
            effect: Effect::Allow,
        }]);
        assert_eq!(
            rs.evaluate("bash", "cargo build --release -p kanzei"),
            Effect::Allow
        );
    }

    /// R-198 验收②:含命令链接/重定向/子 shell 的命令不得命中前缀规则,回落 Ask。
    #[test]
    fn 前缀白名单_命令链接重定向回落ask() {
        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: "node scripts/*.mjs".into(),
            effect: Effect::Allow,
        }]);
        for command in [
            "node scripts/x.mjs; rm -rf /",
            "node scripts/x.mjs && whoami",
            "node scripts/x.mjs | cat",
            "node scripts/x.mjs > out.txt",
            "node scripts/x.mjs $(whoami)",
            "node scripts/x.mjs `whoami`",
        ] {
            assert_eq!(
                rs.evaluate("bash", command),
                Effect::Ask,
                "验收②:含 shell 结构必须回落 Ask: {command}"
            );
        }
    }

    /// R-198 验收③:结构化 bash 资源(JSON)与纯字符串命令两种形态都覆盖——
    /// 纯字符串规则不授权 JSON(既有保护),JSON 资源经既有整串路径。
    #[test]
    fn 前缀白名单_结构化与纯字符串双形态() {
        // 纯字符串前缀规则对纯字符串命令生效(验收①已证)。
        // 纯字符串前缀规则不得授权 JSON 资源(与 legacy_bash_rules 同源保护)。
        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: "node scripts/*.mjs".into(),
            effect: Effect::Allow,
        }]);
        assert_eq!(
            rs.evaluate(
                "bash",
                r#"{"command":"node scripts/e2e-smoke.mjs","workdir":"C:/project"}"#
            ),
            Effect::Ask,
            "纯字符串前缀规则不得授权结构化 JSON 资源"
        );
        // 结构化 JSON 资源走既有整串精确匹配路径(不因新增前缀规则而退化)。
        let structured = r#"{"command":"node scripts/e2e-smoke.mjs","workdir":"C:/project"}"#;
        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: structured.into(),
            effect: Effect::Allow,
        }]);
        assert_eq!(rs.evaluate("bash", structured), Effect::Allow);
    }

    /// R-198 验收④:D-051 既有回归保持绿——整体 `*` 放行不受降级影响,
    /// 非本程序命令仍 Ask(前缀通配不放行未明确授权的命令 测试覆盖)。
    #[test]
    fn 前缀白名单_非本程序命令仍ask() {
        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: "node scripts/*.mjs".into(),
            effect: Effect::Allow,
        }]);
        for command in [
            "python scripts/e2e-smoke.mjs", // 程序名不匹配
            "node other/x.mjs",             // 参数前缀不匹配
            "cargo build",                  // 程序名不匹配
        ] {
            assert_eq!(
                rs.evaluate("bash", command),
                Effect::Ask,
                "非本程序/前缀不匹配必须 Ask: {command}"
            );
        }
        // 整体 `*` 放行仍是 yolo,不降级(D-051)。
        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: "*".into(),
            effect: Effect::Allow,
        }]);
        assert_eq!(rs.evaluate("bash", "git status; rm -rf ~"), Effect::Allow);
    }

    /// D-269 定向反证:已批准命令的任一 `/` 换成 `T/../` 即可把任意 shell 语句藏进 T。
    /// 规范化后两条命令逐字节相等——所以判定站点一旦对 bash 资源做路径规范化,一条规则
    /// 准入的就是整个原像类。分流之后注入版必须回到 Ask。
    #[test]
    fn bash注入形态不再借历史授权提权() {
        let 已批准 = r#"{"command":"cat src/main.rs","workdir":"c:/project"}"#;
        let 注入版 = r#"{"command":"cat src/;rm -rf ~;/../main.rs","workdir":"c:/project"}"#;

        // 原像坍缩是真实存在的:这一行就是提权链本身,不是假设。
        assert_ne!(已批准, 注入版);
        assert_eq!(normalize_resource(注入版), normalize_resource(已批准));
        assert_eq!(normalize_resource(注入版), 已批准);

        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: 已批准.into(),
            effect: Effect::Allow,
        }]);
        // 判定站点看到的串由 normalize_resource_for_action 产出。drive.rs 里共三处,
        // 按**名字**记(行号会随任何一次编辑漂,原来写的 :545/:604/:764 已经全部对不上了):
        // 并行预检(can_parallel_tools 的资源循环)、并行 deny 预筛、串行门禁。
        assert_eq!(
            rs.evaluate("bash", &normalize_resource_for_action("bash", 已批准)),
            Effect::Allow,
            "被批准的那条命令本身必须仍然放行"
        );
        assert_eq!(
            rs.evaluate("bash", &normalize_resource_for_action("bash", 注入版)),
            Effect::Ask,
            "注入版必须回到询问,而不是借原像类拿到 Allow"
        );
    }

    /// 同形态第二例:`cargo --manifest-path ./x/; evil ;/../y.toml`(D-269 验收②后半)。
    #[test]
    fn cargo_manifest_path同形态注入被拦() {
        let 已批准 = r#"{"command":"cargo --manifest-path ./x/y.toml","workdir":"c:/project"}"#;
        let 注入版 =
            r#"{"command":"cargo --manifest-path ./x/; evil ;/../y.toml","workdir":"c:/project"}"#;

        assert_ne!(已批准, 注入版);
        assert_eq!(normalize_resource(注入版), 已批准);

        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: 已批准.into(),
            effect: Effect::Allow,
        }]);
        assert_eq!(
            rs.evaluate("bash", &normalize_resource_for_action("bash", 已批准)),
            Effect::Allow
        );
        assert_eq!(
            rs.evaluate("bash", &normalize_resource_for_action("bash", 注入版)),
            Effect::Ask
        );
    }

    /// D-269 验收⑤:注入段里的 `*` 曾在 pattern 成形前就被 `..` 整段弹掉,于是 D-051 的
    /// `command_chaining_escapes` 拿不到 `*`、降级不触发。drive.rs 落 session_rule 时用的
    /// 正是同一个分流函数,bash 走原样后 `*` 活到 pattern 里,降级重新生效。
    #[test]
    fn d051串联降级在注入形态下重新生效() {
        let 注入值 = r#"{"command":"cat src/;rm -rf * ;/../main.rs","workdir":"c:/project"}"#;

        // 旧路径:pattern 由 normalize_resource 产出,`*` 连同整段被弹掉。
        let 旧pattern = normalize_resource(注入值);
        assert!(
            !旧pattern.contains('*'),
            "旧路径下 `*` 被 `..` 弹掉,降级无从触发:{旧pattern}"
        );
        let 旧规则 = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: 旧pattern.clone(),
            effect: Effect::Allow,
        }]);
        assert_eq!(
            旧规则.evaluate("bash", &旧pattern),
            Effect::Allow,
            "这一行是被修掉的旧行为的见证,不是期望行为"
        );

        // 新路径:pattern 就是原始命令文本,`*` 还在。
        let 新pattern = normalize_resource_for_action("bash", 注入值);
        assert_eq!(新pattern, 注入值);
        assert!(新pattern.contains('*'));
        let 新规则 = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: 新pattern.clone(),
            effect: Effect::Allow,
        }]);
        assert_eq!(
            新规则.evaluate("bash", &normalize_resource_for_action("bash", 注入值)),
            Effect::Ask,
            "含 `*` 的非整体 bash 规则必须被 D-051 降级成询问"
        );
    }

    /// bash 资源进 evaluate 时与 BashTool 的产出逐字节相同:JSON 转义的 `\"` 不被折成 `/"`,
    /// 大小写不被折叠(workdir 的规范化由 bash.rs 单独负责,这里不能再来一次)。
    #[test]
    fn bash资源进evaluate时逐字节等于工具产出() {
        for resource in [
            r#"{"command":"git commit -m \"fix: A//B\"","workdir":"c:/project"}"#,
            r#"{"command":"Select-String -Path Crates/App.rs -Pattern X","workdir":"c:/project"}"#,
            r#"{"command":"cat a/../b","workdir":"c:/project"}"#,
        ] {
            assert_eq!(normalize_resource_for_action("bash", resource), resource);
        }
    }

    /// 只动了 bash 分支:路径类 action 的规范化逐字节不变(D-269 验收④的直接落点)。
    #[test]
    fn 路径类action的规范化分流一字不变() {
        for action in ["write", "edit", "read", "glob", "grep", "task"] {
            for resource in [
                r".KANZEI\project\..\project\goals.md",
                "src//main.rs",
                "./scripts/release.ps1",
                r"C:\Workspace\.KANZEI\project\goals.md",
                "plain-token",
            ] {
                assert_eq!(
                    normalize_resource_for_action(action, resource),
                    normalize_resource(resource),
                    "action={action} resource={resource}"
                );
            }
        }
    }

    /// 历史非结构化 bash 规则的处置:不做迁移。`./scripts/release.ps1` 这类 pattern 无论
    /// 是否规范化,都被 [`resource_match_for_action`] 里那道「结构化 value 不接受非结构化
    /// pattern」的 gate 挡在结构化请求之外(按名字记,别写行号)——迁移改不动这个事实,只会
    /// 把一条今天命不中的规则改成能命中,那是替用户扩大授权,方向反了。
    #[test]
    fn 非结构化历史pattern对结构化请求恒不命中() {
        let value = r#"{"command":"./scripts/release.ps1 -skiptests","workdir":"c:/project"}"#;
        for pattern in [
            "./scripts/release.ps1",
            "scripts/release.ps1",
            "git *",
            "get-childitem *",
        ] {
            let rs = Ruleset::new(vec![Rule {
                action: "bash".into(),
                resource: pattern.into(),
                effect: Effect::Allow,
            }]);
            assert_eq!(
                rs.evaluate("bash", &normalize_resource_for_action("bash", value)),
                Effect::Ask,
                "pattern={pattern}"
            );
            assert!(
                !resource_match_for_action("bash", pattern, value),
                "pattern={pattern}"
            );
        }
    }

    #[test]
    fn hard_deny_cannot_be_overridden_by_later_normal_rules() {
        let mut rs = Ruleset::new(vec![Rule {
            action: "write".into(),
            resource: "*".into(),
            effect: Effect::Allow,
        }]);
        rs.push_hard_deny(Rule {
            action: "write".into(),
            resource: "*.kanzei/project/*".into(),
            effect: Effect::Deny,
        });
        rs.push(Rule {
            action: "write".into(),
            resource: "*.kanzei/project/*".into(),
            effect: Effect::Ask,
        });
        assert_eq!(
            rs.evaluate("write", ".KANZEI\\project\\requirements.md"),
            Effect::Deny
        );
        rs.push(Rule {
            action: "write".into(),
            resource: "*.kanzei/project/*".into(),
            effect: Effect::Allow,
        });
        assert_eq!(
            rs.evaluate("write", ".kanzei/project/requirements.md"),
            Effect::Deny
        );
    }

    #[test]
    fn managed_hard_deny_carries_its_legal_alternative() {
        // D-173:硬 deny 必须同时回答"那该走哪条路",否则模型会去找旁路。
        let mut rs = Ruleset::default();
        rs.push_managed_hard_deny(
            Rule {
                action: "write".into(),
                resource: "*.kanzei/project/defects.md".into(),
                effect: Effect::Deny,
            },
            Some("defect"),
            Some("缺陷条目由引擎分配 ID"),
        );
        rs.push_managed_hard_deny(
            Rule {
                action: "write".into(),
                resource: "*.kanzei/project/*".into(),
                effect: Effect::Deny,
            },
            None,
            None,
        );
        assert_eq!(
            rs.evaluate("write", ".kanzei/project/defects.md"),
            Effect::Deny
        );
        let managed = rs
            .managed_for("write", &normalize_resource(".kanzei/project/defects.md"))
            .expect("命中托管族");
        assert_eq!(managed.required_tool.as_deref(), Some("defect"));
        // 只被兜底族命中的资源:必须如实说成"没有专用工具",不能编一个。
        let fallback = rs
            .managed_for(
                "write",
                &normalize_resource(".kanzei/project/architecture/README.md"),
            )
            .expect("命中兜底族");
        assert_eq!(fallback.required_tool, None);
        assert!(rs.managed_for("write", "src/main.rs").is_none());
    }

    #[test]
    fn denial_note_rejects_only_its_resource_without_removing_the_action() {
        let mut rs = Ruleset::default();
        rs.push_denial_note(
            Rule {
                action: "requirement".into(),
                resource: "write:*".into(),
                effect: Effect::Deny,
            },
            "当前线未开启 tracker 写入",
        );
        assert_eq!(rs.evaluate("requirement", "write:add"), Effect::Deny);
        assert_eq!(rs.evaluate("requirement", "read:list"), Effect::Ask);
        assert!(
            !rs.action_fully_denied("requirement"),
            "只禁写不能让 materialize 把整个 tracker 工具摘掉"
        );
        let managed = rs
            .managed_for("requirement", "write:add")
            .expect("写资源应携带明确拒绝理由");
        assert!(managed.note_only);
        assert_eq!(managed.note.as_deref(), Some("当前线未开启 tracker 写入"));
    }

    #[test]
    fn hard_deny_participates_in_fully_denied_action() {
        let mut rs = Ruleset::default();
        rs.push_hard_deny(Rule {
            action: "write".into(),
            resource: "*".into(),
            effect: Effect::Deny,
        });
        assert!(rs.action_fully_denied("write"));
        assert!(!rs.action_fully_denied("edit"));
    }
    #[test]
    fn fully_denied_action() {
        let rs = Ruleset::new(vec![Rule {
            action: "task".into(),
            resource: "*".into(),
            effect: Effect::Deny,
        }]);
        assert!(rs.action_fully_denied("task"));
        assert!(!rs.action_fully_denied("bash"));
    }

    // ══ R-183:评估带命中规则原文(验收④轨迹)══

    #[test]
    fn evaluate_with_rule_命中返回规则原文() {
        let rs = Ruleset::new(vec![
            Rule {
                action: "bash".into(),
                resource: "git *".into(),
                effect: Effect::Allow,
            },
            Rule {
                action: "bash".into(),
                resource: "git status".into(),
                effect: Effect::Allow,
            },
        ]);
        let (effect, rule) = rs.evaluate_with_rule("bash", "git status");
        assert_eq!(effect, Effect::Allow);
        // last-match-wins:后注册的 `git status` 命中。
        let rule = rule.expect("命中规则应返回");
        assert_eq!(rule.resource, "git status");
        assert_eq!(rule.action, "bash");
        assert_eq!(rule.effect, Effect::Allow);
    }

    #[test]
    fn evaluate_with_rule_硬deny无普通规则归属() {
        // 硬 deny 优先:hard_denies 里的规则先判。
        let mut rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: "git status".into(),
            effect: Effect::Allow,
        }]);
        rs.push_hard_deny(Rule {
            action: "bash".into(),
            resource: "*".into(),
            effect: Effect::Deny,
        });
        let (effect, rule) = rs.evaluate_with_rule("bash", "git status");
        assert_eq!(effect, Effect::Deny);
        assert!(rule.is_none(), "硬 deny 无普通规则可归属");
    }

    #[test]
    fn evaluate_with_rule_无匹配返回_ask且无规则() {
        let rs = Ruleset::new(vec![Rule {
            action: "bash".into(),
            resource: "git *".into(),
            effect: Effect::Allow,
        }]);
        let (effect, rule) = rs.evaluate_with_rule("bash", "cargo test");
        assert_eq!(effect, Effect::Ask);
        assert!(rule.is_none());
    }
}
