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
    /// R-326:notebook(.ipynb)只读这些单元格,形如 "1-5" / "3" / "10-20"。
    /// 对普通文本用 offset/limit——那是行号,这里是**单元格序号**,两套坐标别混。
    #[serde(default)]
    cells: Option<String>,
    /// R-326:PDF 只读这些页,形如 "1-5" / "3"。同样是**页码**不是行号。
    #[serde(default)]
    pages: Option<String>,
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
         as a viewable image; offset/limit/tail do not apply to them. Jupyter notebooks (.ipynb) render as numbered cells with source and captured outputs; use `cells` (for example `1-5` or `3`) for a range, which is a CELL index and not a line number. \
         Multiple read calls in the SAME step run in parallel: when you know several files to \
         open, emit them together instead of one per step."
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
        // R-244 批2:read 迁移走统一 pipeline 通道(与 glob 同构;guards/策略/
        // 观察者现阶段空,权限判定在 drive 层)。
        let input2 = input.clone();
        let ctx2 = ctx.clone();
        kanzei_harness::tool_pipeline::run_tool_pipeline(
            "read",
            input,
            ctx,
            &[],
            async move { read_body(self, &input2, &ctx2).await },
            &[],
            &[],
        )
        .await
    }
}

/// R-244 批2:read 工具本体(原 execute 主体),供 pipeline body 调用。
async fn read_body(tool: &dyn Tool, input: &serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
    let input: ReadInput = match crate::parse_input(tool, input.clone()) {
        Ok(v) => v,
        Err(out) => return out,
    };
    let path = ctx
        .cwd
        .join(kanzei_harness::permission::normalize_resource(&input.path));
    let path_for_read = path.clone();
    let project_root = ctx.project_root.clone();
    let result =
        tokio::task::spawn_blocking(move || read_any(&path_for_read, &input, &project_root)).await;
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
        Ok(Err(e)) => {
            if let Some(detail) = e.strip_prefix("READ_RANGE_OUT_OF_BOUNDS: ") {
                ToolOutput::needs_correction("READ_RANGE_OUT_OF_BOUNDS", detail)
            } else if e.starts_with("path not found: ") {
                ToolOutput::failed("READ_PATH_NOT_FOUND", e)
            } else {
                ToolOutput::error(e)
            }
        }
        Err(e) => ToolOutput::error(format!("read task panicked: {e}")),
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

fn read_any(
    path: &std::path::Path,
    input: &ReadInput,
    project_root: &std::path::Path,
) -> Result<ReadPayload, String> {
    let mut file = std::fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            crate::missing_path_hint(path, "", project_root)
        } else {
            format!("cannot open {}: {error}", path.display())
        }
    })?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if meta.is_dir() {
        return Err(format!("{} is a directory", path.display()));
    }

    // R-326:notebook 按**扩展名**分派而不是嗅探内容——.ipynb 就是 JSON,
    // 与普通 .json 在字节上无法区分,只有路径能表达「按 notebook 语义读」。
    if path.extension().is_some_and(|e| e == "ipynb") {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        return read_notebook(&raw, input.cells.as_deref()).map(ReadPayload::Text);
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
    if head[..n].starts_with(b"%PDF-") {
        return read_pdf(path, input).map(ReadPayload::Text);
    }

    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    read_sync_from(file, meta.len(), path, input).map(ReadPayload::Text)
}

fn read_pdf(path: &std::path::Path, input: &ReadInput) -> Result<String, String> {
    let text = pdf_to_text(path)?;
    // R-326:`pages` 走**页**坐标,与 offset/limit 的行坐标是两套,不互相回退。
    // 给了 pages 就整页整页地给,不再按行截断——按行读一份跨页文本,读者根本
    // 不知道自己停在第几页,这正是加页码的原因。
    if let Some(spec) = input.pages.as_deref() {
        return render_pdf_pages(&text, spec);
    }
    let lines: Vec<&str> = text.lines().collect();
    let offset = input.offset.unwrap_or(1).max(1);
    let limit = input.limit.unwrap_or(DEFAULT_LIMIT);
    if offset > lines.len().max(1) {
        return Err(range_error(lines.len(), offset));
    }
    let mut out = String::new();
    let mut shown = 0usize;
    for (index, line) in lines.iter().enumerate().skip(offset.saturating_sub(1)) {
        if shown >= limit || out.len() >= MAX_OUTPUT_BYTES {
            out.push_str(&format!(
                "... (truncated at line {}; use offset to continue)\n",
                index + 1
            ));
            break;
        }
        out.push_str(&render_line(index + 1, line));
        shown += 1;
    }
    if shown == 0 {
        return Ok(format!(
            "(empty range: PDF text has {} lines, offset was {offset})",
            lines.len()
        ));
    }
    Ok(out)
}

