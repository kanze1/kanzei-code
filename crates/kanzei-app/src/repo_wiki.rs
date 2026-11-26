//! Repo Wiki 仓库架构百科：
//! - 复用 `kanzei_harness::areas::AreaRegistry` 扫描项目两级代码区域、依赖关系与拓扑层带；
//! - 为每个区域计算源码指纹（相对路径 + 大小 + mtime），精确检测文档是否因代码改动而漂移（stale）；
//! - 将持久化 Markdown 百科页原子写入 `<project>/.kanzei/wiki/`，支持 AI 深度生成与人工修订保存。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use kanzei_harness::areas::{Area, AreaKind, AreaRegistry};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::normalized_project_root;

const WIKI_DIR_REL: &str = ".kanzei/wiki";
const OVERVIEW_AREA_ID: &str = "overview";
const MAX_AREA_SCAN_FILES: usize = 300;
const MAX_PROMPT_SNIPPET_BYTES: usize = 20 * 1024;
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".kanzei",
    "target",
    "node_modules",
    "dist",
    "dist-web",
    "output",
    "build",
    "out",
    "vendor",
    ".next",
    ".venv",
    "__pycache__",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WikiIndex {
    #[serde(default)]
    pages: BTreeMap<String, WikiPageMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WikiPageMeta {
    area_id: String,
    title: String,
    slug: String,
    source_fingerprint: String,
    generated_at: u64,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    file_count: usize,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn wiki_root(project_root: &Path) -> PathBuf {
    project_root.join(WIKI_DIR_REL)
}

fn wiki_index_path(project_root: &Path) -> PathBuf {
    wiki_root(project_root).join("index.json")
}

fn slug_for_area(area_id: &str) -> String {
    let mut out = String::with_capacity(area_id.len());
    for ch in area_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "area".to_string()
    } else {
        trimmed.to_string()
    }
}

fn wiki_page_path(project_root: &Path, area_id: &str) -> PathBuf {
    wiki_root(project_root)
        .join("pages")
        .join(format!("{}.md", slug_for_area(area_id)))
}

fn load_wiki_index(project_root: &Path) -> WikiIndex {
    let path = wiki_index_path(project_root);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<WikiIndex>(&text).ok())
        .unwrap_or_default()
}

fn save_wiki_index(project_root: &Path, index: &WikiIndex) -> Result<(), String> {
    let path = wiki_index_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(index).map_err(|e| e.to_string())?;
    kanzei_base::atomic_file::write_atomic(&path, &json).map_err(|e| e.to_string())
}

