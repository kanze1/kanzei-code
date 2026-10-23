//! Read-only run metrics projection and IPC endpoints.
use crate::normalized_project_root;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[tauri::command]
pub(crate) async fn run_metrics(
    project_dir: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || run_metrics_rounds(&project_dir, limit))
        .await
        .map_err(|error| format!("运行画像查询任务失败: {error}"))?
}

pub(super) fn run_metrics_rounds(
    project_dir: &str,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    // 与会话落库同一个身份根:projectDir 写法不同(斜杠、`\\?\` 前缀)也得落在同一组 session_id 上。
    let root = normalized_project_root(Path::new(project_dir));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    let base_session_id = kanzei_core::project_session_id(&root);
    let limit = limit.unwrap_or(20).clamp(1, 200);
    let rows = store
        .recent_project_episodes(&base_session_id, limit)
        .map_err(|e| e.to_string())?;
    let parse = |text: &str| {
        serde_json::from_str::<serde_json::Value>(text).unwrap_or(serde_json::json!({}))
    };
    let rounds: Vec<serde_json::Value> = rows
        .into_iter()
        .map(
            |(
                session_id,
                at,
                prompt,
                outcome,
                steps,
                input,
                output,
                tools,
                context,
                metrics,
                duration_ms,
            )| {
                json!({
                    "sessionId": session_id,
                    "at": at,
                    "prompt": prompt,
                    "outcome": outcome,
                    "steps": steps,
                    "inputTokens": input,
                    "outputTokens": output,
                    "tools": parse(&tools),
                    "context": parse(&context),
                    "metrics": parse(&metrics),
                    "measured": metrics.trim() != "{}" && !metrics.trim().is_empty(),
                    // Historical rows predate duration observation and contain zero.
                    "durationMs": (duration_ms > 0).then_some(duration_ms),
                })
            },
        )
        .collect();
    Ok(json!({ "rounds": rounds }))
}

/// R-338 B3:读取可重建的 task 运行画像；前端只消费此 projection，不自行分组。
#[tauri::command]
pub(crate) async fn run_metrics_by_task(project_dir: String) -> Result<serde_json::Value, String> {
    // 历史审计和 SQLite 打开均可能阻塞；整个查询留在阻塞线程池，释放窗口事件循环。
    tauri::async_runtime::spawn_blocking(move || {
        let root = PathBuf::from(&project_dir);
        let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
            .map_err(|error| error.to_string())?;
        let projection = store.task_metrics().map_err(|error| error.to_string())?;
        serde_json::to_value(projection).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("任务画像查询任务失败: {error}"))?
}

/// R-240:从 prompt_head 提取需求 ID(`R-123` / `D-321`),取第一个命中。
/// 自举/取活轮的 prompt 以条目标题开头(R-xxx …),用户轮通常无——据此归类。
pub(crate) fn extract_ticket_id(prompt_head: &str) -> Option<String> {
    let bytes = prompt_head.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        // 前一个字符是字母/数字/下划线说明 `R-`/`D-` 只是单词的一截(ERROR-5、PR-12),不是条目号。
        let at_boundary = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if at_boundary && (bytes[i] == b'R' || bytes[i] == b'D') && bytes[i + 1] == b'-' {
            let mut j = i + 2;
            let mut num = String::new();
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                num.push(bytes[j] as char);
                j += 1;
            }
            if !num.is_empty() {
                return Some(format!("{}-{num}", bytes[i] as char));
            }
        }
        i += 1;
    }
    None
}

