//! 文件页未保存草稿的落盘(UX-089)。
//!
//! 编辑器里的未保存修改原来只活在前端内存里(切项目时暂存的草稿也是):关窗、F5、崩溃、更新重启
//! 都会静默丢掉。这里把它们镜像到 `<根>/.kanzei/artifacts/file-drafts/<路径指纹>.json`——
//! `.kanzei/artifacts/` 已在 .gitignore 里,不会进版本库;写策略(files_edit::write_policy)里它属内部目录,
//! 文件页自己也改不了它。
//!
//! 一个文件一份草稿:保存成功、放弃修改、用户选「不保存」时由前端清掉;下次打开文件页时前端整批读回
//! (`file_drafts_load`)当作「暂存的草稿」——树上标琥珀点,打开该文件即恢复,磁盘版本在这期间变了就进冲突态
//! (那条逻辑在前端 17-files-editor.js 的 loadDoc,草稿带着打开时的内容指纹)。
//!
//! 路径只做词法校验(`lexical_segments`),不碰文件系统:被删掉的文件的草稿同样要能存、能读回。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// 草稿目录(相对项目根)。
const DRAFT_DIR: [&str; 3] = [".kanzei", "artifacts", "file-drafts"];

fn draft_dir(root: &Path) -> PathBuf {
    DRAFT_DIR
        .iter()
        .fold(root.to_path_buf(), |dir, part| dir.join(part))
}

/// 规范化后的相对路径(`/` 分隔)。非法路径(绝对、`..`、盘符等)直接拒绝。
fn rel_of(path: &str) -> Result<String, String> {
    Ok(crate::files_edit::lexical_segments(path)?.join("/"))
}

/// 一个相对路径一个文件;名字用路径指纹,避开 Windows 文件名限制与过长路径。
fn draft_file(root: &Path, rel: &str) -> PathBuf {
    draft_dir(root).join(format!(
        "{}.json",
        kanzei_tools::content_hash(rel.as_bytes())
    ))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) struct DraftSave<'a> {
    pub path: &'a str,
    pub content: &'a str,
    /// 草稿基于的磁盘版本指纹(打开/上次保存时);恢复时据此判断磁盘有没有被改过。
    pub hash: Option<&'a str>,
    pub bom: bool,
    pub eol: &'a str,
}

pub(crate) fn save_at(root: &Path, draft: DraftSave) -> Result<(), String> {
    let rel = rel_of(draft.path)?;
    if draft.content.len() as u64 > crate::files_edit::MAX_EDIT_BYTES {
        return Err(format!(
            "草稿超过 {}MB,不落盘",
            crate::files_edit::MAX_EDIT_BYTES / 1024 / 1024
        ));
    }
    let eol = if draft.eol == "crlf" { "crlf" } else { "lf" };
    let text = serde_json::to_string(&json!({
        "path": rel,
        "content": draft.content,
        "hash": draft.hash,
        "bom": draft.bom,
        "eol": eol,
        "savedAtMs": now_ms(),
    }))
    .map_err(|e| e.to_string())?;
    let file = draft_file(root, &rel);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("草稿目录创建失败: {e}"))?;
    }
    kanzei_tools::atomic_file::write_atomic(&file, &text).map_err(|e| format!("草稿写入失败: {e}"))
}

pub(crate) fn clear_at(root: &Path, path: &str) -> Result<(), String> {
    let rel = rel_of(path)?;
    match std::fs::remove_file(draft_file(root, &rel)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("草稿清理失败: {error}")),
    }
}

/// 读回本项目全部草稿(按路径排序);读不了/格式不对的文件跳过——草稿是尽力而为的保险,不该挡住文件页。
pub(crate) fn load_all(root: &Path) -> Vec<Value> {
    let Ok(read) = std::fs::read_dir(draft_dir(root)) else {
        return Vec::new();
    };
    let mut out: Vec<Value> = read
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(|value| value["path"].is_string() && value["content"].is_string())
        .collect();
    out.sort_by(|a, b| {
        a["path"]
            .as_str()
            .unwrap_or_default()
            .cmp(b["path"].as_str().unwrap_or_default())
    });
    out
}