/// Extract text from a real PDF using the installed `pdftotext` executable.
/// The path is passed as an argument, never through a shell; missing tooling is explicit.
pub fn pdf_to_text(path: &std::path::Path) -> Result<String, String> {
    let output = std::process::Command::new("pdftotext")
        .arg("-layout")
        .arg(path)
        .arg("-")
        .output()
        // R-326:pdftotext 没装不再直接失败——回落到进程内的 pdf-extract。
        // 保留 pdftotext 为首选是因为 `-layout` 对表格/多栏排版明显更好;
        // 回落只保证「没装 poppler 也能读」,不追求同等排版质量。
        .map_err(|error| {
            pdf_extract::extract_text(path).map_err(|fallback| {
                format!(
                    "读取 {} 失败:pdftotext 未就绪({error}),进程内回落也失败({fallback})。                     装上 poppler 的 pdftotext 可获得更好的排版还原。",
                    path.display()
                )
            })
        });
    let output = match output {
        Ok(output) => output,
        // 回落成功:直接返回其文本,不再走下面的 stdout 解析。
        Err(fallback) => return fallback,
    };
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "pdftotext 读取 {} 失败: {}",
            path.display(),
            if detail.is_empty() {
                "未知错误"
            } else {
                &detail
            }
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    if text.trim().is_empty() {
        return Err(format!("PDF 没有可抽取文本: {}", path.display()));
    }
    Ok(text)
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
    if offset > line_no.max(1) {
        return Err(range_error(line_no, offset));
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

fn range_error(line_count: usize, offset: usize) -> String {
    format!(
        "READ_RANGE_OUT_OF_BOUNDS: requested offset {offset}, but the file has {line_count} lines; legal offset range is 1..={}; use a smaller offset or `tail`.",
        line_count.max(1)
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
        // D-366:决策排序在检索门面(kanzei_tools::memory = kanzei_memory re-export)。
        let index = crate::memory::SqliteMemoryIndex::new(&dir);
        let hits = index.search_entries(
            &crate::memory::IndexQuery::text("发版"),
            None,
            Some("active"),
            5,
        );
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

    #[tokio::test]
    async fn offset_limit_streams_only_requested_lines() {
        let (dir, ctx) = temp_project();
        let path = dir.join("large.log");
        let contents = (1..=20_000)
            .map(|line| format!("line-{line}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, contents).unwrap();
        let out = ReadTool
            .execute(
                json!({"path": "large.log", "offset": 19_999, "limit": 2}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("line-19999"), "{}", out.content);
        assert!(out.content.contains("line-20000"), "{}", out.content);
        assert!(
            !out.content.contains("\tline-1\n"),
            "offset 不应回填前段内容"
        );
        assert!(out.content.len() < 1_000, "offset/limit 结果不应复制整文件");
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn missing_path_and_range_errors_are_recoverable() {
        let (dir, ctx) = temp_project();
        std::fs::write(dir.join("coordinator.rs"), "fn coordinator() {}\n").unwrap();
        let missing = ReadTool
            .execute(json!({"path": "coordinatr.rs"}), &ctx)
            .await;
        assert!(missing.is_error, "{}", missing.content);
        assert_eq!(missing.code, Some("READ_PATH_NOT_FOUND"));
        assert!(
            missing.content.contains("coordinator.rs"),
            "{}",
            missing.content
        );

        let file = dir.join("range.txt");
        std::fs::write(&file, "one\ntwo\n").unwrap();
        let range = ReadTool
            .execute(json!({"path": "range.txt", "offset": 4}), &ctx)
            .await;
        assert!(range.is_error, "{}", range.content);
        assert_eq!(range.code, Some("READ_RANGE_OUT_OF_BOUNDS"));
        assert!(range.content.contains("2 lines"), "{}", range.content);
        assert!(range.content.contains("1..=2"), "{}", range.content);

        let memory = dir.join(".kanzei").join("memory");
        std::fs::create_dir_all(&memory).unwrap();
        std::fs::write(memory.join("M-001-sop.md"), "memory").unwrap();
        let memory_missing = ReadTool
            .execute(json!({"path": ".kanzei/memory/M-002-sop.md"}), &ctx)
            .await;
        assert!(memory_missing.is_error, "{}", memory_missing.content);
        assert!(
            memory_missing
                .content
                .contains(&memory.display().to_string()),
            "{}",
            memory_missing.content
        );
        std::fs::remove_dir_all(dir).ok();
    }

    fn tiny_pdf() -> Vec<u8> {
        let objects = [
            "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
            "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>\nendobj\n",
            "4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
            "5 0 obj\n<< /Length 45 >>\nstream\nBT /F1 12 Tf 72 220 Td (PDF smoke) Tj ET\nendstream\nendobj\n",
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for object in objects {
            offsets.push(pdf.len());
            pdf.extend_from_slice(object.as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        pdf
    }

    #[tokio::test]
    async fn read_pdf_uses_pdftotext_and_keeps_line_window() {
        if std::process::Command::new("pdftotext")
            .arg("-v")
            .output()
            .is_err()
        {
            return;
        }
        let (dir, ctx) = temp_project();
        let path = dir.join("paper.bin");
        std::fs::write(&path, tiny_pdf()).unwrap();
        let out = ReadTool
            .execute(json!({"path": "paper.bin", "offset": 1, "limit": 5}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("PDF smoke"), "{}", out.content);
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

/// R-326:notebook 单元格上限。一个 notebook 动辄几百格,全量倒进上下文没有意义;
/// 超出就截断并如实说明,让模型用 `cells` 缩范围。
const MAX_NOTEBOOK_CELLS: usize = 50;
/// 单个输出块保留的字符数。训练日志/长 traceback 常有几万字符,留头部足够判断。
const MAX_OUTPUT_CHARS: usize = 2000;

/// R-326:把 .ipynb 渲染成「带序号的单元格 + 各自捕获的输出」。
///
/// 为什么不直接把 JSON 交给模型:notebook 的 JSON 里 `source` 是**逐行字符串数组**,
/// 输出还分 stream/execute_result/display_data/error 四种形态,每种把正文藏在不同键下。
/// 让模型自己在原始 JSON 里挑,等于每次读 notebook 都先付一遍解析税,而且极易看漏
/// error 输出——那恰恰是读 notebook 最想看的东西。
///
/// `cells` 是**单元格序号**(1-based,闭区间),与文本的 offset/limit 是两套坐标,
/// 混用会静默读错位置,所以两者在入参上互不回退。
fn read_notebook(raw: &str, cells: Option<&str>) -> Result<String, String> {
    let doc: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("notebook is not valid JSON: {e}"))?;
    let all = doc
        .get("cells")
        .and_then(|c| c.as_array())
        .ok_or_else(|| "notebook has no `cells` array".to_string())?;
    let total = all.len();
    let (from, to) = match cells {
        Some(spec) => parse_cell_range(spec, total)?,
        None => (1, total.min(MAX_NOTEBOOK_CELLS)),
    };

    let mut out = String::new();
    let language = doc
        .pointer("/metadata/kernelspec/language")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    out.push_str(&format!(
        "notebook: {total} cells, kernel language {language}; showing {from}-{to}\n"
    ));
    for (offset, cell) in all[from - 1..to].iter().enumerate() {
        let index = from + offset;
        let kind = cell
            .get("cell_type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        out.push_str(&format!("\n[{index}] {kind}\n"));
        out.push_str(&join_source(cell.get("source")));
        for text in cell
            .get("outputs")
            .and_then(|o| o.as_array())
            .map(|outputs| outputs.iter().filter_map(render_output).collect::<Vec<_>>())
            .unwrap_or_default()
        {
            out.push_str(&format!("  |out| {text}\n"));
        }
    }
    if cells.is_none() && total > MAX_NOTEBOOK_CELLS {
        out.push_str(&format!(
            "\n... ({total} cells total, showed first {MAX_NOTEBOOK_CELLS}; \
             pass `cells` such as \"{}-{total}\" to read further)\n",
            MAX_NOTEBOOK_CELLS + 1
        ));
    }
    Ok(out)
}

/// `source` 在 notebook 里可能是逐行数组,也可能是单个字符串;两种都要吃。
fn join_source(value: Option<&serde_json::Value>) -> String {
    let text = match value {
        Some(serde_json::Value::Array(lines)) => lines
            .iter()
            .filter_map(|l| l.as_str())
            .collect::<Vec<_>>()
            .concat(),
        Some(serde_json::Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    if text.is_empty() {
        return String::new();
    }
    text.lines()
        .map(|line| format!("  {line}\n"))
        .collect::<String>()
}

/// 四种输出形态各把正文藏在不同键下。**error 必须留下**——读 notebook 十有八九
/// 就是为了看它为什么失败,把 traceback 吞掉等于白读。
fn render_output(output: &serde_json::Value) -> Option<String> {
    let kind = output.get("output_type").and_then(|v| v.as_str())?;
    let body = match kind {
        "stream" => join_source(output.get("text")),
        "execute_result" | "display_data" => {
            let data = output.get("data")?;
            match data.get("text/plain") {
                Some(v) => join_source(Some(v)),
                // 非文本输出(图片/HTML)只报类型,不把 base64 倒进上下文。
                None => {
                    let kinds: Vec<&str> = data
                        .as_object()
                        .map(|m| m.keys().map(String::as_str).collect())
                        .unwrap_or_default();
                    format!("<non-text output: {}>", kinds.join(", "))
                }
            }
        }
        "error" => {
            let name = output
                .get("ename")
                .and_then(|v| v.as_str())
                .unwrap_or("error");
            let value = output.get("evalue").and_then(|v| v.as_str()).unwrap_or("");
            let trace = join_source(output.get("traceback"));
            format!("{name}: {value}\n{trace}")
        }
        _ => return None,
    };
    let body = body.trim();
    if body.is_empty() {
        return None;
    }
    let clipped: String = body.chars().take(MAX_OUTPUT_CHARS).collect();
    Some(if clipped.len() < body.len() {
        format!("{clipped} …(output clipped)")
    } else {
        clipped
    })
}

/// 解析 "1-5" / "3" 形态的单元格区间,返回 1-based 闭区间。
/// 越界即报错而不是静默夹取——静默夹取会让模型以为自己读到了它要的那一段。
fn parse_cell_range(spec: &str, total: usize) -> Result<(usize, usize), String> {
    let spec = spec.trim();
    let (from, to) = match spec.split_once('-') {
        Some((a, b)) => (a.trim(), b.trim()),
        None => (spec, spec),
    };
    let parse = |s: &str| -> Result<usize, String> {
        s.parse::<usize>()
            .map_err(|_| format!("invalid cell range `{spec}`; use forms like \"1-5\" or \"3\""))
    };
    let (from, to) = (parse(from)?, parse(to)?);
    if from == 0 || to == 0 {
        return Err("cell indexes are 1-based; 0 is not a cell".into());
    }
    if from > to {
        return Err(format!("cell range `{spec}` is inverted"));
    }
    if from > total {
        return Err(format!(
            "cell range `{spec}` starts past the end; the notebook has {total} cells"
        ));
    }
    Ok((from, to.min(total)))
}

#[cfg(test)]
mod notebook_tests {
    use super::{parse_cell_range, read_notebook};

    const NB: &str = r#"{
      "metadata": {"kernelspec": {"language": "python"}},
      "cells": [
        {"cell_type": "markdown", "source": ["Title\n", "intro\n"]},
        {"cell_type": "code", "source": "print(1)\n",
         "outputs": [{"output_type": "stream", "text": ["1\n"]}]},
        {"cell_type": "code", "source": ["boom()\n"],
         "outputs": [{"output_type": "error", "ename": "ValueError",
                      "evalue": "bad input", "traceback": ["Traceback...\n", "  line\n"]}]},
        {"cell_type": "code", "source": ["img()\n"],
         "outputs": [{"output_type": "display_data", "data": {"image/png": "BASE64BLOB"}}]}
      ]}"#;

    #[test]
    fn 渲染单元格序号与类型并合并逐行source() {
        let out = read_notebook(NB, None).unwrap();
        assert!(out.contains("notebook: 4 cells"), "{out}");
        assert!(out.contains("[1] markdown"), "{out}");
        assert!(out.contains("Title"), "逐行 source 数组要拼回来: {out}");
        assert!(out.contains("[2] code"), "{out}");
        assert!(
            out.contains("print(1)"),
            "字符串形态的 source 也要吃: {out}"
        );
    }

    /// 读 notebook 十有八九是为了看它为什么失败——error 输出绝不能吞。
    #[test]
    fn error输出保留名称原因与traceback() {
        let out = read_notebook(NB, None).unwrap();
        assert!(out.contains("ValueError: bad input"), "{out}");
        assert!(out.contains("Traceback"), "traceback 必须留: {out}");
    }

    /// 非文本输出只报类型,不把 base64 倒进上下文。
    #[test]
    fn 非文本输出只报类型不倒base64() {
        let out = read_notebook(NB, None).unwrap();
        assert!(out.contains("<non-text output: image/png>"), "{out}");
        assert!(!out.contains("BASE64BLOB"), "base64 不得进上下文: {out}");
    }

    #[test]
    fn cells区间只渲染指定单元格() {
        let out = read_notebook(NB, Some("2-3")).unwrap();
        assert!(
            out.contains("[2] code") && out.contains("[3] code"),
            "{out}"
        );
        assert!(!out.contains("[1] markdown"), "区间外不该出现: {out}");
        assert!(!out.contains("[4]"), "区间外不该出现: {out}");
        let single = read_notebook(NB, Some("1")).unwrap();
        assert!(
            single.contains("[1] markdown") && !single.contains("[2]"),
            "{single}"
        );
    }

    /// 越界报错而不是静默夹取——静默夹取会让模型以为读到了它要的那一段。
    #[test]
    fn 区间非法或越界都报错() {
        assert!(parse_cell_range("0", 4).is_err(), "0 不是单元格");
        assert!(parse_cell_range("5-2", 4).is_err(), "倒置区间");
        assert!(parse_cell_range("9-12", 4).is_err(), "起点越界");
        assert!(parse_cell_range("abc", 4).is_err(), "非数字");
        // 终点越界向下夹到总数,这是安全的(起点仍在范围内)。
        assert_eq!(parse_cell_range("3-99", 4).unwrap(), (3, 4));
    }

    #[test]
    fn 坏json与缺cells都给出可行动错误() {
        let e = read_notebook("{not json", None).unwrap_err();
        assert!(e.contains("not valid JSON"), "{e}");
        let e = read_notebook(r#"{"metadata":{}}"#, None).unwrap_err();
        assert!(e.contains("no `cells`"), "{e}");
    }
}

/// R-326:PDF 单次最多给多少页。整本手册倒进上下文没有意义;超出就截断说明,
/// 让模型用 `pages` 缩范围(与 notebook 的 `cells` 同一套取舍)。
const MAX_PDF_PAGES: usize = 20;

/// R-326:按页渲染 PDF 文本。
///
/// 分页依据是**换页符**(U+000C):`pdftotext` 与 `pdf-extract` 都以它分隔页,
/// 这是两条抽取路径唯一的共同页边界。文件里没有换页符时整篇算一页——
/// 那说明抽取器没给出页信息,不能凭行数硬切,切出来的"页"是假的。
///
/// 全空页(扫描件没有文本层)必须**如实说明**并指路,不能回一段空文本:
/// 静默的空结果最坏,模型会据此断定"这几页是空白"。
fn render_pdf_pages(text: &str, spec: &str) -> Result<String, String> {
    let all: Vec<&str> = text.split('\u{c}').collect();
    let total = all.len();
    let (from, to) = parse_page_range(spec, total)?;
    let selected = &all[from - 1..to];
    if selected.iter().all(|page| page.trim().is_empty()) {
        return Err(format!(
            "pages {from}-{to} carry no extractable text ({total} pages total). \
             This reads the PDF text layer; a scanned/image-only PDF has none. \
             Render it to images or run an OCR step first."
        ));
    }
    let mut out = format!("pdf: {total} pages; showing {from}-{to}\n");
    for (offset, page) in selected.iter().enumerate() {
        let index = from + offset;
        out.push_str(&format!("\n[page {index}]\n"));
        let body = page.trim();
        if body.is_empty() {
            out.push_str("  (no text on this page)\n");
        } else {
            for line in body.lines() {
                out.push_str(&format!("  {line}\n"));
            }
        }
    }
    Ok(out)
}

/// 解析 "1-5" / "3" 形态的页区间,返回 1-based 闭区间。
///
/// 与 [`parse_cell_range`] 同形但**分开写**:两者的越界文案要各自点名"页"与
/// "单元格",共用一份就只能给出一种说法,而错的坐标系正是这里最容易犯的错。
fn parse_page_range(spec: &str, total: usize) -> Result<(usize, usize), String> {
    let spec = spec.trim();
    let (from, to) = match spec.split_once('-') {
        Some((a, b)) => (a.trim(), b.trim()),
        None => (spec, spec),
    };
    let parse = |s: &str| -> Result<usize, String> {
        s.parse::<usize>()
            .map_err(|_| format!("invalid page range `{spec}`; use forms like \"1-5\" or \"3\""))
    };
    let (from, to) = (parse(from)?, parse(to)?);
    if from == 0 || to == 0 {
        return Err("page numbers are 1-based; there is no page 0".into());
    }
    if from > to {
        return Err(format!("page range `{spec}` is inverted"));
    }
    if from > total {
        return Err(format!(
            "page range `{spec}` starts past the end; this PDF has {total} pages"
        ));
    }
    let capped = to.min(total).min(from + MAX_PDF_PAGES - 1);
    Ok((from, capped))
}

#[cfg(test)]
mod pdf_page_tests {
    use super::{parse_page_range, render_pdf_pages, MAX_PDF_PAGES};

    /// 分页依据是换页符——两条抽取路径(pdftotext / pdf-extract)唯一的共同页边界。
    const THREE_PAGES: &str = "first page\u{c}second page\u{c}third page";

    #[test]
    fn 按页渲染并标注页号() {
        let out = render_pdf_pages(THREE_PAGES, "2").unwrap();
        assert!(out.contains("pdf: 3 pages; showing 2-2"), "{out}");
        assert!(
            out.contains("[page 2]") && out.contains("second page"),
            "{out}"
        );
        assert!(!out.contains("first page"), "区间外不该出现: {out}");
    }

    #[test]
    fn 页区间闭合且不越界() {
        let out = render_pdf_pages(THREE_PAGES, "2-99").unwrap();
        assert!(out.contains("showing 2-3"), "终点越界应夹到总页数: {out}");
        assert!(out.contains("third page"), "{out}");
    }

    /// 没有换页符 = 抽取器没给页信息,整篇算一页;不能凭行数硬切假页。
    #[test]
    fn 无换页符时整篇算一页() {
        let out = render_pdf_pages("no form feeds here", "1").unwrap();
        assert!(out.contains("pdf: 1 pages"), "{out}");
        assert!(
            render_pdf_pages("no form feeds here", "2").is_err(),
            "第 2 页不存在"
        );
    }

    /// 扫描件没有文本层:必须如实说明并指路,不能回空文本让模型以为是空白页。
    #[test]
    fn 全空页报错并指向ocr而不是静默返回空() {
        let err = render_pdf_pages("\u{c}   \u{c}\t", "1-3").unwrap_err();
        assert!(err.contains("no extractable text"), "{err}");
        assert!(
            err.contains("OCR") || err.contains("images"),
            "必须给出下一步而不是只说失败: {err}"
        );
    }

    /// 越界/非法一律报错,不静默夹取——夹取会让模型以为读到了它要的那段。
    #[test]
    fn 页区间非法即报错且文案点名页而不是单元格() {
        assert!(parse_page_range("0", 3).is_err());
        assert!(parse_page_range("3-1", 3).is_err());
        assert!(parse_page_range("abc", 3).is_err());
        let err = parse_page_range("9-12", 3).unwrap_err();
        assert!(err.contains("page"), "文案必须点名 page: {err}");
        assert!(!err.contains("cell"), "不得串用单元格文案: {err}");
    }

    /// 单次页数封顶,防止一次 read 把整本手册倒进上下文。
    #[test]
    fn 单次页数封顶() {
        let many: String = (0..100)
            .map(|n| format!("page {n} body"))
            .collect::<Vec<_>>()
            .join("\u{c}");
        let (from, to) = parse_page_range("1-100", 100).unwrap();
        assert_eq!(from, 1);
        assert_eq!(to, MAX_PDF_PAGES, "一次最多给 {MAX_PDF_PAGES} 页");
        let out = render_pdf_pages(&many, "1-100").unwrap();
        assert!(out.contains(&format!("showing 1-{MAX_PDF_PAGES}")), "{out}");
    }
}
