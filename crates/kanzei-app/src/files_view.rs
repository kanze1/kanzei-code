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
    aggregate_dirs, annotations_path, load_annotations, save_annotations, scan, Annotation,
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
    // 已标注/待标注都只数「可标注」的文件(代码/md 且未超限)——用户要一眼看到
    // "已标注量/全量"(D-213),分母不含二进制那些永远不会标的。
    let annotatable: Vec<_> = entries
        .iter()
        .filter(|e| !e.oversized && (e.lines.is_some() || e.chars.is_some()))
        .collect();
    let annotated = annotatable
        .iter()
        .filter(|e| {
            annotations
                .files
                .get(&e.path)
                .is_some_and(|a| a.hash == e.stamp)
        })
        .count();
    Ok(json!({
        "files": files,
        "dirs": dirs,
        "dirNotes": annotations.dirs,
        "annotated": annotated,
        "annotatable": annotatable.len(),
        "unannotated": annotatable.len() - annotated,
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
pub async fn files_annotate(
    window: tauri::Window,
    project_dir: String,
) -> Result<serde_json::Value, String> {
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
    // 只有真的写入过标注才落盘(D-213):全失败的运行(fast 模型挂了/思考吃光预算)
    // 每 8 个保存一次会把 load 时的 store 原样重写——若 load 是空的,等于拿空库
    // 反复覆盖缓存文件。dirty 之前一个字节都不碰盘。
    let mut dirty = false;
    // 失败原因必须上浮:231 个文件全 failed 而 UI 只说"231 失败"没有一个字的
    // 原因,正是这次排查花掉半小时的地方。
    let mut first_error: Option<String> = None;

    let backend = annotate_backend(&root).await?;
    for entry in &pending {
        let abs = root.join(&entry.path);
        let (head, bytes_now) = match std::fs::read(&abs) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                let head = text.lines().take(HEAD_LINES).collect::<Vec<_>>().join("\n");
                (head, bytes)
            }
            Err(e) => {
                failed += 1;
                first_error.get_or_insert(format!("{}: {e}", entry.path));
                continue;
            }
        };
        match backend.annotate(&entry.path, &head).await {
            Ok(note) => {
                // 落库前重算内容 hash:标注期间文件被改就丢弃,下次增量再来——
                // 绑旧指纹立即失效,绑新指纹会把旧内容的标注挂在新内容上。
                let stamp_now = kanzei_tools::files::content_hash(&bytes_now);
                if stamp_now == entry.stamp {
                    store.files.insert(
                        entry.path.clone(),
                        Annotation {
                            hash: stamp_now,
                            note,
                        },
                    );
                    done += 1;
                    dirty = true;
                } else {
                    failed += 1;
                }
            }
            Err(e) => {
                failed += 1;
                first_error.get_or_insert(format!("{}: {e}", entry.path));
            }
        }
        if dirty && (done + failed) % SAVE_EVERY == 0 {
            let _ = save_annotations(&root, &store);
        }
        let _ = window.emit(
            "kz:annotate-progress",
            json!({ "done": done, "failed": failed, "total": total }),
        );
    }

    // 目录聚合标注:输入 = 目录下(已有标注的)文件名+一句话,输出目录一句话。
    // 只在本轮有新标注时做——全失败还去标目录纯属浪费。
    if dirty {
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
            let prompt = format!("目录: {label}\n\n文件用途:\n{}", notes.join("\n"));
            if let Ok(note) = backend
                .annotate_raw(
                    "根据目录下各文件的用途,用一句中文(20 字内)概括这个目录的职责。只输出这一句。",
                    &prompt,
                )
                .await
            {
                store.dirs.insert(dir.clone(), note);
            }
        }
        save_annotations(&root, &store).map_err(|e| e.to_string())?;
    }
    Ok(json!({
        "annotated": done,
        "failed": failed,
        "total": total,
        "firstError": first_error,
        "cache": annotations_path(&root).display().to_string(),
    }))
}

const ANNOTATE_SYSTEM: &str =
    "用一句中文(20 字内)说明这个文件的用途——它负责什么,不要复述代码。只输出这一句,不要前缀、引号或解释。";

/// 标注后端。qwen3.5 这类思考模型经 openai 兼容层**关不掉思考**:实测 4b 档一句话
/// 任务思考 1024 token 还没想完,正文永远为空,231 个文件全部失败(D-213)。
/// ollama 的原生 /api/chat 有 think:false,探测到 ollama 就直连原生;其他 provider
/// 走标准 LlmClient(云模型思考可控)。
enum AnnotateBackend {
    OllamaNative {
        base: String,
        model: String,
    },
    Llm {
        client: LlmClient,
        route: kanzei_llm::Route,
        model: String,
        service_tier: Option<String>,
    },
}

