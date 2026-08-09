//! 文件导览页的后端(R-148):快照、预览、AI 用途标注。
//!
//! 独立模块而非塞进 main.rs——那个文件已经 6400 行,本功能恰好是"分析重文件"
//! 的工具,自己先别成为反例。

use std::path::{Path, PathBuf};

use serde_json::json;
use tauri::Emitter;

use kanzei_harness::KanzeiConfig;
use kanzei_llm::{LlmClient, ProxyConfig};
use kanzei_tools::files::{
    aggregate_dirs, annotations_path, content_stamp, load_annotations, save_annotations,
    scan, Annotation,
};

fn resolve_root(project_dir: &str) -> PathBuf {
    kanzei_harness::config::discover_project_root(Path::new(project_dir))
        .unwrap_or_else(|| PathBuf::from(project_dir))
}

/// 整树快照:文件清单 + 目录聚合 + 有效标注。前端树从这一份渲染。
#[tauri::command]
pub fn files_snapshot(project_dir: String) -> Result<serde_json::Value, String> {
    let root = resolve_root(&project_dir);
    let entries = scan(&root);
    let dirs = aggregate_dirs(&entries);
    let annotations = load_annotations(&root);
    let files: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            // 只下发仍然有效的标注:过期的一句话比没有更误导。
            let note = annotations
                .files
                .get(&entry.path)
                .filter(|a| a.hash == entry.stamp)
                .map(|a| a.note.clone());
            json!({
                "path": entry.path,
                "size": entry.size,
                "lines": entry.lines,
                "chars": entry.chars,
                "oversized": entry.oversized,
                "note": note,
            })
        })
        .collect();
    let unannotated = entries
        .iter()
        .filter(|e| {
            !e.oversized
                && annotations
                    .files
                    .get(&e.path)
                    .is_none_or(|a| a.hash != e.stamp)
        })
        .count();
    Ok(json!({
        "files": files,
        "dirs": dirs,
        "dirNotes": annotations.dirs,
        "unannotated": unannotated,
    }))
}

/// 文件预览(Monaco 只读打开)。路径必须落在项目根内;超限文件截断并如实标注。
#[tauri::command]
pub fn file_preview(project_dir: String, path: String) -> Result<serde_json::Value, String> {
    const MAX_PREVIEW_BYTES: u64 = 4 * 1024 * 1024;
    let root = resolve_root(&project_dir);
    let rel = path.trim_matches(['/', '\\']);
    // 逃逸检查:canonicalize 后必须仍在根内。软链接/.. 都在这里挡下。
    let abs = root.join(rel);
    let canon = std::fs::canonicalize(&abs).map_err(|e| format!("无法打开 {rel}: {e}"))?;
    let canon_root = std::fs::canonicalize(&root).map_err(|e| e.to_string())?;
    if !canon.starts_with(&canon_root) {
        return Err(format!("路径越界: {rel}"));
    }
    let meta = std::fs::metadata(&canon).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(format!("不是文件: {rel}"));
    }
    let truncated = meta.len() > MAX_PREVIEW_BYTES;
    let bytes = if truncated {
        use std::io::Read;
        let mut file = std::fs::File::open(&canon).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; MAX_PREVIEW_BYTES as usize];
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        buf.truncate(n);
        buf
    } else {
        std::fs::read(&canon).map_err(|e| e.to_string())?
    };
    // 二进制判据:头 8KB 含 NUL。二进制不进 Monaco,前端显示占位。
    let binary = bytes.iter().take(8192).any(|b| *b == 0);
    let content = if binary {
        String::new()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    Ok(json!({
        "content": content,
        "binary": binary,
        "truncated": truncated,
        "size": meta.len(),
    }))
}

/// AI 用途标注(增量):只标「没标过或内容已变」的文件,逐个调 fast 模型,
/// 每 8 个落一次盘(中途关掉不全丢),每个文件 emit 进度事件。
/// 全部文件标完后再给每个目录聚合一句话。
#[tauri::command]
pub async fn files_annotate(window: tauri::Window, project_dir: String) -> Result<serde_json::Value, String> {
    const HEAD_LINES: usize = 60;
    const SAVE_EVERY: usize = 8;
    let root = resolve_root(&project_dir);
    let entries = scan(&root);
    let mut store = load_annotations(&root);

    let pending: Vec<_> = entries
        .iter()
        .filter(|e| {
            !e.oversized
                && (e.lines.is_some() || e.chars.is_some())
                && store.files.get(&e.path).is_none_or(|a| a.hash != e.stamp)
        })
        .cloned()
        .collect();
    let total = pending.len();
    let mut done = 0usize;
    let mut failed = 0usize;

    let (client, route, model) = fast_route(&root).await?;
    for entry in &pending {
        let abs = root.join(&entry.path);
        let head: String = match std::fs::read_to_string(&abs) {
            Ok(text) => text.lines().take(HEAD_LINES).collect::<Vec<_>>().join("\n"),
            Err(_) => {
                failed += 1;
                continue;
            }
        };
        match annotate_one(&client, &route, &model, &entry.path, &head).await {
            Ok(note) => {
                // 落库前重新取指纹:标注期间文件可能又被改了,绑旧指纹会立即失效,
                // 绑新指纹会把旧内容的标注挂在新内容上——取标注**开始时**的指纹,
                // 变了就宁可下次重标。
                let stamp = std::fs::metadata(&abs)
                    .map(|m| content_stamp(&m))
                    .unwrap_or_else(|_| entry.stamp.clone());
                if stamp == entry.stamp {
                    store.files.insert(entry.path.clone(), Annotation { hash: stamp, note });
                    done += 1;
                } else {
                    failed += 1; // 标注期间被改,下次增量再来
                }
            }
            Err(_) => failed += 1,
        }
        if (done + failed) % SAVE_EVERY == 0 {
            let _ = save_annotations(&root, &store);
        }
        let _ = window.emit(
            "kz:annotate-progress",
            json!({ "done": done, "failed": failed, "total": total }),
        );
    }

    // 目录聚合标注:输入 = 目录下(已有标注的)文件名+一句话,输出目录一句话。
    let dirs = aggregate_dirs(&entries);
    for dir in dirs.keys() {
        let notes: Vec<String> = entries
            .iter()
            .filter(|e| {
                let parent = e.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                parent == dir.as_str()
            })
            .filter_map(|e| {
                let a = store.files.get(&e.path)?;
                (a.hash == e.stamp).then(|| format!("{}: {}", e.path, a.note))
            })
            .collect();
        if notes.is_empty() {
            continue;
        }
        let label = if dir.is_empty() { "(仓库根)" } else { dir };
        if let Ok(note) = annotate_dir(&client, &route, &model, label, &notes).await {
            store.dirs.insert(dir.clone(), note);
        }
    }
    save_annotations(&root, &store).map_err(|e| e.to_string())?;
    Ok(json!({ "annotated": done, "failed": failed, "total": total, "cache": annotations_path(&root).display().to_string() }))
}

