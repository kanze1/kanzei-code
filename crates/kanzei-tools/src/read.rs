//! read 工具。设计红线 2:流式读取,永不整读文件。
//! - offset/limit:BufReader 逐行流式,跳过即丢弃;
//! - tail:从文件尾反向 64KiB 分块 seek,凑够行数即停。

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

const DEFAULT_LIMIT: usize = 2000;
const MAX_LINE_CHARS: usize = 500;
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const TAIL_CHUNK: u64 = 64 * 1024;

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
        "Read a text file. Params: path; optional offset (1-based line), limit (max lines), tail (last N lines).".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(ReadInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["path"].as_str().unwrap_or("*").to_string()]
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: ReadInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let path = ctx
            .cwd
            .join(kanzei_harness::permission::normalize_resource(&input.path));
        let result = tokio::task::spawn_blocking(move || read_sync(&path, &input)).await;
        match result {
            Ok(Ok(text)) => ToolOutput::ok(text),
            Ok(Err(e)) => ToolOutput::error(e),
            Err(e) => ToolOutput::error(format!("read task panicked: {e}")),
        }
    }
}

fn read_sync(path: &std::path::Path, input: &ReadInput) -> Result<String, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if meta.is_dir() {
        return Err(format!("{} is a directory", path.display()));
    }

    // 二进制探测:前 8KiB 含 NUL 即拒绝。
    let mut probe = [0u8; 8192];
    let n = file.read(&mut probe).map_err(|e| e.to_string())?;
    if probe[..n].contains(&0) {
        return Err(format!(
            "{} looks binary ({} bytes); read refuses binary files",
            path.display(),
            meta.len()
        ));
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;

    if let Some(tail) = input.tail {
        return read_tail(&mut file, meta.len(), tail);
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