fn resolve_area_dir(root: &Path, area: &Area) -> Option<PathBuf> {
    if area.id == OVERVIEW_AREA_ID {
        return Some(root.to_path_buf());
    }
    let direct = root.join(&area.id);
    if direct.exists() {
        return Some(direct);
    }
    let in_crates = root.join("crates").join(&area.id);
    if in_crates.exists() {
        return Some(in_crates);
    }
    if let Some(parent) = &area.parent {
        let sub = area
            .id
            .strip_prefix(&format!("{parent}/"))
            .unwrap_or(&area.label);
        for base in [root.join("crates").join(parent), root.join(parent)] {
            for candidate in [
                base.join("src").join(sub),
                base.join("src").join(format!("{sub}.rs")),
                base.join("src").join(format!("{sub}.ts")),
                base.join("src").join(format!("{sub}.tsx")),
                base.join(sub),
            ] {
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn collect_files_recursive(base: &Path, root: &Path, out: &mut Vec<PathBuf>, limit: usize) {
    if out.len() >= limit {
        return;
    }
    if base.is_file() {
        out.push(base.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    let mut children: Vec<_> = entries.filter_map(Result::ok).collect();
    children.sort_by_key(|e| e.file_name());
    for entry in children {
        if out.len() >= limit {
            break;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') && name_str != ".env.example" {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            if SKIP_DIRS.contains(&name_str.as_ref()) {
                continue;
            }
            collect_files_recursive(&path, root, out, limit);
        } else if meta.is_file() {
            let _ = root;
            out.push(path);
        }
    }
}

fn fingerprint_files(root: &Path, files: &[PathBuf]) -> String {
    let mut hasher = Sha256::new();
    for path in files {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        if let Ok(meta) = std::fs::metadata(path) {
            hasher.update(meta.len().to_le_bytes());
            if let Ok(mtime) = meta.modified() {
                if let Ok(dur) = mtime.duration_since(UNIX_EPOCH) {
                    hasher.update(dur.as_secs().to_le_bytes());
                }
            }
        }
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    format!("{digest:x}")
}

fn scan_area_files(root: &Path, area: Option<&Area>) -> (Vec<String>, String) {
    let mut paths = Vec::new();
    match area {
        None => collect_files_recursive(root, root, &mut paths, MAX_AREA_SCAN_FILES),
        Some(a) => {
            if let Some(target) = resolve_area_dir(root, a) {
                collect_files_recursive(&target, root, &mut paths, MAX_AREA_SCAN_FILES);
            }
        }
    }
    let fp = fingerprint_files(root, &paths);
    let rels = paths
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    (rels, fp)
}

fn band_label(band: u32) -> &'static str {
    match band {
        0 => "基础层",
        1 => "引擎层",
        2 => "工具层",
        _ => "入口层",
    }
}

fn page_status(meta: Option<&WikiPageMeta>, current_fp: &str, file_exists: bool) -> &'static str {
    match meta {
        Some(m) if file_exists => {
            if m.source_fingerprint == current_fp {
                "fresh"
            } else {
                "stale"
            }
        }
        _ => "missing",
    }
}

/// 扫描项目代码区域与现有 `.kanzei/wiki/` 持久化文档，返回完整架构概览与漂移检测结果。
#[tauri::command]
pub(crate) fn repo_wiki_overview(project_dir: String) -> Result<Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    if !root.is_dir() {
        return Err("项目目录不存在".into());
    }
    if crate::general_chat::is_general_root(&root) {
        return Err("无项目对话不支持 Repo Wiki，请切换到具体代码项目".into());
    }

    let registry = AreaRegistry::scan(&root);
    let index = load_wiki_index(&root);
    let deps = registry.crate_deps();

    let (overview_files, overview_fp) = scan_area_files(&root, None);
    let overview_meta = index.pages.get(OVERVIEW_AREA_ID);
    let overview_exists = wiki_page_path(&root, OVERVIEW_AREA_ID).is_file();
    let overview_status = page_status(overview_meta, &overview_fp, overview_exists);

    let mut pages = vec![json!({
        "areaId": OVERVIEW_AREA_ID,
        "title": "仓库全局架构总览",
        "kind": "overview",
        "parent": null,
        "band": 4,
        "bandLabel": "全局总览",
        "depth": 0,
        "status": overview_status,
        "fileCount": overview_files.len(),
        "sourceFingerprint": overview_fp,
        "generatedAt": overview_meta.map(|m| m.generated_at),
        "model": overview_meta.and_then(|m| m.model.clone()),
    })];

    let mut stale_count = usize::from(overview_status == "stale");
    let mut fresh_count = usize::from(overview_status == "fresh");

    // 优先展示全部 Crate/顶层区域，再附带其子模块（上限 80 个避免超大仓库卡顿）
    let mut sorted_areas: Vec<&Area> = registry.all().iter().collect();
    sorted_areas.sort_by(|a, b| {
        b.band
            .cmp(&a.band)
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.id.cmp(&b.id))
    });

    for area in sorted_areas.into_iter().take(80) {
        let (files, fp) = scan_area_files(&root, Some(area));
        let meta = index.pages.get(&area.id);
        let exists = wiki_page_path(&root, &area.id).is_file();
        let status = page_status(meta, &fp, exists);
        if status == "stale" {
            stale_count += 1;
        } else if status == "fresh" {
            fresh_count += 1;
        }
        pages.push(json!({
            "areaId": area.id,
            "title": area.label,
            "kind": area.kind.as_str(),
            "parent": area.parent,
            "band": area.band,
            "bandLabel": band_label(area.band),
            "depth": area.depth,
            "status": status,
            "fileCount": files.len(),
            "sourceFingerprint": fp,
            "generatedAt": meta.map(|m| m.generated_at),
            "model": meta.and_then(|m| m.model.clone()),
        }));
    }

    let deps_json: Vec<Value> = deps
        .iter()
        .map(|(from, to)| json!({ "from": from, "to": to }))
        .collect();

    Ok(json!({
        "projectDir": root.display().to_string(),
        "wikiDir": wiki_root(&root).display().to_string(),
        "totalPages": pages.len(),
        "freshCount": fresh_count,
        "staleCount": stale_count,
        "missingCount": pages.len().saturating_sub(fresh_count + stale_count),
        "deps": deps_json,
        "pages": pages,
    }))
}

/// 读取单页 Repo Wiki 文档详情、关联源码文件列表和模块依赖关系。
#[tauri::command]
pub(crate) fn repo_wiki_read(project_dir: String, area_id: String) -> Result<Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    if !root.is_dir() {
        return Err("项目目录不存在".into());
    }
    let clean_id = area_id.trim();
    if clean_id.is_empty() {
        return Err("缺少 areaId".into());
    }

    let registry = AreaRegistry::scan(&root);
    let area = if clean_id == OVERVIEW_AREA_ID {
        None
    } else {
        registry.get(clean_id)
    };
    let (files, current_fp) = scan_area_files(&root, area);
    let index = load_wiki_index(&root);
    let meta = index.pages.get(clean_id);
    let page_path = wiki_page_path(&root, clean_id);
    let content = std::fs::read_to_string(&page_path).ok();
    let status = page_status(meta, &current_fp, content.is_some());

    let deps = registry.crate_deps();
    let outbound: Vec<String> = deps
        .iter()
        .filter(|(from, _)| from == clean_id)
        .map(|(_, to)| to.clone())
        .collect();
    let inbound: Vec<String> = deps
        .iter()
        .filter(|(_, to)| to == clean_id)
        .map(|(from, _)| from.clone())
        .collect();

    let title = if clean_id == OVERVIEW_AREA_ID {
        "仓库全局架构总览".to_string()
    } else {
        area.map(|a| a.label.clone())
            .unwrap_or_else(|| clean_id.to_string())
    };

    Ok(json!({
        "areaId": clean_id,
        "title": title,
        "path": page_path.display().to_string(),
        "status": status,
        "content": content,
        "files": files,
        "outboundDeps": outbound,
        "inboundDeps": inbound,
        "sourceFingerprint": current_fp,
        "generatedAt": meta.map(|m| m.generated_at),
        "model": meta.and_then(|m| m.model.clone()),
    }))
}

fn collect_representative_snippets(root: &Path, rel_files: &[String]) -> String {
    let mut priority = Vec::new();
    let mut others = Vec::new();
    for rel in rel_files {
        let lower = rel.to_ascii_lowercase();
        let is_entry = lower.ends_with("readme.md")
            || lower.ends_with("cargo.toml")
            || lower.ends_with("package.json")
            || lower.ends_with("/lib.rs")
            || lower.ends_with("/main.rs")
            || lower.ends_with("/mod.rs")
            || lower.ends_with("/index.ts")
            || lower.ends_with("/index.tsx")
            || lower.ends_with("/app.tsx")
            || lower == "agents.md";
        if is_entry {
            priority.push(rel);
        } else {
            others.push(rel);
        }
    }
    let mut picked = BTreeSet::new();
    let mut budget = MAX_PROMPT_SNIPPET_BYTES;
    let mut out = String::new();
    for rel in priority.into_iter().chain(others).take(18) {
        if budget < 400 || !picked.insert(rel.as_str()) {
            continue;
        }
        let full = root.join(rel);
        let Ok(raw) = std::fs::read_to_string(&full) else {
            continue;
        };
        let clip_len = raw.len().min(2200).min(budget);
        let mut end = clip_len;
        while end > 0 && !raw.is_char_boundary(end) {
            end -= 1;
        }
        let chunk = &raw[..end];
        out.push_str(&format!("\n### 文件：`{rel}`\n```\n{chunk}\n```\n"));
        budget = budget.saturating_sub(end);
    }
    out
}

const REPO_WIKI_SYSTEM: &str = "你是资深软件架构师，正在为开发团队撰写高精度 Repo Wiki 仓库架构百科文档。\
请严格基于提供的真实文件列表、依赖边与代码摘录撰写，绝不编造不存在的文件或接口。\
输出规范的简体中文 Markdown 文档，必须包含以下章节：\
1. `# <模块名称>` 与一句话职责定位；\
2. `## 核心结构与文件地图`（用表格或列表说明关键文件/子目录及其职责，引用真实相对路径）；\
3. `## 架构与数据流`（用简明 Mermaid `flowchart TD` 或 `flowchart LR` 展示核心组件或依赖流向，节点含特殊字符时用双引号包裹）；\
4. `## 关键不变量与维护注意事项`（列出修改该区域时的核心边界、风险点与验证方式）。\
只输出最终 Markdown 正文，不要加多余寒暄。";

/// 调用当前项目配置的模型为指定代码区域（或全局 `overview`）生成/增量同步 Repo Wiki 页面。
#[tauri::command]
pub(crate) async fn repo_wiki_generate(
    project_dir: String,
    area_id: String,
) -> Result<Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    if !root.is_dir() {
        return Err("项目目录不存在".into());
    }
    if crate::general_chat::is_general_root(&root) {
        return Err("无项目对话不支持生成 Repo Wiki".into());
    }
    let clean_id = area_id.trim();
    if clean_id.is_empty() {
        return Err("缺少 areaId".into());
    }

    let registry = AreaRegistry::scan(&root);
    let area = if clean_id == OVERVIEW_AREA_ID {
        None
    } else {
        registry.get(clean_id)
    };
    let (files, current_fp) = scan_area_files(&root, area);
    let deps = registry.crate_deps();
    let title = if clean_id == OVERVIEW_AREA_ID {
        "仓库全局架构总览".to_string()
    } else {
        area.map(|a| format!("{} ({})", a.label, a.id))
            .unwrap_or_else(|| clean_id.to_string())
    };

    let areas_overview = registry
        .all()
        .iter()
        .filter(|a| a.kind == AreaKind::Crate)
        .map(|a| format!("- `{}`（{}，深度 {}）", a.id, band_label(a.band), a.depth))
        .collect::<Vec<_>>()
        .join("\n");

    let deps_text = if clean_id == OVERVIEW_AREA_ID {
        deps.iter()
            .map(|(a, b)| format!("- `{a}` -> `{b}`"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        deps.iter()
            .filter(|(a, b)| a == clean_id || b == clean_id)
            .map(|(a, b)| format!("- `{a}` -> `{b}`"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let file_listing = files
        .iter()
        .take(120)
        .map(|f| format!("- `{f}`"))
        .collect::<Vec<_>>()
        .join("\n");

    let snippets = collect_representative_snippets(&root, &files);
    let user_prompt = format!(
        "目标百科页：{title}\n项目根目录：{}\n\n## 顶层代码区域\n{}\n\n## 内部依赖关系\n{}\n\n## 该区域文件列表（共 {} 个，展示前 {} 个）\n{}\n\n## 代表性源码摘录\n{}",
        root.display(),
        if areas_overview.is_empty() { "(无)" } else { &areas_overview },
        if deps_text.is_empty() { "(无显式内部依赖)" } else { &deps_text },
        files.len(),
        files.len().min(120),
        if file_listing.is_empty() { "(空)" } else { &file_listing },
        snippets
    );

    let mut config =
        kanzei_harness::KanzeiConfig::load_at_root(&root).map_err(|e| e.to_string())?;
    config.fill_defaults();
    let resolved = config.resolve_model("primary").map_err(|e| e.to_string())?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => kanzei_llm::ProxyConfig::Disabled,
        Some("env") | None => kanzei_llm::ProxyConfig::Env,
        Some(value) => kanzei_llm::ProxyConfig::Explicit(value.to_owned()),
    };
    let route = kanzei_core::build_route(&resolved, &proxy)
        .await
        .map_err(|e| e.to_string())?;
    let client = kanzei_llm::LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let request = kanzei_llm::LlmRequest {
        model: resolved.model.clone(),
        system: vec![REPO_WIKI_SYSTEM.into()],
        messages: vec![kanzei_llm::Message::user_text(&user_prompt)],
        tools: Vec::new(),
        hosted_tools: Vec::new(),
        max_tokens: config.limits.max_tokens().min(6144),
        temperature: None,
        reasoning: kanzei_llm::ReasoningEffort::Off,
        service_tier: config.service_tier_for(&resolved),
    };

    let markdown = tokio::time::timeout(Duration::from_secs(150), async {
        let mut stream = client
            .stream(&route, &request)
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
        Ok::<_, String>(out.trim().to_string())
    })
    .await
    .map_err(|_| "生成 Repo Wiki 超时，请重试".to_string())??;

    if markdown.is_empty() {
        return Err("模型未返回有效的 Wiki 正文".into());
    }

    let page_path = wiki_page_path(&root, clean_id);
    if let Some(parent) = page_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    kanzei_base::atomic_file::write_atomic(&page_path, &markdown).map_err(|e| e.to_string())?;

    let mut index = load_wiki_index(&root);
    let model_id = format!("{}:{}", resolved.provider_name, resolved.model);
    index.pages.insert(
        clean_id.to_string(),
        WikiPageMeta {
            area_id: clean_id.to_string(),
            title: title.clone(),
            slug: slug_for_area(clean_id),
            source_fingerprint: current_fp.clone(),
            generated_at: now_ms(),
            model: Some(model_id),
            file_count: files.len(),
        },
    );
    save_wiki_index(&root, &index)?;

    repo_wiki_read(project_dir, clean_id.to_string())
}

/// 手动保存编辑后的 Repo Wiki 页面，并同步最新源码指纹。
#[tauri::command]
pub(crate) fn repo_wiki_save(
    project_dir: String,
    area_id: String,
    content: String,
) -> Result<Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    if !root.is_dir() {
        return Err("项目目录不存在".into());
    }
    let clean_id = area_id.trim();
    if clean_id.is_empty() {
        return Err("缺少 areaId".into());
    }
    if content.len() > 256 * 1024 {
        return Err("文档体积超过 256 KB 上限".into());
    }
    let registry = AreaRegistry::scan(&root);
    let area = if clean_id == OVERVIEW_AREA_ID {
        None
    } else {
        registry.get(clean_id)
    };
    let (files, current_fp) = scan_area_files(&root, area);
    let page_path = wiki_page_path(&root, clean_id);
    if let Some(parent) = page_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    kanzei_base::atomic_file::write_atomic(&page_path, &content).map_err(|e| e.to_string())?;

    let mut index = load_wiki_index(&root);
    let existing_model = index
        .pages
        .get(clean_id)
        .and_then(|m| m.model.clone())
        .or_else(|| Some("manual".into()));
    let title = if clean_id == OVERVIEW_AREA_ID {
        "仓库全局架构总览".to_string()
    } else {
        area.map(|a| a.label.clone())
            .unwrap_or_else(|| clean_id.to_string())
    };
    index.pages.insert(
        clean_id.to_string(),
        WikiPageMeta {
            area_id: clean_id.to_string(),
            title,
            slug: slug_for_area(clean_id),
            source_fingerprint: current_fp,
            generated_at: now_ms(),
            model: existing_model,
            file_count: files.len(),
        },
    );
    save_wiki_index(&root, &index)?;
    repo_wiki_read(project_dir, clean_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_wiki_overview_detects_missing_fresh_and_stale_after_source_edit() {
        let dir = std::env::temp_dir().join(format!(
            "kanzei-repo-wiki-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(dir.join("app/src")).unwrap();
        std::fs::write(dir.join("app/src/a.ts"), "export const a = 1;").unwrap();
        std::fs::write(dir.join("app/src/b.ts"), "export const b = 2;").unwrap();
        std::fs::write(dir.join("app/src/c.ts"), "export const c = 3;").unwrap();

        let dir_str = dir.display().to_string();
        let initial = repo_wiki_overview(dir_str.clone()).unwrap();
        assert_eq!(initial["pages"][0]["areaId"], "overview");
        assert_eq!(initial["pages"][0]["status"], "missing");

        let saved = repo_wiki_save(
            dir_str.clone(),
            "overview".into(),
            "# 全局总览\n\n测试内容".into(),
        )
        .unwrap();
        assert_eq!(saved["status"], "fresh");

        let after_save = repo_wiki_overview(dir_str.clone()).unwrap();
        assert_eq!(after_save["pages"][0]["status"], "fresh");
        assert_eq!(after_save["freshCount"], 1);

        // 修改源码文件后应立即检测到漂移 (stale)
        std::fs::write(
            dir.join("app/src/a.ts"),
            "export const a = 42; // changed length to guarantee fingerprint diff",
        )
        .unwrap();
        let after_edit = repo_wiki_overview(dir_str).unwrap();
        assert_eq!(after_edit["pages"][0]["status"], "stale");
        assert_eq!(after_edit["staleCount"], 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