async fn annotate_backend(cwd: &Path) -> Result<AnnotateBackend, String> {
    let config = KanzeiConfig::load(cwd).map_err(|e| e.to_string())?;
    let resolved = config.resolve_model("fast").map_err(|e| e.to_string())?;
    let base = resolved.provider.base_url.trim_end_matches('/');
    if resolved.provider_name.contains("ollama") || base.contains("11434") {
        return Ok(AnnotateBackend::OllamaNative {
            base: base.trim_end_matches("/v1").to_string(),
            model: resolved.model,
        });
    }
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let route = kanzei_core::build_route(&resolved, &proxy)
        .await
        .map_err(|e| e.to_string())?;
    let client = LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let service_tier = config.service_tier_for(&resolved);
    Ok(AnnotateBackend::Llm {
        client,
        route,
        model: resolved.model,
        service_tier,
    })
}

impl AnnotateBackend {
    async fn annotate(&self, path: &str, head: &str) -> Result<String, String> {
        self.annotate_raw(
            ANNOTATE_SYSTEM,
            &format!("文件: {path}\n\n开头内容:\n{head}"),
        )
        .await
    }

    async fn annotate_raw(&self, system: &str, user: &str) -> Result<String, String> {
        let raw = match self {
            AnnotateBackend::OllamaNative { base, model } => {
                let body = json!({
                    "model": model,
                    "stream": false,
                    "think": false,
                    "options": { "num_predict": 256 },
                    "messages": [
                        { "role": "system", "content": system },
                        { "role": "user", "content": user },
                    ],
                });
                let response = reqwest::Client::new()
                    .post(format!("{base}/api/chat"))
                    .json(&body)
                    .timeout(std::time::Duration::from_secs(120))
                    .send()
                    .await
                    .map_err(|e| format!("ollama 请求失败: {e}"))?;
                let value: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|e| format!("ollama 响应解析失败: {e}"))?;
                if let Some(err) = value["error"].as_str() {
                    return Err(format!("ollama: {err}"));
                }
                value["message"]["content"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            }
            AnnotateBackend::Llm {
                client,
                route,
                model,
                service_tier,
            } => {
                use futures::StreamExt;
                let request = kanzei_llm::LlmRequest {
                    model: model.clone(),
                    system: vec![system.to_string()],
                    messages: vec![kanzei_llm::Message::user_text(user.to_string())],
                    tools: vec![],
                    // 思考型模型的预算要留足:128 会被思考吃光,正文为空(D-213)。
                    max_tokens: 1024,
                    temperature: None,
                    reasoning: kanzei_llm::ReasoningEffort::Off,
                    service_tier: service_tier.clone(),
                };
                let mut stream = client
                    .stream(route, &request)
                    .await
                    .map_err(|e| e.to_string())?;
                let mut out = String::new();
                while let Some(event) = stream.next().await {
                    if let kanzei_llm::LlmEvent::TextDelta { text, .. } =
                        event.map_err(|e| e.to_string())?
                    {
                        out.push_str(&text);
                    }
                }
                out
            }
        };
        clean_note(&raw)
            .ok_or_else(|| "模型没有产出标注正文(思考模型请确认已关思考或预算充足)".to_string())
    }
}

/// 标注清洗:剥 <think> 块、取最后一个非空行(思考模型正文在末尾)、去引号、封顶 60 字。
fn clean_note(raw: &str) -> Option<String> {
    let mut text = raw.to_string();
    while let (Some(start), Some(end)) = (text.find("<think>"), text.find("</think>")) {
        if end > start {
            text.replace_range(start..end + "</think>".len(), "");
        } else {
            break;
        }
    }
    let line = text
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())?
        .trim_matches(['"', '“', '”', '「', '」'])
        .to_string();
    if line.is_empty() {
        return None;
    }
    Some(line.chars().take(60).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D-213:标注清洗要能从思考模型的输出里捞出正文,捞不出必须报 None
    /// (上游转失败并上浮原因),绝不产出空/垃圾标注。
    #[test]
    fn 标注清洗剥思考块并拒绝空产出() {
        assert_eq!(
            clean_note("glob 文件匹配工具"),
            Some("glob 文件匹配工具".into())
        );
        assert_eq!(
            clean_note("<think>用户要我总结……应该说这是工具</think>\nglob 文件匹配工具"),
            Some("glob 文件匹配工具".into())
        );
        // 思考模型正文在末尾:取最后一个非空行,不是第一行。
        assert_eq!(
            clean_note("先分析一下\n\n结论:异步文件匹配工具"),
            Some("结论:异步文件匹配工具".into())
        );
        assert_eq!(clean_note("\"带引号的答案\""), Some("带引号的答案".into()));
        assert_eq!(clean_note(""), None, "空输出必须报失败,不产出空标注");
        assert_eq!(clean_note("<think>只想没答</think>"), None);
        // 超长截断到 60 字。
        let long = "长".repeat(100);
        assert_eq!(clean_note(&long).unwrap().chars().count(), 60);
    }

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
        let outside = root
            .parent()
            .unwrap()
            .join(format!("kz-fv-outside-{}.txt", std::process::id()));
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
