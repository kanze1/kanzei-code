//! 研究页快照:topic 目录枚举、来源/发现条目 IPC 形状与运行记录(由 docs.rs 拆出)。

use std::path::Path;

use kanzei_tools::docstore::{DocStore, FINDINGS, SOURCES};
use serde_json::json;

/// 研究来源/发现条目的 IPC 形状。`nextStatuses` 与 docs_snapshot 里需求/缺陷条目同口径(UX-106):
/// 只列引擎真会放行的目标状态,界面据此画「归档/确认/放弃」按钮,不再自己抄一份状态机。
fn research_entry_json(
    entry: &kanzei_tools::docstore::Entry,
    topic: &str,
    store: &DocStore,
) -> serde_json::Value {
    json!({
        "id": entry.id,
        "title": entry.title,
        "status": entry.status,
        "severity": entry.severity,
        "fields": entry.fields,
        "topic": topic,
        "nextStatuses": next_statuses(store, &entry.status),
    })
}

/// 终态不再列出任何去向(D-241:终态不可重开;引擎只放行终态间纠错,那不该是界面上的一颗常驻按钮)。
fn next_statuses(store: &DocStore, status: &str) -> Vec<&'static str> {
    if store.kind.terminal.contains(&status) {
        return Vec::new();
    }
    store
        .kind
        .statuses
        .iter()
        .copied()
        .filter(|next| *next != status && store.transition_allowed(status, next).is_ok())
        .collect()
}

/// 每个运行随快照带回的事件条数上限(取尾部)。研究页 4 秒一轮地读快照,长跑实验的事件可达数万条,
/// 全量带回只会拖慢轮询;界面的指标曲线只画每个指标最近 120 点,终端区本就以日志尾部为主(UX-105)。
const RESEARCH_RUN_EVENT_TAIL: usize = 1200;
/// 终端日志预览只读文件末尾这么多字节。
const RESEARCH_TERMINAL_TAIL_BYTES: u64 = 64 * 1024;

/// 读文件末尾至多 `max` 字节(宽松 UTF-8 解码)。只 seek 到尾部再读,不把整份日志读进内存:
/// 长跑实验的终端日志可达数百 MB,而这里每个轮询周期、每个运行都要调一次。
fn read_tail_lossy(path: &Path, max: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(max))).ok()?;
    let mut bytes = Vec::with_capacity(len.min(max) as usize);
    file.take(max).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// R-221 B2:枚举 topic 目录，来源/发现/报告以同一 topic 作为隔离边界。
fn research_runs(root: &Path, topic: &str) -> Result<Vec<serde_json::Value>, String> {
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))
        .map_err(|error| format!("读取 topic `{topic}` research runs 失败: {error}"))?;
    store
        .list_research_runs(topic)
        .map_err(|error| format!("读取 topic `{topic}` research runs 失败: {error}"))
        .map(|runs| {
            runs.into_iter()
                .map(|run| {
                    let (events, events_total) = store
                        .list_research_run_events_tail(&run.result_id, RESEARCH_RUN_EVENT_TAIL)
                        .unwrap_or_default();
                    let snapshot_path = root.join(&run.environment_snapshot_ref);
                    let environment = std::fs::read_to_string(snapshot_path)
                        .ok()
                        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                        .unwrap_or_else(|| json!({}));
                    let terminal_path = root.join(&run.terminal_log_path);
                    let terminal_preview =
                        read_tail_lossy(&terminal_path, RESEARCH_TERMINAL_TAIL_BYTES)
                            .unwrap_or_default();
                    json!({
                        "run": run,
                        "events": events,
                        // 事件总数与是否被截断:界面据此写明「只显示最近 N 条」,不让截断变成静默丢数据。
                        "events_total": events_total,
                        "events_truncated": events_total > events.len() as i64,
                        "terminal_preview": terminal_preview,
                        "environment": environment,
                        "drift": environment.get("drift").cloned().unwrap_or_else(|| json!({})),
                    })
                })
                .collect()
        })
}

