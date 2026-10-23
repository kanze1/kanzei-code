use super::*;
use kanzei_harness::{Tool, ToolCtx};

pub async fn writeback(
    root: &Path,
    def: &ScheduleDef,
    outcome: &Outcome,
    remote: bool,
) -> Vec<Value> {
    let mut results = vec![];
    for channel in &def.writeback {
        if remote && !channel.starts_with("file:") {
            continue;
        }
        if channel != "notify" && (!outcome.ok || outcome.text.trim().is_empty()) {
            results.push(
                json!({"channel":channel,"ok":false,"detail":"运行未成功，保留已有文件和记录"}),
            );
            continue;
        }
        let result: Result<String, String> = match channel.as_str() {
            "notify" => SessionStore::open(&kanzei_core::project_state_path(root))
                .map_err(|e| e.to_string())
                .and_then(|store| {
                    store
                        .append_notification_atomic(
                            "kz-schedules",
                            if outcome.ok { "succeeded" } else { "failed" },
                            &format!("{}: {}", def.name, outcome.summary),
                            false,
                        )
                        .map(|_| "已保存通知".into())
                        .map_err(|e| e.to_string())
                }),
            "memory_inbox" => kanzei_memory::memory::MemoryStore::project(root)
                .append_note(
                    &format!("定时任务 {}: {}", def.name, outcome.summary),
                    &outcome.text.chars().take(8000).collect::<String>(),
                    "fact",
                    &[],
                )
                .map(|path| path.display().to_string())
                .map_err(|e| e.to_string()),
            "idea" => {
                let tool = crate::tracker::TrackerTool {
                    tool_name: "idea",
                    noun: "idea",
                    kind: &kanzei_memory::docstore::IDEAS,
                    requires_refs: None,
                };
                let ctx = ToolCtx::new(root.into(), root.into());
                let output=kanzei_harness::managed_fence::tool_scope(&ctx.project_root,"idea",tool.execute(json!({"action":"add","title":format!("定时任务 {}: {}",def.name,outcome.summary.chars().take(40).collect::<String>()),"fields":{"原始描述":outcome.text,"来源":format!("schedule:{} run {}",def.name,outcome.run_id)}}),&ctx)).await;
                if output.is_error {
                    Err(output.content)
                } else {
                    Ok(output.content)
                }
            }
            value => {
                safe_output_path(root, value.strip_prefix("file:").unwrap_or("")).and_then(|path| {
                    kanzei_base::atomic_file::write_atomic(&path, &outcome.text)
                        .map(|_| path.display().to_string())
                        .map_err(|e| e.to_string())
                })
            }
        };
        results.push(match result {
            Ok(detail) => json!({"channel":channel,"ok":true,"detail":detail}),
            Err(detail) => json!({"channel":channel,"ok":false,"detail":detail}),
        });
    }
    results
}
fn safe_output_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        || path.starts_with(".kanzei")
    {
        return Err("回写文件必须是项目内普通相对路径，不能指向 .kanzei 托管区".into());
    }
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let target = root.join(path);
    let parent = target.parent().ok_or("回写路径缺少父目录")?;
    for ancestor in parent.ancestors().take_while(|ancestor| *ancestor != root) {
        if ancestor.exists()
            && !std::fs::canonicalize(ancestor)
                .map_err(|e| e.to_string())?
                .starts_with(&root)
        {
            return Err("回写路径通过链接越出项目".into());
        }
    }
    if std::fs::symlink_metadata(&target).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err("回写目标为符号链接".into());
    }
    Ok(target)
}
