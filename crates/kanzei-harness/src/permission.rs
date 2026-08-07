//! 权限 Ruleset:有序规则,last-match-wins,无匹配默认 Ask(照搬 opencode V2 语义)。

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Default)]
pub struct Ruleset {
    rules: Vec<Rule>,
    hard_denies: Vec<Rule>,
}

impl Ruleset {
    pub fn new(rules: Vec<Rule>) -> Self {
        Ruleset {
            rules,
            hard_denies: Vec::new(),
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

    pub fn extend(&mut self, rules: impl IntoIterator<Item = Rule>) {
        self.rules.extend(rules);
    }

    /// 硬 deny 优先；普通规则保持 last-match-wins，无匹配 → Ask。
    pub fn evaluate(&self, action: &str, resource: &str) -> Effect {
        if self.hard_denies.iter().any(|r| {
            wildcard_match(&r.action, action) && resource_match_for_action(action, &r.resource, resource)
        }) {
            return Effect::Deny;
        }
        let matched =
            self.rules.iter().rev().find(|r| {
                wildcard_match(&r.action, action) && resource_match_for_action(action, &r.resource, resource)
            });
        let Some(rule) = matched else {
            return Effect::Ask;
        };
        if rule.effect == Effect::Allow
            && command_chaining_escapes(action, resource, &rule.resource)
        {
            return Effect::Ask;
        }
        rule.effect
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

/// 按 action 选择资源语义；bash 命令是 shell 语法文本，不能因其中出现 `/` 就走文件路径规范化。
pub fn resource_match_for_action(action: &str, pattern: &str, value: &str) -> bool {
    if action == "bash" {
        wildcard_match(pattern, value)
    } else {
        resource_match(pattern, value)
    }
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
    action == "bash" && pattern != "*" && pattern.contains('*') && !resource.is_empty()
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
        // 旧的首词通配规则也不能放行重定向、解释器入口或其他子命令。
        for command in [
            "git status",
            "git status > .kanzei/project/requirements.md",
            "git -c alias.x=!calc x",
            "python -c open_secret",
            "pwsh -Command Set-Content secret x",
        ] {
            assert_eq!(rs.evaluate("bash", command), Effect::Ask, "command={command}");
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
        let resource = r#"{"command":"git status > .kanzei/project/requirements.md","workdir":"subdir"}"#;
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
}
