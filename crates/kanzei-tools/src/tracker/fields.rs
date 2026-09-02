//! Tracker 字段词表与消费者元数据(R-356 B1)。
//!
//! 已知字段由这里集中登记；未知字段保持宽容写入，但在结构化读取中显式标灰并计数。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldCategory {
    Engine,
    Scheduling,
    Narrative,
}

impl FieldCategory {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::Scheduling => "scheduling",
            Self::Narrative => "narrative",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FieldDefinition {
    pub(crate) key: &'static str,
    pub(crate) category: FieldCategory,
    pub(crate) has_consumer: bool,
}

/// §3.6 的稳定词表。aliases 保持原样登记，不做全局 schema 强校验。
pub(crate) const FIELD_REGISTRY: &[FieldDefinition] = &[
    FieldDefinition {
        key: "observed_head",
        category: FieldCategory::Engine,
        has_consumer: true,
    },
    FieldDefinition {
        key: "observed_worktree_hash",
        category: FieldCategory::Engine,
        has_consumer: true,
    },
    FieldDefinition {
        key: "recorded_at",
        category: FieldCategory::Engine,
        has_consumer: true,
    },
    FieldDefinition {
        key: "取活依据",
        category: FieldCategory::Engine,
        has_consumer: false,
    },
    FieldDefinition {
        key: "优先级",
        category: FieldCategory::Scheduling,
        has_consumer: true,
    },
    FieldDefinition {
        key: "依赖",
        category: FieldCategory::Scheduling,
        has_consumer: true,
    },
    FieldDefinition {
        key: "阻塞",
        category: FieldCategory::Scheduling,
        has_consumer: true,
    },
    FieldDefinition {
        key: "停车",
        category: FieldCategory::Scheduling,
        has_consumer: true,
    },
    FieldDefinition {
        key: "阶段",
        category: FieldCategory::Scheduling,
        has_consumer: true,
    },
    FieldDefinition {
        key: "取得线",
        category: FieldCategory::Scheduling,
        has_consumer: true,
    },
    FieldDefinition {
        key: "refs",
        category: FieldCategory::Scheduling,
        has_consumer: true,
    },
    FieldDefinition {
        key: "内容",
        category: FieldCategory::Narrative,
        has_consumer: true,
    },
    FieldDefinition {
        key: "验收",
        category: FieldCategory::Narrative,
        has_consumer: true,
    },
    FieldDefinition {
        key: "进展",
        category: FieldCategory::Narrative,
        has_consumer: true,
    },
    FieldDefinition {
        key: "来源",
        category: FieldCategory::Narrative,
        has_consumer: true,
    },
    FieldDefinition {
        key: "发现记录",
        category: FieldCategory::Narrative,
        has_consumer: true,
    },
    FieldDefinition {
        key: "边界",
        category: FieldCategory::Narrative,
        has_consumer: true,
    },
    FieldDefinition {
        key: "对账",
        category: FieldCategory::Narrative,
        has_consumer: false,
    },
    FieldDefinition {
        key: "批次表",
        category: FieldCategory::Narrative,
        has_consumer: false,
    },
    FieldDefinition {
        key: "背景",
        category: FieldCategory::Narrative,
        has_consumer: false,
    },
    FieldDefinition {
        key: "根因",
        category: FieldCategory::Narrative,
        has_consumer: false,
    },
    FieldDefinition {
        key: "执行者",
        category: FieldCategory::Narrative,
        has_consumer: false,
    },
    FieldDefinition {
        key: "归属",
        category: FieldCategory::Narrative,
        has_consumer: false,
    },
    FieldDefinition {
        key: "原始描述",
        category: FieldCategory::Narrative,
        has_consumer: false,
    },
    FieldDefinition {
        key: "不变量",
        category: FieldCategory::Narrative,
        has_consumer: false,
    },
    FieldDefinition {
        key: "不变式",
        category: FieldCategory::Narrative,
        has_consumer: false,
    },
];

pub(crate) fn registry_json() -> serde_json::Value {
    serde_json::json!(FIELD_REGISTRY
        .iter()
        .map(|definition| serde_json::json!({
            "key": definition.key,
            "category": definition.category.as_str(),
            "has_consumer": definition.has_consumer,
        }))
        .collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docstore::Entry;

    #[test]
    fn field_registry_lists_categories_and_consumers() {
        assert_eq!(FIELD_REGISTRY.len(), 26);
        let engine = definition("observed_head").expect("引擎字段应登记");
        assert_eq!(engine.category, FieldCategory::Engine);
        assert!(engine.has_consumer);
        let zero = definition("不变量").expect("零消费者字段应登记");
        assert!(!zero.has_consumer);
    }

    #[test]
    fn structured_entry_marks_unknown_fields_gray_and_counts_them() {
        let entry = Entry {
            id: "R-356".into(),
            title: "test".into(),
            status: "todo".into(),
            severity: None,
            fields: vec![
                ("内容".into(), "known".into()),
                ("历史自定义".into(), "free".into()),
            ],
        };
        let value = super::super::scheduling::structured_entry(&entry, &[], false);
        assert_eq!(value["unknown_field_count"], 1);
        assert_eq!(value["fields"][0]["category"], "narrative");
        assert_eq!(value["fields"][1]["known"], false);
        assert_eq!(value["fields"][1]["presentation"], "gray");
        assert!(value["field_registry"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| { item["key"] == "observed_head" && item["category"] == "engine" }));
    }
}

pub(crate) fn definition(key: &str) -> Option<&'static FieldDefinition> {
    FIELD_REGISTRY
        .iter()
        .find(|definition| definition.key == key || definition.key.eq_ignore_ascii_case(key))
}

pub(crate) fn metadata(key: &str) -> serde_json::Value {
    match definition(key) {
        Some(definition) => serde_json::json!({
            "category": definition.category.as_str(),
            "has_consumer": definition.has_consumer,
            "known": true,
        }),
        None => serde_json::json!({
            "category": "unknown",
            "has_consumer": false,
            "known": false,
            "presentation": "gray",
        }),
    }
}
