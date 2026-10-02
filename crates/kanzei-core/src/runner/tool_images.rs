//! 工具截图落盘与对话回显(UI2-0926 #8,docs/design/preview_pane.md §5「截图回显」)。
//!
//! 此前 browser / ui_screenshot / plot 的图片只进模型:ToolEnd 事件不带图,用户在对话里
//! 看不到模型看到的画面。这里在统一消费出口([`super::tool_exec::materialize_tool_output`])
//! 把 `output.images` 按内容 sha256 写到 `.kanzei/artifacts/tool-images/<sha>.png`,
//! 并在 content **末尾**追加一行 `[tool-image] <相对路径>`:
//! - 只收 image/png:IPC 契约与前端(24-preview.js 的 TOOL_IMAGE_REL、tool_image 命令)只认
//!   `<sha>.png`,别的格式写出来只会在对话里露出一行认不出的标记;截图类工具产出的都是 PNG;
//! - `read` 读图片文件不落盘:源文件本来就在盘上,复制一份只占配额;
//! - 实时显示与历史回放是同一机制——回放读的就是消息里的 ToolResult content;
//! - 模型也看得到这个路径,需要时可以直接 `deliver` 给用户;
//! - 标记在外置/截断**之后**追加,1 MiB 以上结果被外置时也不会丢(风险 10)。
//!
//! 配额:与 D-349 大结果外置同属一个 2 GiB 配额(tool-results + tool-images 合计),
//! 同一把跨进程配额锁。超额或拿不到锁时只是不落盘、不加标记,模型照常收到图片。
//! 清理:每个项目根在本进程第一次落图时(即应用启动后的第一次)按「最近 300 张、14 天内」
//! 清理一次,之后每新写 50 张再清一次。清理按修改时间;同内容再次被引用时刷新修改时间,
//! 免得一张 15 天前写过、今天又出现在新消息里的截图在下一轮清理时被删。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use base64::Engine;
use sha2::Digest;

/// 截图目录(相对项目根)。
pub(crate) const TOOL_IMAGES_DIR: &str = ".kanzei/artifacts/tool-images";
/// content 里的回显标记前缀。前端 extractToolImages 按它识别。
pub(crate) const TOOL_IMAGE_MARKER: &str = "[tool-image]";
/// 清理:保留最近这么多张。
const KEEP_COUNT: usize = 300;
/// 清理:超过这么多天的删掉。
const KEEP_AGE: Duration = Duration::from_secs(14 * 24 * 3600);
/// 每新写这么多张再清一次(防止单次会话里无界增长)。
const PRUNE_EVERY_NEW: usize = 50;
/// 单张解码后上限;超出的不落盘(截图本身有 4 MB base64 的上游上限)。
const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;

/// 每个项目根在本进程里新写了多少张(None = 还没做过首次清理)。
static NEW_SINCE_PRUNE: Mutex<Option<HashMap<PathBuf, usize>>> = Mutex::new(None);

/// 落盘的唯一格式(与 IPC 契约 `<sha>.png` 一致)。
const PERSISTED_MEDIA_TYPE: &str = "image/png";
/// 图片来自盘上已有文件的工具:不复制、不加标记。
const SOURCE_ON_DISK_TOOLS: [&str; 1] = ["read"];

/// tool-images 目录的总字节数(不存在记 0)。与 tool-results 合计进同一配额。
pub(crate) fn tool_images_bytes(project_root: &Path) -> std::io::Result<u64> {
    let dir = project_root.join(TOOL_IMAGES_DIR);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut total = 0u64;
    for entry in entries {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_file() {
            total = total.saturating_add(meta.len());
        }
    }
    Ok(total)
}

/// 把 output.images 落盘,返回要追加到 content 末尾的标记行(已存在的同内容图片直接复用)。
pub(crate) fn persist_tool_images(
    output: &kanzei_harness::ToolOutput,
    tool_name: &str,
    project_root: &Path,
    quota_bytes: u64,
    lock_budget: Duration,
) -> Vec<String> {
    if output.images.is_empty()
        || project_root.as_os_str().is_empty()
        || SOURCE_ON_DISK_TOOLS.contains(&tool_name)
    {
        return Vec::new();
    }
    prune_once(project_root);
    let mut markers = Vec::new();
    for image in &output.images {
        if !image.media_type.eq_ignore_ascii_case(PERSISTED_MEDIA_TYPE) {
            continue;
        }
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(image.data.as_bytes())
        else {
            continue;
        };
        if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
            continue;
        }
        let sha: String = sha2::Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let rel = format!("{TOOL_IMAGES_DIR}/{sha}.png");
        let path = project_root.join(&rel);
        match store(project_root, &path, &bytes, quota_bytes, lock_budget) {
            Ok(written) => {
                if written {
                    note_new_image(project_root);
                }
                markers.push(format!("{TOOL_IMAGE_MARKER} {rel}"));
            }
            Err(reason) => {
                tracing::warn!(%reason, path = %rel, "工具截图未落盘(对话里不回显,模型照常收到图片)");
            }
        }
    }
    markers
}

