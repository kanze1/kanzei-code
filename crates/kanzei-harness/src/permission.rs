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
}

impl Ruleset {
    pub fn new(rules: Vec<Rule>) -> Self {
        Ruleset { rules }
    }

    pub fn push(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    pub fn extend(&mut self, rules: impl IntoIterator<Item = Rule>) {
        self.rules.extend(rules);
    }

    /// last-match-wins;无匹配 → Ask。
    pub fn evaluate(&self, action: &str, resource: &str) -> Effect {
        self.rules
            .iter()
            .rev()
            .find(|r| wildcard_match(&r.action, action) && wildcard_match(&r.resource, resource))
            .map(|r| r.effect)
            .unwrap_or(Effect::Ask)
    }

    /// 某 action 是否被整体 deny(resource "*")——materialize 时直接摘掉该工具。
    pub fn action_fully_denied(&self, action: &str) -> bool {
        matches!(self.evaluate(action, "*"), Effect::Deny)
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }
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
        assert!(wildcard_match("*.kanzei/project/*", "x/.kanzei/project/requirements.md"));
    }

    #[test]
    fn last_match_wins_default_ask() {
        let rs = Ruleset::new(vec![
            Rule { action: "bash".into(), resource: "*".into(), effect: Effect::Allow },
            Rule { action: "bash".into(), resource: "rm *".into(), effect: Effect::Deny },
        ]);
        assert_eq!(rs.evaluate("bash", "git status"), Effect::Allow);
        assert_eq!(rs.evaluate("bash", "rm -rf /"), Effect::Deny);
        assert_eq!(rs.evaluate("write", "a.txt"), Effect::Ask);
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
