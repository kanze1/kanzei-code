//! read 工具。设计红线 2:流式读取,永不整读文件。
//! - offset/limit:BufReader 逐行流式,跳过即丢弃;
//! - tail:从文件尾反向 64KiB 分块 seek,凑够行数即停。

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolImage, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

const DEFAULT_LIMIT: usize = 2000;
const MAX_LINE_CHARS: usize = 500;
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const TAIL_CHUNK: u64 = 64 * 1024;
/// R-249:图片原始字节上限。base64 后约 4/3 倍,3.75 MiB → 5 MB,贴着
/// Anthropic 单图 5 MB 的硬限。超限直接拒绝并说明,不静默截断
/// ——截断的图片解不出来,provider 400 的报错会指向别处,很难查。
const MAX_IMAGE_BYTES: usize = 3_932_160;

#[derive(Deserialize, JsonSchema)]
struct ReadInput {
    /// 文件路径(绝对或相对 cwd)
    #[serde(alias = "file_path", alias = "filepath", alias = "file")]
    path: String,
    /// 起始行号(1-based)
    #[serde(default)]
    offset: Option<usize>,
    /// 最多读多少行(默认 2000)
    #[serde(default)]
    limit: Option<usize>,
    /// 只读最后 N 行(与 offset 互斥,反向 seek 实现)
    #[serde(default)]
    tail: Option<usize>,
}

pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &'static str {
        "read"
    }

    fn description(&self) -> String {
        "Read a file. Text files: params path; optional offset (1-based line), limit (max lines), \
         tail (last N lines). Image files (PNG/JPEG/WebP/GIF) are detected by content and returned \
         as a viewable image — offset/limit/tail do not apply to them."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(ReadInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["path"].as_str().unwrap_or("*").to_string()]
    }

    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::shared_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: ReadInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let path = ctx
            .cwd
            .join(kanzei_harness::permission::normalize_resource(&input.path));
        let path_for_read = path.clone();
        let result = tokio::task::spawn_blocking(move || read_any(&path_for_read, &input)).await;
        match result {
            Ok(Ok(ReadPayload::Text(text))) => {
                // R-161 采纳盲区:read 读记忆文件正文 = 这次召回起了作用,
                // 回填 fetched(与 memory_search 回填同口径,CLI/桌面同源)。
                crate::memory::mark_memory_file_read(&ctx.project_root, &path);
                ToolOutput::ok(text)
            }
            Ok(Ok(ReadPayload::Image {
                media_type,
                data,
                bytes,
            })) => {
                // 文本位仍要给一句实话:模型看得到图,但轨迹与 UI 只留这行。
                let summary = format!(
                    "[image] {} ({media_type}, {bytes} bytes) — attached to this tool result.",
                    path.display()
                );
                ToolOutput::ok(summary).with_images(vec![ToolImage { media_type, data }])
            }
            Ok(Err(e)) => ToolOutput::error(e),
            Err(e) => ToolOutput::error(format!("read task panicked: {e}")),
        }
    }
}

/// read 的两种载荷。图片不走行切分,offset/limit/tail 对它没有意义。
enum ReadPayload {
    Text(String),
    Image {
        media_type: String,
        data: String,
        bytes: usize,
    },
}