/// 把标记行追加到 content 末尾(每行一个,位于最后)。
pub(crate) fn append_markers(output: &mut kanzei_harness::ToolOutput, markers: &[String]) {
    if markers.is_empty() {
        return;
    }
    if !output.content.is_empty() && !output.content.ends_with('\n') {
        output.content.push('\n');
    }
    output.content.push_str(&markers.join("\n"));
}

/// 落盘一张;返回是否新写。同名同长度视为已存在(内容寻址),不取锁、不计配额,
/// 但刷新修改时间——清理按修改时间判龄,被新消息再次引用的图不能按「写入那天」算。
fn store(
    project_root: &Path,
    path: &Path,
    bytes: &[u8],
    quota_bytes: u64,
    lock_budget: Duration,
) -> Result<bool, String> {
    let existing = |path: &Path| {
        std::fs::symlink_metadata(path)
            .ok()
            .filter(|meta| meta.is_file() && meta.len() == bytes.len() as u64)
            .is_some()
    };
    if existing(path) {
        touch(path);
        return Ok(false);
    }
    let _guard = match super::tool_exec::lock_tool_result_storage(project_root, lock_budget) {
        Ok(Some(guard)) => guard,
        Ok(None) => return Err("配额锁等待超时".into()),
        Err(error) => return Err(format!("配额锁不可用: {error}")),
    };
    if existing(path) {
        touch(path);
        return Ok(false);
    }
    let used = super::tool_exec::artifact_storage_bytes(project_root)
        .map_err(|error| format!("无法计量工具产物占用: {error}"))?;
    if used.saturating_add(bytes.len() as u64) > quota_bytes {
        return Err(format!(
            "工具产物已达配额(占用 {used} 字节,配额 {quota_bytes} 字节)"
        ));
    }
    kanzei_base::atomic_file::write_atomic_bytes(path, bytes).map_err(|error| error.to_string())?;
    Ok(true)
}

/// 把修改时间刷成现在(失败只记日志:最坏是这张图按旧日期被清理)。
fn touch(path: &Path) {
    let result = std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(SystemTime::now()));
    if let Err(error) = result {
        tracing::warn!(%error, path = %path.display(), "工具截图复用时没能刷新修改时间");
    }
}

fn prune_once(project_root: &Path) {
    let first = {
        let Ok(mut guard) = NEW_SINCE_PRUNE.lock() else {
            return;
        };
        let map = guard.get_or_insert_with(HashMap::new);
        if map.contains_key(project_root) {
            false
        } else {
            map.insert(project_root.to_path_buf(), 0);
            true
        }
    };
    if first {
        prune_tool_images(
            &project_root.join(TOOL_IMAGES_DIR),
            KEEP_COUNT,
            KEEP_AGE,
            SystemTime::now(),
        );
    }
}

fn note_new_image(project_root: &Path) {
    let due = {
        let Ok(mut guard) = NEW_SINCE_PRUNE.lock() else {
            return;
        };
        let count = guard
            .get_or_insert_with(HashMap::new)
            .entry(project_root.to_path_buf())
            .or_insert(0);
        *count += 1;
        if *count >= PRUNE_EVERY_NEW {
            *count = 0;
            true
        } else {
            false
        }
    };
    if due {
        prune_tool_images(
            &project_root.join(TOOL_IMAGES_DIR),
            KEEP_COUNT,
            KEEP_AGE,
            SystemTime::now(),
        );
    }
}

