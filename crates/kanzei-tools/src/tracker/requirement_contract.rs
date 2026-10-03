//! Typed requirement writes and versioned acceptance evidence.
use super::TrackerInput;
use crate::docstore::Entry;
use kanzei_memory::docstore::requirement::{acceptance_evidence, RequirementSpec, SPEC_FIELDS};

pub(super) fn prepare(input: &mut TrackerInput, existing: Option<&Entry>) -> Result<(), String> {
    if input.action == "add"
        && input.requirement.is_none()
        && !input.fields.contains_key("内容")
        && !input.fields.contains_key("验收")
    {
        input.requirement = Some(RequirementSpec::default());
        // Keep any old source/discovery text as supplementary provenance.
        if let Some(source) = input.fields.remove("来源") {
            input.requirement.as_mut().unwrap().source.quote = source;
        }
    }
    let old = existing
        .map(RequirementSpec::from_entry)
        .transpose()?
        .flatten();
    if let Some(spec) = &mut input.requirement {
        if input
            .fields
            .keys()
            .any(|key| SPEC_FIELDS.contains(&key.as_str()))
        {
            return Err("需求正文通过 requirement 对象提交，不要在 fields 重复填写".into());
        }
        spec.normalize(old.as_ref())?;
        if let Some(previous) = existing.filter(|_| old.is_none()) {
            // A deliberate migration must keep every legacy field, including normative
            // boundaries and evidence; never discard them just because a spec is shorter.
            let preserved: Vec<_> = previous
                .fields
                .iter()
                .filter(|(key, _)| SPEC_FIELDS.contains(&key.as_str()))
                .cloned()
                .collect();
            if !preserved.is_empty() {
                input.fields.insert(
                    "迁移原文".into(),
                    serde_json::to_string(&preserved).unwrap(),
                );
            }
        }
        input.fields.extend(spec.fields());
        if input.action == "add" {
            input.status = Some(
                if spec.gaps().is_empty() {
                    "todo"
                } else {
                    "draft"
                }
                .into(),
            );
        }
    } else if old.is_some()
        && input
            .fields
            .keys()
            .any(|key| SPEC_FIELDS.contains(&key.as_str()))
    {
        return Err(
            "此需求使用结构化格式；先 req get，再通过 requirement 整体更新正文并保留验收项 ID"
                .into(),
        );
    }
    if let Some(evidence) = &input.evidence {
        let spec = input
            .requirement
            .as_ref()
            .or(old.as_ref())
            .ok_or("结构化验收证据需要 requirement 契约")?;
        let revision = spec.revision();
        let mut keys = std::collections::BTreeSet::new();
        for item in evidence {
            if !spec
                .acceptance
                .iter()
                .any(|criterion| criterion.id == item.criterion_id)
            {
                return Err(format!("验收项 {} 不存在", item.criterion_id));
            }
            if item.revision != revision {
                return Err("验收证据对应旧需求版本；请核对当前要求并重新验证".into());
            }
            if item.reference.trim().is_empty() {
                return Err("验收证据需要真实引用，不能只声明通过".into());
            }
            if !keys.insert(&item.criterion_id) {
                return Err("验收证据的 criterion_id 重复".into());
            }
        }
        input
            .fields
            .insert("验收证据".into(), serde_json::to_string(evidence).unwrap());
    } else if input.fields.contains_key("验收证据") {
        return Err("验收证据通过顶层 evidence 提交，不能通过自由字段写入".into());
    }
    if let Some(previous) = existing {
        let mut merged = previous.clone();
        for (key, value) in &input.fields {
            merged.fields.retain(|(k, _)| k != key);
            merged.fields.push((key.clone(), value.clone()));
        }
        if let Some(spec) = RequirementSpec::from_entry(&merged)? {
            let target = input.status.as_deref().unwrap_or(&previous.status);
            if matches!(target, "todo" | "doing" | "awaiting_external" | "done")
                && !spec.gaps().is_empty()
            {
                return Err(format!(
                    "需求尚有未决内容: {}。保存为 draft 后补齐，再转 todo",
                    spec.gaps().join("；")
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn check_close(entry: &Entry, root: &std::path::Path) -> Result<(), String> {
    let Some(spec) = RequirementSpec::from_entry(entry)? else {
        return Ok(());
    };
    if !spec.gaps().is_empty() {
        return Err(format!("需求尚未补齐: {}", spec.gaps().join("；")));
    }
    let evidence = acceptance_evidence(entry)?;
    for item in &evidence {
        if item.reference.trim().starts_with("T-") {
            let records = crate::test_record::records_for_entry(root, &entry.id);
            if !records
                .iter()
                .any(|record| record["id"] == item.reference && record["status"] == "passed")
            {
                return Err(format!(
                    "{} 的测试证据 {} 不存在或未通过",
                    item.criterion_id, item.reference
                ));
            }
        } else {
            let path = item
                .reference
                .rsplit_once(':')
                .filter(|(_, line)| line.parse::<usize>().is_ok())
                .map(|(path, _)| path)
                .unwrap_or(&item.reference);
            if !root.join(path).is_file() {
                return Err(format!(
                    "{} 的证据文件不存在: {}",
                    item.criterion_id, item.reference
                ));
            }
        }
    }
    let revision = spec.revision();
    let missing: Vec<_> = spec
        .acceptance
        .iter()
        .filter(|criterion| {
            !evidence.iter().any(|e| {
                e.criterion_id == criterion.id
                    && e.revision == revision
                    && !e.reference.trim().is_empty()
            })
        })
        .map(|c| c.id.as_str())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("缺少当前需求版本的验收证据: {}；用 req get 获取 revision，再通过 evidence 逐项关联实际测试或设计证据", missing.join("、")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docstore::{DocStore, REQUIREMENTS};
    use kanzei_harness::{Tool, ToolCtx};
    use serde_json::json;

    fn temp_root() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-requirement-contract-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn spec() -> RequirementSpec {
        serde_json::from_value(json!({
            "statement":"收到导出请求时，系统应提供包含所选评分的 CSV 文件。",
            "acceptance":[{"text":"选择两条评分并导出，文件包含所选记录及评分。"}],
            "source":{"quote":"我要导出选中的评分"}
        }))
        .unwrap()
    }

    fn entry(spec: &RequirementSpec) -> Entry {
        Entry {
            id: "R-001".into(),
            title: "导出所选评分".into(),
            status: "todo".into(),
            severity: None,
            fields: spec.fields().into_iter().collect(),
        }
    }

    #[test]
    fn spec_roundtrip_preserves_ids_and_invalidates_evidence_only_on_meaning_change() {
        let mut before = spec();
        before.normalize(None).unwrap();
        let e = entry(&before);
        assert_eq!(RequirementSpec::from_entry(&e).unwrap().unwrap(), before);
        let mut after = before.clone();
        after.source.quote = "另一来源".into();
        assert_eq!(after.revision(), before.revision());
        after.acceptance[0].text = "导出后重新导入，所选评分逐项一致。".into();
        after.normalize(Some(&before)).unwrap();
        assert_eq!(after.acceptance[0].id, before.acceptance[0].id);
        assert_ne!(after.revision(), before.revision());
        after.acceptance[0].id.clear();
        let same = after.clone();
        assert!(after.normalize(Some(&same)).is_err());
    }

    #[test]
    fn close_requires_existing_evidence_for_each_current_criterion() {
        let root = temp_root();
        let mut spec = spec();
        spec.normalize(None).unwrap();
        let mut e = entry(&spec);
        assert!(check_close(&e, &root).unwrap_err().contains("AC-1"));
        let evidence = json!([{"criterion_id":"AC-1","revision":spec.revision(),"reference":"verification.txt"}]);
        e.fields.push(("验收证据".into(), evidence.to_string()));
        assert!(check_close(&e, &root).unwrap_err().contains("不存在"));
        std::fs::write(root.join("verification.txt"), "实际测试步骤和结果").unwrap();
        check_close(&e, &root).unwrap();
        spec.statement.push_str("并显示下载完成状态。");
        for (k, v) in spec.fields() {
            e.fields.retain(|(key, _)| *key != k);
            e.fields.push((k, v));
        }
        assert!(check_close(&e, &root).unwrap_err().contains("当前需求版本"));
    }

    #[tokio::test]
    async fn draft_excluded_from_work_until_explicit_ready_transition_and_list_is_summary() {
        let root = temp_root();
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let tool = super::super::TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let out = tool.execute(json!({"action":"add","title":"待定义导出","requirement":{"questions":["导出哪些字段？"]}}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert_eq!(
            DocStore::open(&root, &REQUIREMENTS).load().unwrap()[0].status,
            "draft"
        );
        let decision = crate::work::resolve_work_decision(
            &root,
            &root,
            kanzei_harness::auto_run::WorkPriority::RequirementFirst,
        )
        .unwrap();
        assert_eq!(decision.decision, crate::work::WorkDecision::Empty);
        assert!(decision.selected.is_none());
        assert!(decision.blocked_items.is_empty());
        let early = tool
            .execute(
                json!({"action":"update","id":"R-001","status":"todo"}),
                &ctx,
            )
            .await;
        assert!(early.is_error);
        let ready = tool
            .execute(
                json!({"action":"update","id":"R-001","status":"todo","requirement":spec()}),
                &ctx,
            )
            .await;
        assert!(!ready.is_error, "{}", ready.content);
        let listed = tool
            .execute(
                json!({"action":"list","reason":"deduplicate_registration"}),
                &ctx,
            )
            .await;
        let view: serde_json::Value = serde_json::from_str(&listed.content).unwrap();
        assert!(view["entries"][0].get("fields").is_none());
        let fetched = tool
            .execute(json!({"action":"get","id":"R-001"}), &ctx)
            .await;
        assert!(fetched.content.contains("AC-1") && fetched.content.contains("revision"));
    }

    #[test]
    fn explicit_legacy_migration_preserves_overwritten_text_and_unknown_boundaries() {
        let mut previous = entry(&spec());
        previous.fields = vec![
            ("内容".into(), "旧正文包含额外限制".into()),
            ("验收".into(), "旧验收全文".into()),
            ("边界".into(), "仍被引用的文件不得删除".into()),
            ("自定义限制".into(), "保留原文".into()),
        ];
        let mut input: TrackerInput =
            serde_json::from_value(json!({"action":"update","id":"R-001","requirement":spec()}))
                .unwrap();
        prepare(&mut input, Some(&previous)).unwrap();
        assert!(input.fields["迁移原文"].contains("旧验收全文"));
        assert!(!input.fields.contains_key("边界"));
        assert!(!input.fields.contains_key("自定义限制"));
    }
}