pub(super) fn research_topics(root: &Path) -> Result<Vec<serde_json::Value>, String> {
    let research_root = root.join(".kanzei/research");
    let mut names = std::fs::read_dir(&research_root)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|topic| kanzei_tools::docstore::DocStore::validate_topic(topic).is_ok())
        .collect::<Vec<_>>();
    names.sort();
    let mut topics = names
        .into_iter()
        .map(|topic| {
            let source_store =
                kanzei_tools::docstore::DocStore::open_topic(root, &SOURCES, &topic)
                    .map_err(|error| format!("读取 topic `{topic}` 来源失败: {error}"))?;
            let sources = source_store
                .load()
                .map_err(|error| format!("读取 topic `{topic}` 来源失败: {error}"))?;
            let finding_store =
                kanzei_tools::docstore::DocStore::open_topic(root, &FINDINGS, &topic)
                    .map_err(|error| format!("读取 topic `{topic}` 发现失败: {error}"))?;
            let findings = finding_store
                .load()
                .map_err(|error| format!("读取 topic `{topic}` 发现失败: {error}"))?;
            let experiment_model = kanzei_core::load_research_topic(root, &topic)
                .map_err(|error| format!("读取 topic `{topic}` 探索事实失败: {error}"))?;
            let topic_path = crate::research_topics::topic_path(root, &topic)?;
            let metadata = crate::research_topics::describe_topic(&topic_path, !sources.is_empty() || !findings.is_empty())?;
            let report = topic_path.join("report.md");
            Ok(json!({
                "topic": topic,
                "legacy": false,
                "kind": metadata.kind,
                "label": metadata.title,
                "sources": sources.iter().map(|entry| research_entry_json(entry, &topic, &source_store)).collect::<Vec<_>>(),
                "findings": findings.iter().map(|entry| research_entry_json(entry, &topic, &finding_store)).collect::<Vec<_>>(),
                "report": report.is_file(),
                "explorations": experiment_model.explorations,
                "exploration_diagnostics": experiment_model.diagnostics,
                "runs": research_runs(root, &topic)?,
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;

    // 旧版平铺文件只保留兼容读取，不再作为新写入落点。用 nullable topic 明确
    // 它没有可传给 open_topic 的目录，前端仍能选择并查看历史研究成果。
    let legacy_source_store = DocStore::open(root, &SOURCES);
    let legacy_sources = legacy_source_store
        .load()
        .map_err(|error| format!("读取旧版平铺来源失败: {error}"))?;
    let legacy_finding_store = DocStore::open(root, &FINDINGS);
    let legacy_findings = legacy_finding_store
        .load()
        .map_err(|error| format!("读取旧版平铺发现失败: {error}"))?;
    let legacy_report = research_root.join("report.md");
    if !legacy_sources.is_empty() || !legacy_findings.is_empty() || legacy_report.is_file() {
        topics.push(json!({
            "topic": null,
            "legacy": true,
            "kind": "legacy",
            "label": "旧版平铺",
            "sources": legacy_sources.iter().map(|entry| research_entry_json(entry, "", &legacy_source_store)).collect::<Vec<_>>(),
            "findings": legacy_findings.iter().map(|entry| research_entry_json(entry, "", &legacy_finding_store)).collect::<Vec<_>>(),
            "report": legacy_report.is_file(),
        }));
    }
    Ok(topics)
}

#[cfg(test)]
mod research_snapshot_tail_tests {
    use super::{next_statuses, read_tail_lossy};
    use kanzei_tools::docstore::{DocStore, FINDINGS, SOURCES};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("kz-docs-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// UX-105:终端日志预览只读文件末尾,小文件读全部,缺文件不报错。
    #[test]
    fn 终端日志预览只读尾部() {
        let dir = temp_dir("tail");
        let big = dir.join("big.log");
        std::fs::write(
            &big,
            format!("{}{}", "a".repeat(60_000), "b".repeat(40_000)),
        )
        .unwrap();
        let tail = read_tail_lossy(&big, 64 * 1024).unwrap();
        assert_eq!(tail.len(), 64 * 1024);
        assert!(tail.starts_with('a') && tail.ends_with('b'));
        assert_eq!(tail.matches('b').count(), 40_000);
        let small = dir.join("small.log");
        std::fs::write(&small, "hello\nworld").unwrap();
        assert_eq!(
            read_tail_lossy(&small, 64 * 1024).as_deref(),
            Some("hello\nworld")
        );
        assert_eq!(read_tail_lossy(&dir.join("missing.log"), 64), None);
        std::fs::remove_dir_all(dir).ok();
    }

    /// UX-106:来源/发现条目带 nextStatuses,与引擎状态机同口径;终态没有去向。
    #[test]
    fn 研究条目带下一状态且终态为空() {
        let dir = temp_dir("next");
        let sources = DocStore::open(&dir, &SOURCES);
        assert_eq!(next_statuses(&sources, "active"), vec!["archived"]);
        assert!(next_statuses(&sources, "archived").is_empty());
        let findings = DocStore::open(&dir, &FINDINGS);
        assert_eq!(
            next_statuses(&findings, "draft"),
            vec!["confirmed", "dropped"]
        );
        assert!(next_statuses(&findings, "confirmed").is_empty());
        assert!(next_statuses(&findings, "dropped").is_empty());
        std::fs::remove_dir_all(dir).ok();
    }
}
