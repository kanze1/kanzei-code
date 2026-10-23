//! 名字键控、保序、同名 last-wins 的注册表，供 agents/tools/skills/context 共用。
//! 保序=注入提示词的顺序确定,同名替换原位=覆盖不打乱既有顺序。

use std::path::PathBuf;

/// 注册动作的来源。由 Harness 装配自动填组件,文件组件补充原文路径。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RegistrationOrigin {
    pub component: Option<String>,
    pub path: Option<PathBuf>,
}

/// 当前条目的胜者与按发生顺序排列的替换链,不包含被覆盖的运行时对象。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RegistrationReport {
    pub name: String,
    pub winner: RegistrationOrigin,
    pub replaced: Vec<RegistrationOrigin>,
}

struct Registered<T> {
    name: String,
    value: T,
    origins: Vec<RegistrationOrigin>,
}

pub struct Registry<T> {
    items: Vec<Registered<T>>,
    component: Option<&'static str>,
}

impl<T> Default for Registry<T> {
    fn default() -> Self {
        Registry {
            items: Vec::new(),
            component: None,
        }
    }
}

impl<T> Registry<T> {
    pub fn insert(&mut self, name: impl Into<String>, item: T) {
        self.insert_with_origin(name.into(), item, None);
    }

    pub fn insert_at(&mut self, name: impl Into<String>, item: T, path: PathBuf) {
        self.insert_with_origin(name.into(), item, Some(path));
    }

    pub(crate) fn set_component(&mut self, component: &'static str) {
        self.component = Some(component);
    }

    fn insert_with_origin(&mut self, name: String, item: T, path: Option<PathBuf>) {
        let origin = RegistrationOrigin {
            component: self.component.map(str::to_string),
            path,
        };
        match self.items.iter_mut().find(|entry| entry.name == name) {
            Some(entry) => {
                entry.value = item;
                entry.origins.push(origin);
            }
            None => self.items.push(Registered {
                name,
                value: item,
                origins: vec![origin],
            }),
        }
    }

    pub fn remove(&mut self, name: &str) -> Option<T> {
        let pos = self.items.iter().position(|entry| entry.name == name)?;
        Some(self.items.remove(pos).value)
    }

    pub fn get(&self, name: &str) -> Option<&T> {
        self.items
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| &entry.value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.items
            .iter()
            .map(|entry| (entry.name.as_str(), &entry.value))
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(|entry| entry.name.as_str())
    }

    pub fn report(&self) -> Vec<RegistrationReport> {
        self.items
            .iter()
            .map(|entry| {
                let (winner, replaced) = entry
                    .origins
                    .split_last()
                    .expect("registered entry has an origin");
                RegistrationReport {
                    name: entry.name.clone(),
                    winner: winner.clone(),
                    replaced: replaced.to_vec(),
                }
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_wins_keeps_position() {
        let mut r = Registry::default();
        r.insert("a", 1);
        r.insert("b", 2);
        r.insert("a", 3);
        let collected: Vec<_> = r.iter().map(|(n, v)| (n.to_string(), *v)).collect();
        assert_eq!(collected, vec![("a".to_string(), 3), ("b".to_string(), 2)]);
    }

    #[test]
    fn registration_report_preserves_winner_and_ordered_override_chain() {
        let mut registry = Registry::default();
        registry.set_component("builtin::Agents");
        registry.insert("agent", 1);
        registry.insert("other", 2);
        registry.set_component("markdown::MarkdownComponent");
        registry.insert_at("agent", 3, PathBuf::from("global/agents/agent.md"));
        registry.insert_at("agent", 4, PathBuf::from("project/.kanzei/agents/agent.md"));
        let report = registry.report();
        assert_eq!(
            report
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["agent", "other"]
        );
        assert_eq!(registry.get("agent"), Some(&4));
        assert_eq!(report[0].replaced.len(), 2);
        assert_eq!(
            report[0].replaced[0].component.as_deref(),
            Some("builtin::Agents")
        );
        assert_eq!(
            report[0].replaced[1].path,
            Some(PathBuf::from("global/agents/agent.md"))
        );
        assert_eq!(
            report[0].winner.path,
            Some(PathBuf::from("project/.kanzei/agents/agent.md"))
        );
        assert_eq!(
            report[0].winner.component.as_deref(),
            Some("markdown::MarkdownComponent")
        );
        assert_eq!(registry.remove("agent"), Some(4));
        assert_eq!(registry.report().len(), 1);
        registry.insert("agent", 5);
        assert!(registry.report()[1].replaced.is_empty());
    }
}