/// R-240:从需求/缺陷文档解析 `<id>` 的复杂度字段(小/中/大)。
/// 读 `.kanzei/project/requirements.md` 与 `defects.md`,`## {id} ` 段落内扫
/// `- 复杂度: X` 行。找不到或字段缺失返回 None(归类为「未知」)。
pub(crate) fn ticket_complexity(project_root: &Path, id: &str) -> Option<String> {
    for name in ["requirements.md", "defects.md"] {
        let text = std::fs::read_to_string(project_root.join(".kanzei/project").join(name)).ok()?;
        let marker = format!("## {id} ");
        let Some(pos) = text.find(&marker) else {
            continue;
        };
        let rest = &text[pos..];
        let section_end = rest.find("\n## ").unwrap_or(rest.len());
        for line in rest[..section_end].lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("- 复杂度:") {
                let value = value.trim();
                if value == "小" || value == "中" || value == "大" {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// R-240:按 (类型, 复杂度) 聚合运行指标。纯函数,可单测。
/// rows 取 (prompt_head, outcome, steps, input_tokens, output_tokens)。
/// 返回:groups 数组(每项 count/sumInput/sumOutput/sumSteps/avgSteps + 分类键)
/// + uncategorized(未提取到需求 ID 的轮次合计)。
pub(crate) fn aggregate_run_metrics(
    rows: &[(String, String, u32, u64, u64)],
    metas: &HashMap<String, String>,
) -> serde_json::Value {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<(String, String), (u32, u64, u64, u64)> = BTreeMap::new();
    let mut other: (u32, u64, u64, u64) = (0, 0, 0, 0);
    for (prompt, _, steps, input, output) in rows {
        let target = match extract_ticket_id(prompt) {
            Some(id) => {
                let kind = if id.starts_with('D') { "D" } else { "R" };
                let complexity = metas
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| "未知".to_string());
                groups
                    .entry((kind.to_string(), complexity))
                    .or_insert((0, 0, 0, 0))
            }
            None => &mut other,
        };
        target.0 += 1;
        target.1 += input;
        target.2 += output;
        target.3 += *steps as u64;
    }
    let group_values: Vec<serde_json::Value> = groups
        .into_iter()
        .map(
            |((kind, complexity), (count, sum_input, sum_output, sum_steps))| {
                serde_json::json!({
                    "kind": kind,
                    "complexity": complexity,
                    "count": count,
                    "sumInput": sum_input,
                    "sumOutput": sum_output,
                    "sumSteps": sum_steps,
                    "avgInput": if count > 0 { sum_input as f64 / count as f64 } else { 0.0 },
                    "avgOutput": if count > 0 { sum_output as f64 / count as f64 } else { 0.0 },
                    "avgSteps": if count > 0 { sum_steps as f64 / count as f64 } else { 0.0 },
                })
            },
        )
        .collect();
    serde_json::json!({
        "groups": group_values,
        "uncategorized": {
            "count": other.0,
            "sumInput": other.1,
            "sumOutput": other.2,
            "sumSteps": other.3,
        }
    })
}

/// R-240:按需求类型(R-/D-)与复杂度(小/中/大)聚合的运行时指标。
/// 数据源 = episodes 表(prompt_head 提取需求 ID → requirements/defects 文档取复杂度)。
/// 返回 groups(按分类的 count/token 合计与均值)+ uncategorized(无 ID 轮)。
#[tauri::command]
pub(crate) fn run_metrics_by_category(
    project_dir: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let root = PathBuf::from(&project_dir);
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    let session_id = kanzei_core::project_session_id(&root);
    let limit = limit.unwrap_or(200).clamp(1, 1000);
    let rows = store
        .recent_episodes(&session_id, limit)
        .map_err(|e| e.to_string())?;
    let mut metas: HashMap<String, String> = HashMap::new();
    for (_, prompt, _, _, _, _, _, _, _) in &rows {
        if let Some(id) = extract_ticket_id(prompt) {
            metas.entry(id.clone()).or_insert_with(|| {
                ticket_complexity(&root, &id).unwrap_or_else(|| "未知".to_string())
            });
        }
    }
    let mapped: Vec<(String, String, u32, u64, u64)> = rows
        .iter()
        .map(|(_, prompt, outcome, steps, input, output, _, _, _)| {
            (prompt.clone(), outcome.clone(), *steps, *input, *output)
        })
        .collect();
    Ok(aggregate_run_metrics(&mapped, &metas))
}