/// R-249:**按内容判定**图片,不看扩展名。
///
/// 扩展名会骗人:`.png` 结尾的其实是 jpeg 时,media_type 与实际字节不符,
/// provider 直接 400,而报错指向请求体、不指向这个文件,极难定位。magic bytes
/// 同时让没有扩展名的截图也能读。
fn sniff_image(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    // WebP = RIFF 容器,第 8..12 字节是 "WEBP"。
    if head.len() >= 12 && head.starts_with(b"RIFF") && &head[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

fn read_any(path: &std::path::Path, input: &ReadInput) -> Result<ReadPayload, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if meta.is_dir() {
        return Err(format!("{} is a directory", path.display()));
    }

    let mut head = [0u8; 12];
    let n = file.read(&mut head).map_err(|e| e.to_string())?;
    if let Some(media_type) = sniff_image(&head[..n]) {
        let bytes = meta.len() as usize;
        if bytes > MAX_IMAGE_BYTES {
            return Err(format!(
                "{} is {bytes} bytes ({media_type}); the limit is {MAX_IMAGE_BYTES} bytes \
                 (~5 MB once base64-encoded). Resize or crop it first.",
                path.display()
            ));
        }
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut raw = Vec::with_capacity(bytes);
        file.read_to_end(&mut raw).map_err(|e| e.to_string())?;
        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD.encode(&raw);
        return Ok(ReadPayload::Image {
            media_type: media_type.to_string(),
            data,
            bytes: raw.len(),
        });
    }

    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    read_sync_from(file, meta.len(), path, input).map(ReadPayload::Text)
}

/// 文本读取正文。文件已开好、图片已在 [`read_any`] 里分流走,这里只管文本。
fn read_sync_from(
    mut file: std::fs::File,
    len: u64,
    path: &std::path::Path,
    input: &ReadInput,
) -> Result<String, String> {
    // 二进制探测:前 8KiB 含 NUL 即拒绝。图片不会走到这里(read_any 已按 magic
    // bytes 分流),所以这条拒绝仍然只针对「既不是文本也不是可视图片」的字节流。
    let mut probe = [0u8; 8192];
    let n = file.read(&mut probe).map_err(|e| e.to_string())?;
    if probe[..n].contains(&0) {
        return Err(format!(
            "{} looks binary ({len} bytes); read refuses binary files",
            path.display()
        ));
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;

    if let Some(tail) = input.tail {
        return read_tail(&mut file, len, tail);
    }

    let offset = input.offset.unwrap_or(1).max(1);
    let limit = input.limit.unwrap_or(DEFAULT_LIMIT);
    let reader = BufReader::new(file);
    let mut out = String::new();
    let mut total_bytes = 0usize;
    let mut shown = 0usize;
    let mut line_no = 0usize;
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        line_no += 1;
        if line_no < offset {
            continue;
        }
        if shown >= limit || total_bytes >= MAX_OUTPUT_BYTES {
            out.push_str(&format!(
                "... (truncated at line {line_no}; use offset to continue)\n"
            ));
            break;
        }
        let rendered = render_line(line_no, &line);
        total_bytes += rendered.len();
        out.push_str(&rendered);
        shown += 1;
    }
    if shown == 0 {
        return Ok(format!(
            "(empty range: file has {line_no} lines, offset was {offset})"
        ));
    }
    Ok(out)
}

/// 反向分块 seek:凑够 n+1 个换行或到文件头即停,内存上界 = 收集的字节数。
fn read_tail(file: &mut std::fs::File, len: u64, n: usize) -> Result<String, String> {
    let mut pos = len;
    let mut collected: Vec<u8> = Vec::new();
    while pos > 0 {
        let newlines = collected.iter().filter(|&&b| b == b'\n').count();
        if newlines > n || collected.len() >= MAX_OUTPUT_BYTES * 2 {
            break;
        }
        let read_size = TAIL_CHUNK.min(pos);
        pos -= read_size;
        file.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
        let mut chunk = vec![0u8; read_size as usize];
        file.read_exact(&mut chunk).map_err(|e| e.to_string())?;
        chunk.extend_from_slice(&collected);
        collected = chunk;
    }
    let text = String::from_utf8_lossy(&collected);
    let mut lines: Vec<&str> = text.lines().collect();
    let from_start = pos == 0 && lines.len() <= n;
    if lines.len() > n {
        lines = lines.split_off(lines.len() - n);
    }
    let mut out = String::new();
    if !from_start {
        out.push_str(&format!(
            "(last {} lines of {})\n",
            lines.len(),
            human_bytes(len)
        ));
    }
    for (i, line) in lines.iter().enumerate() {
        out.push_str(&render_line(i + 1, line));
    }
    Ok(out)
}

fn render_line(no: usize, line: &str) -> String {
    let mut text = line;
    let truncated = match line.char_indices().nth(MAX_LINE_CHARS) {
        Some((idx, _)) => {
            text = &line[..idx];
            true
        }
        None => false,
    };
    format!(
        "{no:>6}\t{text}{}\n",
        if truncated {
            " …(line truncated)"
        } else {
            ""
        }
    )
}

fn human_bytes(n: u64) -> String {
    if n > 1024 * 1024 {
        format!("{:.1} MiB", n as f64 / 1048576.0)
    } else {
        format!("{:.1} KiB", n as f64 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::Tool;
    use serde_json::json;

    fn temp_project() -> (std::path::PathBuf, ToolCtx) {
        let dir = std::env::temp_dir().join(format!(
            "kz-readtool-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (
            dir.clone(),
            ToolCtx {
                cwd: dir.clone(),
                project_root: dir.clone(),
                ..Default::default()
            },
        )
    }

    #[tokio::test]
    async fn read_memory_file_backfills_recall_fetched() {
        // R-161 验收②:read 工具读 .kanzei/memory/ 下的记忆文件正文 = 这次召回
        // 被采纳,必须回填 fetched(此前只有 memory_search 回填,read 是盲区)。
        let (dir, ctx) = temp_project();
        let store = crate::memory::MemoryStore::project(&dir);
        let entry = match store
            .add(
                "sop",
                "发版 SOP 两条通道",
                "发版发布安装更新必读",
                "package.ps1 -Publish 后静默装 setup",
                "user",
                &[],
                None,
                false,
            )
            .unwrap()
        {
            crate::memory::AddOutcome::Added(e) => e,
            _ => panic!("expected add"),
        };
        let hits = store.search("发版", None, Some("active"), 5).unwrap();
        assert!(!hits.is_empty());
        // 制造一次召回:此时 fetched=0(召回≠采纳)。
        let recall_id = store.record_recall("这轮要发版", &hits, 128);
        let rounds = store.recalls(10);
        assert!(
            !rounds[0]
                .hits
                .iter()
                .find(|h| h.id == entry.id)
                .unwrap()
                .fetched
        );

        // 通过 ReadTool 读该记忆文件 → 回填 fetched。
        let path = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == entry.id)
            .map(|(p, _)| p)
            .unwrap();
        let out = ReadTool
            .execute(json!({"path": path.to_string_lossy()}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("package.ps1"), "{}", out.content);

        // 回填只作用于最近一次召回的同一条目。
        let after = store.recalls(10);
        let hit = after
            .iter()
            .find(|r| r.recall_id == recall_id)
            .unwrap()
            .hits
            .iter()
            .find(|h| h.id == entry.id)
            .unwrap();
        assert!(hit.fetched, "read 记忆文件后未回填采纳");
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn read_non_memory_file_does_not_touch_fetched() {
        // 普通文件 read 不应触发任何记忆回填副作用。
        let (dir, ctx) = temp_project();
        let plain = dir.join("notes.md");
        std::fs::write(&plain, "普通笔记").unwrap();
        let out = ReadTool.execute(json!({"path": "notes.md"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("普通笔记"));
        // 项目记忆库根目录不应因这次 read 被创建(快速路径短路,无副作用)。
        assert!(
            !dir.join(".kanzei").join("memory").exists(),
            "非记忆文件的 read 不应创建记忆库目录"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    // ---- R-249:图片读取 ----

    /// 1x1 PNG(最小合法 PNG),用于走通 magic bytes → base64 → ToolImage 全链路。
    fn tiny_png() -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(
                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
            )
            .unwrap()
    }

    #[tokio::test]
    async fn read_png_returns_image_part() {
        let (dir, ctx) = temp_project();
        let png = dir.join("shot.png");
        std::fs::write(&png, tiny_png()).unwrap();
        let out = ReadTool.execute(json!({"path": "shot.png"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert_eq!(out.images.len(), 1, "PNG 必须作为图片返回");
        assert_eq!(out.images[0].media_type, "image/png");
        assert!(!out.images[0].data.is_empty(), "base64 载荷为空");
        // 文本位仍要说清楚发生了什么(轨迹与 UI 只看得到这行)。
        assert!(out.content.contains("[image]"), "{}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn read_detects_image_by_content_not_extension() {
        // 扩展名撒谎时必须以内容为准:media_type 与真实字节不符会让 provider 400,
        // 且报错指向请求体、不指向这个文件,极难定位。
        let (dir, ctx) = temp_project();
        std::fs::write(dir.join("actually-png.txt"), tiny_png()).unwrap();
        let out = ReadTool
            .execute(json!({"path": "actually-png.txt"}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert_eq!(out.images.len(), 1);
        assert_eq!(out.images[0].media_type, "image/png");

        // 反向:.png 结尾但内容是纯文本 → 走文本路径,不伪造 image/png。
        std::fs::write(dir.join("actually-text.png"), "just text").unwrap();
        let out = ReadTool
            .execute(json!({"path": "actually-text.png"}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.images.is_empty(), "纯文本不得被当成图片");
        assert!(out.content.contains("just text"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn sniff_covers_four_formats_and_rejects_others() {
        assert_eq!(sniff_image(&tiny_png()), Some("image/png"));
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_image(b"GIF89a....."), Some("image/gif"));
        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&[0, 0, 0, 0]);
        webp.extend_from_slice(b"WEBP");
        assert_eq!(sniff_image(&webp), Some("image/webp"));
        // RIFF 容器但不是 WebP(如 wav)不得误判。
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&[0, 0, 0, 0]);
        wav.extend_from_slice(b"WAVE");
        assert_eq!(sniff_image(&wav), None);
        assert_eq!(sniff_image(b"# markdown"), None);
        assert_eq!(sniff_image(b""), None);
    }

    #[tokio::test]
    async fn oversized_image_is_rejected_not_truncated() {
        // 截断的图片解不出来,provider 的 400 会指向请求体而不是这个文件。
        let (dir, ctx) = temp_project();
        let mut big = tiny_png();
        big.resize(MAX_IMAGE_BYTES + 1, 0u8);
        std::fs::write(dir.join("big.png"), &big).unwrap();
        let out = ReadTool.execute(json!({"path": "big.png"}), &ctx).await;
        assert!(out.is_error, "超限图片必须报错");
        assert!(out.images.is_empty());
        assert!(out.content.contains("limit"), "{}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn text_read_keeps_images_empty() {
        // 回归:文本路径的 images 必须恒空,否则等于给每条工具结果加了一个空 Part。
        let (dir, ctx) = temp_project();
        std::fs::write(
            dir.join("a.md"),
            "line1
line2
",
        )
        .unwrap();
        let out = ReadTool.execute(json!({"path": "a.md"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.images.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }
}