/// 清理:按修改时间从新到旧,保留前 `keep` 张且不早于 `max_age` 的,其余删除。返回删除数。
pub(crate) fn prune_tool_images(
    dir: &Path,
    keep: usize,
    max_age: Duration,
    now: SystemTime,
) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some((meta.modified().ok()?, entry.path()))
        })
        .collect();
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let mut removed = 0;
    for (index, (modified, path)) in files.iter().enumerate() {
        let too_old = now
            .duration_since(*modified)
            .map(|age| age > max_age)
            .unwrap_or(false);
        if (index >= keep || too_old) && std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::{ToolCtx, ToolImage, ToolOutput};

    fn root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-tool-images-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn png(seed: u8) -> String {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend([seed; 32]);
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn shot(content: &str, seeds: &[u8]) -> ToolOutput {
        ToolOutput::ok(content).with_images(
            seeds
                .iter()
                .map(|seed| ToolImage {
                    media_type: "image/png".into(),
                    data: png(*seed),
                })
                .collect(),
        )
    }

    fn marker_paths(content: &str) -> Vec<String> {
        content
            .lines()
            .filter_map(|line| line.strip_prefix("[tool-image] "))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn 带图输出落盘且content末行是标记() {
        let root = root("marker");
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let mut output = shot("[screenshot] 当前窗口画面", &[1]);
        super::super::tool_exec::materialize_tool_output(&mut output, &ctx, "ui_screenshot");
        let last = output.content.lines().last().unwrap().to_string();
        assert!(
            last.starts_with("[tool-image] .kanzei/artifacts/tool-images/"),
            "{last}"
        );
        assert!(last.ends_with(".png"), "{last}");
        let rel = last.trim_start_matches("[tool-image] ");
        assert!(root.join(rel).is_file(), "标记指向的文件必须存在");
        assert_eq!(output.images.len(), 1, "模型照常收到图片");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 同图只写一次_不同图各写一张() {
        let root = root("dedupe");
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let mut first = shot("a", &[7]);
        let mut second = shot("b", &[7, 8]);
        super::super::tool_exec::materialize_tool_output(&mut first, &ctx, "browser");
        super::super::tool_exec::materialize_tool_output(&mut second, &ctx, "browser");
        assert_eq!(marker_paths(&first.content).len(), 1);
        let second_paths = marker_paths(&second.content);
        assert_eq!(second_paths.len(), 2);
        assert_eq!(
            marker_paths(&first.content)[0],
            second_paths[0],
            "同内容同路径"
        );
        let files = std::fs::read_dir(root.join(TOOL_IMAGES_DIR))
            .unwrap()
            .count();
        assert_eq!(files, 2, "同图去重,只写两张");
        std::fs::remove_dir_all(&root).ok();
    }

    /// provider 不支持图片时模型拿不到图,但对话回显仍要有(用户看得到模型没看到的东西)。
    #[test]
    fn provider不支持图片时仍落盘并保留标记() {
        let root = root("unsupported");
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let mut output = shot("done", &[3]);
        super::super::tool_exec::materialize_tool_output(&mut output, &ctx, "plot");
        let (parts, note) = super::super::tool_exec::tool_images_to_parts(&output, false);
        assert!(parts.is_empty());
        assert!(note.is_some());
        assert_eq!(marker_paths(&output.model_content()).len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    /// 风险 10:大结果外置后 content 换成引用 + 预览,标记仍在末行。
    #[test]
    fn 大结果外置后标记仍在末行() {
        let root = root("spill");
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let mut output = shot(&"x".repeat(1024 * 1024 + 10), &[5]);
        super::super::tool_exec::materialize_tool_output(&mut output, &ctx, "browser");
        assert!(output.content.starts_with("[tool_result_externalized"));
        assert!(output
            .content
            .lines()
            .last()
            .unwrap()
            .starts_with("[tool-image] "));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 超出合计配额时不落盘也不加标记() {
        let root = root("quota");
        let results = root.join(".kanzei/artifacts/tool-results");
        std::fs::create_dir_all(&results).unwrap();
        std::fs::write(results.join("big.txt"), vec![b'x'; 64]).unwrap();
        let output = shot("x", &[9]);
        // 已占 64 字节,配额 80:再写 40 字节的图就超了。
        let markers = persist_tool_images(&output, "browser", &root, 80, Duration::from_millis(50));
        assert!(markers.is_empty(), "超额不得加标记");
        assert!(!root.join(TOOL_IMAGES_DIR).exists() || tool_images_bytes(&root).unwrap() == 0);
        // 配额充足时照常写;图片字节计入合计口径。
        let markers =
            persist_tool_images(&output, "browser", &root, 1024, Duration::from_millis(50));
        assert_eq!(markers.len(), 1);
        assert_eq!(tool_images_bytes(&root).unwrap(), 40);
        assert_eq!(
            super::super::tool_exec::artifact_storage_bytes(&root).unwrap(),
            104
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 无效或非图片类型的负载跳过() {
        let root = root("invalid");
        let mut output = ToolOutput::ok("x").with_images(vec![
            ToolImage {
                media_type: "image/png".into(),
                data: "not base64 !!".into(),
            },
            ToolImage {
                media_type: "application/pdf".into(),
                data: png(1),
            },
        ]);
        let markers = persist_tool_images(
            &output,
            "browser",
            &root,
            u64::MAX,
            Duration::from_millis(50),
        );
        assert!(markers.is_empty());
        append_markers(&mut output, &markers);
        assert_eq!(output.content, "x", "没有标记时 content 逐字节不变");
        // 没有项目根(ToolCtx::default)时不往进程 cwd 写。
        assert!(persist_tool_images(
            &shot("x", &[1]),
            "browser",
            Path::new(""),
            u64::MAX,
            Duration::ZERO
        )
        .is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    /// 复核修复:契约与前端只认 `<sha>.png`。jpeg / webp / gif 不落盘、不加标记(否则对话里
    /// 露出一行认不出的 `[tool-image] ….jpg`);`read` 读图片文件时源文件本来就在盘上,不复制。
    #[test]
    fn 只落png_read读图不复制() {
        let root = root("png-only");
        for media_type in ["image/jpeg", "image/webp", "image/gif"] {
            let output = ToolOutput::ok("x").with_images(vec![ToolImage {
                media_type: media_type.into(),
                data: png(4),
            }]);
            let markers =
                persist_tool_images(&output, "plot", &root, u64::MAX, Duration::from_millis(50));
            assert!(markers.is_empty(), "{media_type} 不得落盘");
        }
        let markers = persist_tool_images(
            &shot("x", &[4]),
            "read",
            &root,
            u64::MAX,
            Duration::from_millis(50),
        );
        assert!(markers.is_empty(), "read 的图片源文件已在盘上");
        assert!(!root.join(TOOL_IMAGES_DIR).exists() || tool_images_bytes(&root).unwrap() == 0);
        let markers = persist_tool_images(
            &shot("x", &[4]),
            "browser",
            &root,
            u64::MAX,
            Duration::from_millis(50),
        );
        assert_eq!(markers.len(), 1);
        assert!(markers[0].ends_with(".png"), "{}", markers[0]);
        std::fs::remove_dir_all(&root).ok();
    }

    /// 复核修复:同内容再次被引用时刷新修改时间,清理按修改时间判龄,不能把今天还在用的图删掉。
    #[test]
    fn 复用已有截图时刷新修改时间() {
        let root = root("touch");
        let output = shot("x", &[6]);
        let markers = persist_tool_images(
            &output,
            "browser",
            &root,
            u64::MAX,
            Duration::from_millis(50),
        );
        let path = root.join(markers[0].trim_start_matches("[tool-image] "));
        let old = SystemTime::now() - Duration::from_secs(15 * 24 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let again = persist_tool_images(
            &output,
            "browser",
            &root,
            u64::MAX,
            Duration::from_millis(50),
        );
        assert_eq!(again, markers, "同内容同路径");
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert!(
            SystemTime::now()
                .duration_since(modified)
                .is_ok_and(|age| age < Duration::from_secs(3600)),
            "复用时必须刷新修改时间"
        );
        assert_eq!(
            prune_tool_images(path.parent().unwrap(), 300, KEEP_AGE, SystemTime::now()),
            0,
            "刚被引用过的图不被清理"
        );
        assert!(path.is_file());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 清理只删超出数量或超龄的文件() {
        let root = root("prune");
        let dir = root.join(TOOL_IMAGES_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::now();
        let day = Duration::from_secs(24 * 3600);
        // 0..4:1、2、3、4 天前;old:20 天前。
        for (name, age) in [
            ("a.png", day),
            ("b.png", day * 2),
            ("c.png", day * 3),
            ("d.png", day * 4),
            ("old.png", day * 20),
        ] {
            let path = dir.join(name);
            std::fs::write(&path, b"x").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(now - age)
                .unwrap();
        }
        // 保留 3 张、14 天:d(第 4 新)与 old(超龄)被删。
        assert_eq!(prune_tool_images(&dir, 3, day * 14, now), 2);
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, vec!["a.png", "b.png", "c.png"]);
        // 再跑一次不再误删。
        assert_eq!(prune_tool_images(&dir, 3, day * 14, now), 0);
        // 数量没超时,只按天数删。
        let stale = dir.join("stale.png");
        std::fs::write(&stale, b"x").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(now - day * 15)
            .unwrap();
        assert_eq!(prune_tool_images(&dir, 10, day * 14, now), 1);
        assert!(!stale.exists(), "超龄的必须删,哪怕数量没超");
        std::fs::remove_dir_all(&root).ok();
    }
}