/// 文件页:把一份未保存的草稿镜像到磁盘(前端去抖后调用)。
#[tauri::command]
pub async fn file_draft_save(
    project_dir: String,
    path: String,
    content: String,
    hash: Option<String>,
    bom: Option<bool>,
    eol: Option<String>,
) -> Result<(), String> {
    let root = crate::files_view::resolve_root(&project_dir);
    tauri::async_runtime::spawn_blocking(move || {
        save_at(
            &root,
            DraftSave {
                path: &path,
                content: &content,
                hash: hash.as_deref(),
                bom: bom.unwrap_or(false),
                eol: eol.as_deref().unwrap_or("lf"),
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 文件页:清掉一份草稿(保存成功、放弃修改、选「不保存」之后)。
#[tauri::command]
pub async fn file_draft_clear(project_dir: String, path: String) -> Result<(), String> {
    let root = crate::files_view::resolve_root(&project_dir);
    tauri::async_runtime::spawn_blocking(move || clear_at(&root, &path))
        .await
        .map_err(|e| e.to_string())?
}

/// 文件页:读回本项目全部草稿(`[{ path, content, hash, bom, eol, savedAtMs }]`)。
#[tauri::command]
pub async fn file_drafts_load(project_dir: String) -> Result<Vec<Value>, String> {
    let root = crate::files_view::resolve_root(&project_dir);
    tauri::async_runtime::spawn_blocking(move || load_all(&root))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-files-draft-{tag}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn sample<'a>(path: &'a str, content: &'a str) -> DraftSave<'a> {
        DraftSave {
            path,
            content,
            hash: Some("fnv-0000000000000001"),
            bom: true,
            eol: "crlf",
        }
    }

    #[test]
    fn 草稿存取清理回环_含中文路径与换行元数据() {
        let root = temp_root("roundtrip");
        assert!(load_all(&root).is_empty(), "没有草稿目录时应是空");
        save_at(&root, sample("docs/中文说明.md", "改了一半\r\n")).unwrap();
        save_at(&root, sample("src/lib.rs", "fn a() {}\r\n")).unwrap();
        let all = load_all(&root);
        assert_eq!(all.len(), 2, "{all:?}");
        assert_eq!(all[0]["path"], "docs/中文说明.md", "按路径排序");
        assert_eq!(all[0]["content"], "改了一半\r\n");
        assert_eq!(all[0]["hash"], "fnv-0000000000000001");
        assert_eq!(all[0]["bom"], true);
        assert_eq!(all[0]["eol"], "crlf");

        // 同一路径再存 = 覆盖,不是追加;路径写法(反斜杠、./)不同也落到同一份。
        save_at(&root, sample(".\\src\\lib.rs", "fn b() {}\r\n")).unwrap();
        let all = load_all(&root);
        assert_eq!(all.len(), 2);
        assert_eq!(all[1]["content"], "fn b() {}\r\n");

        clear_at(&root, "src/lib.rs").unwrap();
        clear_at(&root, "src/lib.rs").unwrap(); // 清两次不报错
        let all = load_all(&root);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0]["path"], "docs/中文说明.md");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 草稿拒绝越界路径与超限内容() {
        let root = temp_root("reject");
        for bad in ["../outside.txt", "/abs.txt", ""] {
            assert!(
                save_at(&root, sample(bad, "x")).is_err(),
                "非法路径必须拒绝: {bad:?}"
            );
            assert!(clear_at(&root, bad).is_err(), "清理同样校验路径: {bad:?}");
        }
        let huge = "x".repeat(crate::files_edit::MAX_EDIT_BYTES as usize + 1);
        assert!(save_at(&root, sample("big.txt", &huge)).is_err());
        assert!(load_all(&root).is_empty(), "拒绝的草稿不得留下文件");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 损坏的草稿文件被跳过而不是拖垮读取() {
        let root = temp_root("corrupt");
        save_at(&root, sample("a.txt", "好的")).unwrap();
        std::fs::write(draft_dir(&root).join("broken.json"), "{不是 json").unwrap();
        std::fs::write(draft_dir(&root).join("partial.json"), r#"{"path":"x.txt"}"#).unwrap();
        let all = load_all(&root);
        assert_eq!(all.len(), 1, "{all:?}");
        assert_eq!(all[0]["path"], "a.txt");
        std::fs::remove_dir_all(root).ok();
    }

    /// 前端 17-files-editor.js 与预览夹具共读的形状。
    #[test]
    fn file_drafts_load_形状与ipc契约一致() {
        let root = temp_root("contract");
        save_at(&root, sample("src/lib.rs", "fn a() {}\n")).unwrap();
        let actual = crate::ipc_contract::shape(&json!(load_all(&root)));
        crate::ipc_contract::tests::check_contract(
            "file_drafts_load",
            actual,
            "file_drafts_load 的 IPC 形状变了",
        );
        std::fs::remove_dir_all(root).ok();
    }
}
