//! 研究课题身份与会话上下文。复用 topic 目录和 processes 持久化，不迁移历史工件。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TopicMetadata {
    pub(crate) title: String,
    pub(crate) kind: String,
}

pub(crate) fn topic_path(root: &Path, topic: &str) -> Result<PathBuf, String> {
    kanzei_tools::docstore::DocStore::validate_topic(topic).map_err(|e| e.to_string())?;
    let research_root = root.join(".kanzei/research");
    let path = research_root.join(topic);
    let canonical_root = research_root.canonicalize().map_err(|e| e.to_string())?;
    let canonical_path = path
        .canonicalize()
        .map_err(|_| format!("研究课题不存在: {topic}"))?;
    if !canonical_path.is_dir() || !canonical_path.starts_with(&canonical_root) {
        return Err("研究课题路径越出研究目录".into());
    }
    Ok(path)
}

fn header_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    lines
        .take_while(|line| line.trim() != "---")
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim() == key).then(|| value.trim().trim_matches(['"', '\'']))
        })
}

pub(crate) fn describe_topic(path: &Path, has_sources: bool) -> Result<TopicMetadata, String> {
    let metadata_path = path.join("topic.json");
    if metadata_path.is_file() {
        let metadata: TopicMetadata = serde_json::from_str(
            &std::fs::read_to_string(&metadata_path).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("课题元数据读取失败 {}: {e}", metadata_path.display()))?;
        if !["research", "dev_recon", "unclassified"].contains(&metadata.kind.as_str()) {
            return Err(format!("未知课题类别: {}", metadata.kind));
        }
        return Ok(metadata);
    }
    let report = std::fs::read_to_string(path.join("report.md")).unwrap_or_default();
    let kind = match header_field(&report, "kind") {
        Some("dev_recon" | "prior_art" | "prior-art") => "dev_recon",
        Some("research") => "research",
        _ if has_sources
            || path.join("plan.json").is_file()
            || path.join("paper.tex").is_file() =>
        {
            "research"
        }
        _ if path.join("prior-art.md").is_file() => "dev_recon",
        _ => "unclassified",
    };
    Ok(TopicMetadata {
        title: header_field(&report, "title")
            .unwrap_or_else(|| path.file_name().and_then(|v| v.to_str()).unwrap_or(""))
            .to_string(),
        kind: kind.into(),
    })
}

#[tauri::command]
pub(crate) fn research_topic_create(
    project_dir: String,
    topic: String,
    title: String,
) -> Result<serde_json::Value, String> {
    create_topic(Path::new(&project_dir), &topic, &title)
}

fn create_topic(root: &Path, topic: &str, title: &str) -> Result<serde_json::Value, String> {
    kanzei_tools::docstore::DocStore::validate_topic(topic).map_err(|e| e.to_string())?;
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err("课题名称应为 1 到 200 个字符".into());
    }
    let root = crate::normalized_project_root(root);
    if !root.is_dir() {
        return Err("项目目录不存在".into());
    }
    let research_root = root.join(".kanzei/research");
    std::fs::create_dir_all(&research_root).map_err(|e| e.to_string())?;
    let path = research_root.join(topic);
    std::fs::create_dir(&path)
        .map_err(|e| format!("无法创建课题 {topic}，请确认标识未被使用: {e}"))?;
    let metadata = TopicMetadata {
        title: title.into(),
        kind: "research".into(),
    };
    let bytes = serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?;
    std::fs::write(path.join("topic.json"), bytes)
        .map_err(|e| format!("课题目录已创建但元数据保存失败 {}: {e}", path.display()))?;
    Ok(serde_json::json!({ "topic": topic, "label": title, "kind": "research" }))
}

pub(crate) fn validate_run_topic(
    root: &Path,
    profile: Option<&str>,
    bound: Option<&str>,
    requested: Option<&str>,
) -> Result<Option<String>, String> {
    if requested.is_some() && requested != bound {
        return Err("当前会话与所选研究课题不一致，请切换到该课题的会话".into());
    }
    if let Some(topic) = bound {
        if profile != Some("research") {
            return Err("已绑定课题的研究会话不能切换为开发任务".into());
        }
        topic_path(root, topic)?;
    }
    Ok(bound.map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_project(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kanzei-topic-{label}-{}-{}",
            std::process::id(),
            crate::run::now_ms()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn topic_classification_and_creation_preserve_existing_material() {
        let root = temporary_project("classification");
        create_topic(&root, "new-topic", "正式课题").unwrap();
        let path = root.join(".kanzei/research/new-topic");
        assert_eq!(describe_topic(&path, false).unwrap().kind, "research");
        assert!(create_topic(&root, "new-topic", "覆盖标题").is_err());
        assert_eq!(describe_topic(&path, false).unwrap().title, "正式课题");
        assert!(create_topic(&root, "../outside", "越界").is_err());
        let dev = root.join(".kanzei/research/recon");
        std::fs::create_dir(&dev).unwrap();
        std::fs::write(dev.join("report.md"), "---\nkind: dev_recon\n---\n开发勘察").unwrap();
        assert_eq!(describe_topic(&dev, false).unwrap().kind, "dev_recon");
        std::fs::write(dev.join("report.md"), "历史报告").unwrap();
        assert_eq!(describe_topic(&dev, false).unwrap().kind, "unclassified");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bound_topic_rejects_cross_topic_and_development_requests() {
        let root = temporary_project("binding");
        create_topic(&root, "topic-a", "课题 A").unwrap();
        assert_eq!(
            validate_run_topic(&root, Some("research"), Some("topic-a"), Some("topic-a")).unwrap(),
            Some("topic-a".into())
        );
        assert!(
            validate_run_topic(&root, Some("research"), Some("topic-a"), Some("topic-b")).is_err()
        );
        assert!(validate_run_topic(&root, Some("dev"), Some("topic-a"), None).is_err());
        assert!(validate_run_topic(&root, Some("research"), None, Some("topic-a")).is_err());
        assert!(validate_run_topic(&root, Some("dev"), None, None)
            .unwrap()
            .is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn created_research_process_restores_binding_and_projects_ipc_field() {
        let root = temporary_project("process");
        create_topic(&root, "topic-a", "课题 A").unwrap();
        let state = crate::AppState::default();
        let create = |topic: &str| {
            crate::processes::lifecycle::create_process_with_tracker(
                &state,
                root.to_str().unwrap(),
                None,
                Some("research".into()),
                None,
                Some(false),
                Some(true),
                Some(false),
                None,
                None,
                Some(topic.to_string()),
            )
        };
        assert!(create("missing-topic").await.is_err());
        let info = create("topic-a").await.unwrap();
        assert_eq!(
            serde_json::to_value(&info).unwrap()["research_topic"],
            "topic-a"
        );
        let restarted = crate::AppState::default();
        crate::processes::restore_processes_from_store(
            &restarted,
            &crate::normalized_project_root(&root),
        )
        .unwrap();
        let handle = restarted
            .processes
            .lock()
            .unwrap()
            .get(&info.id)
            .unwrap()
            .clone();
        assert_eq!(
            handle.research_topic.lock().unwrap().as_deref(),
            Some("topic-a")
        );
        assert_eq!(handle.profile.lock().unwrap().as_deref(), Some("research"));
        let snapshot = crate::docs::docs_snapshot(root.display().to_string()).unwrap();
        assert_eq!(snapshot["research_topics"][0]["label"], "课题 A");
        assert_eq!(snapshot["research_topics"][0]["kind"], "research");
        std::fs::remove_dir_all(root).unwrap();
    }
}