async fn fast_route(cwd: &Path) -> Result<(LlmClient, kanzei_llm::Route, String), String> {
    let config = KanzeiConfig::load(cwd).map_err(|e| e.to_string())?;
    let resolved = config.resolve_model("fast").map_err(|e| e.to_string())?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let route = kanzei_core::build_route(&resolved, &proxy)
        .await
        .map_err(|e| e.to_string())?;
    let client = LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    Ok((client, route, resolved.model))
}

async fn fast_one_line(
    client: &LlmClient,
    route: &kanzei_llm::Route,
    model: &str,
    system: &str,
    user: String,
) -> Result<String, String> {
    use futures::StreamExt;
    let request = kanzei_llm::LlmRequest {
        model: model.to_string(),
        system: vec![system.to_string()],
        messages: vec![kanzei_llm::Message::user_text(user)],
        tools: vec![],
        max_tokens: 128,
        temperature: None,
        reasoning: kanzei_llm::ReasoningEffort::Off,
    };
    let mut stream = client.stream(route, &request).await.map_err(|e| e.to_string())?;
    let mut out = String::new();
    while let Some(event) = stream.next().await {
        if let kanzei_llm::LlmEvent::TextDelta { text, .. } = event.map_err(|e| e.to_string())? {
            out.push_str(&text);
        }
    }
    let line = out.lines().next().unwrap_or("").trim().trim_matches('"').to_string();
    if line.is_empty() {
        return Err("空标注".into());
    }
    Ok(line.chars().take(60).collect())
}

async fn annotate_one(
    client: &LlmClient,
    route: &kanzei_llm::Route,
    model: &str,
    path: &str,
    head: &str,
) -> Result<String, String> {
    fast_one_line(
        client,
        route,
        model,
        "用一句中文(20 字内)说明这个文件的用途。只输出这一句,不要前缀、引号或解释。",
        format!("文件: {path}\n\n开头内容:\n{head}"),
    )
    .await
}

async fn annotate_dir(
    client: &LlmClient,
    route: &kanzei_llm::Route,
    model: &str,
    dir: &str,
    notes: &[String],
) -> Result<String, String> {
    fast_one_line(
        client,
        route,
        model,
        "根据目录下各文件的用途,用一句中文(20 字内)概括这个目录的职责。只输出这一句。",
        format!("目录: {dir}\n\n文件用途:\n{}", notes.join("\n")),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 预览的路径逃逸必须被挡:file_preview 是前端直连的读文件通道,
    /// 越界等于把整个磁盘开给 webview。
    #[test]
    fn 预览拒绝越界路径() {
        let root = std::env::temp_dir().join(format!(
            "kz-fv-escape-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        std::fs::write(root.join("ok.txt"), "fine").unwrap();
        let outside = root.parent().unwrap().join(format!(
            "kz-fv-outside-{}.txt",
            std::process::id()
        ));
        std::fs::write(&outside, "secret").unwrap();

        let ok = file_preview(root.display().to_string(), "ok.txt".into()).unwrap();
        assert_eq!(ok["content"], "fine");
        assert_eq!(ok["binary"], false);

        let escape = file_preview(
            root.display().to_string(),
            format!("../{}", outside.file_name().unwrap().to_string_lossy()),
        );
        assert!(escape.is_err(), "越界路径必须拒绝: {escape:?}");
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_file(&outside).ok();
    }

    #[test]
    fn 预览识别二进制并拒进文本通道() {
        let root = std::env::temp_dir().join(format!(
            "kz-fv-bin-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        std::fs::write(root.join("blob.bin"), [0u8, 159, 146, 150]).unwrap();
        let preview = file_preview(root.display().to_string(), "blob.bin".into()).unwrap();
        assert_eq!(preview["binary"], true);
        assert_eq!(preview["content"], "");
        std::fs::remove_dir_all(&root).ok();
    }
}
