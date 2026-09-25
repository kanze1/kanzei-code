//! 工具执行域(R-155 B6):wave 构建与并行执行(PreparedToolCall/
//! build_tool_execution_waves_with/execute_prepared_tools)、权限拒绝占位
//! (append_declined_tool_results)、Gate 门禁与并发上限常量。
//! PreparedToolCall 六字段提 pub(super)(测试按字面量构造)。

use futures::StreamExt;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx};
use kanzei_llm::Part;
use sha2::Digest;
use std::path::Path;
use std::sync::Arc;

use super::tool_failure_telemetry::record_tool_failure;
use crate::runner::event::{preview, RunEvent};

/// 同一无冲突 wave 的普通工具并发上限；超过时按原调用顺序切 wave。
/// 测试锚点:生产调用方(execute_prepared_tools 参数)由 drive 层传值。
#[allow(dead_code)]
pub const MAX_PARALLEL_TOOLS_PER_WAVE: usize = 8;
pub(crate) fn append_declined_tool_results(
    results: &mut Vec<Part>,
    calls: &[(String, String, serde_json::Value, String)],
    declined_index: usize,
) {
    for (index, (id, _, _, _)) in calls.iter().enumerate().skip(declined_index) {
        let content = if index == declined_index {
            "permission request declined by user"
        } else {
            "tool call cancelled because a previous permission request was declined"
        };
        results.push(Part::ToolResult {
            call_id: id.clone(),
            content: content.into(),
            is_error: true,
        });
    }
}

pub(crate) struct PreparedToolCall {
    pub(super) index: usize,
    pub(super) id: String,
    pub(super) name: String,
    pub(super) input: serde_json::Value,
    pub(super) tool: Arc<dyn Tool>,
    pub(super) concurrency: ToolConcurrency,
}

/// D-661:按**冲突前驱层级**切波,而不是顺序扫描遇冲突即封波。
///
/// 旧实现只维护一个「当前波」,一旦封波就再也回不去:后面出现的、与任何在跑的调用
/// 都不冲突的工具,只能排到新波里等着。见证用例(D-661 复现):调用序 `[A, B, C, D]`,
/// 冲突关系 `A↔B`、`C↔D`,其余互不冲突——旧实现得到 `[[A], [B, C], [D]]` 三波,
/// 而两波 `[[A, C], [B, D]]` 就够,且不改变任何冲突对的先后。
///
/// # 不变式:冲突对的相对顺序必须保持
///
/// `ToolConcurrency::conflicts_with` 表达的是「不能同时跑」,而它蕴含「顺序有意义」——
/// 同一棵树上的两次 `WorktreeWrite` 是两次写,`Shared` 与 `WorktreeWrite` 是读与写,
/// 先后颠倒都会换掉结果。所以本函数**不做**任意重排:
///
/// > 对任意 `i < j`,若 `calls[i]` 与 `calls[j]` 冲突,则 `wave(i) < wave(j)`。
///
/// 这条不变式把「可达最优」限制成**层级调度**而非**装箱**:朴素 first-fit 会把 j
/// 塞进比其冲突前驱更早的波(等于把两次写颠倒过来),那不是提速,是换语义。
///
/// # 算法
///
/// 每个调用的最早可入波 = 所有**更早的冲突前驱**所在波 + 1(无前驱则 0);
/// 从该波起找第一个未满的波放入。因为任何与 j 冲突的更早调用 i 必然满足
/// `wave(i) + 1 <= earliest`,所以 `earliest` 及其之后的波里不可能存在与 j 冲突的
/// 更早调用——放进去无需再查一次冲突。容量不足时只会往**后**顺延,不会破坏不变式。
///
/// 复杂度 O(n²) 冲突比较,与旧实现同阶;n 是单步工具调用数(上限 8~几十),不是热点。
/// 确定性保持:同样的输入序列永远切出同样的波(顺序遍历 + 最早可用波)。
pub(crate) fn build_tool_execution_waves_with(
    max_parallel: usize,
    calls: Vec<PreparedToolCall>,
) -> Vec<Vec<PreparedToolCall>> {
    // 0 会让下面的容量循环永远找不到空位;调用方传的是常量 8,这里只做防御。
    let capacity = max_parallel.max(1);
    let concurrency: Vec<ToolConcurrency> =
        calls.iter().map(|call| call.concurrency.clone()).collect();
    let mut waves: Vec<Vec<PreparedToolCall>> = Vec::new();
    let mut placed: Vec<usize> = Vec::with_capacity(calls.len());
    for (index, call) in calls.into_iter().enumerate() {
        // 最早可入波:越过每一个更早的冲突前驱。
        let mut earliest = 0usize;
        for prior in 0..index {
            if concurrency[index].conflicts_with(&concurrency[prior]) {
                earliest = earliest.max(placed[prior] + 1);
            }
        }
        let mut slot = earliest;
        loop {
            if slot >= waves.len() {
                waves.push(Vec::new());
            }
            if waves[slot].len() < capacity {
                break;
            }
            slot += 1;
        }
        waves[slot].push(call);
        placed.push(slot);
    }
    waves
}

/// R-249:把 ToolOutput 的图片转成 llm Part;provider 不支持时降级为文本说明。
///
/// 返回 `(图片 Part, 需要追加到工具结果文本的说明)`。
///
/// 并行与串行两条执行路径都要做这层转换,必须共用一份——media_type 口径分叉的话,
/// 同一个工具在两条路径下会给 provider 发出不同的请求体,而这种差异只在其中一条
/// 路径上复现,极难定位。
///
/// **为什么降级要发生在这里,而不是靠 client.rs 那道硬拒绝**:那条会让整个请求
/// 失败,而图片一旦进了 messages 就跟着历史每轮重发——等于一次 read 图片就把这条
/// 对话在该 provider 上永久打死。所以图片必须在进历史**之前**被拦下,并如实告诉
/// 模型它没拿到图,好让它改走别的手段。静默丢弃是最坏的一种:模型会以为自己看过了。
pub(crate) fn tool_images_to_parts(
    output: &kanzei_harness::ToolOutput,
    images_supported: bool,
) -> (Vec<Part>, Option<String>) {
    if output.images.is_empty() {
        return (Vec::new(), None);
    }
    if !images_supported {
        return (
            Vec::new(),
            Some(format!(
                "\n[image not delivered: the active provider does not accept image input; \
                 {} image(s) were dropped. You did NOT see them — do not describe their contents.]",
                output.images.len()
            )),
        );
    }
    let parts = output
        .images
        .iter()
        .map(|image| Part::Image {
            media_type: image.media_type.clone(),
            data: image.data.clone(),
        })
        .collect();
    (parts, None)
}

const TOOL_RESULT_SPILL_THRESHOLD: usize = 1024 * 1024;
const TOOL_RESULT_SHADOW_THRESHOLD: usize = 32 * 1024;
const TOOL_RESULT_STORAGE_QUOTA_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// 配额锁的等待预算。拿不到锁只降级为 Inline 截断,不判工具失败。
const TOOL_RESULT_QUOTA_LOCK_BUDGET: std::time::Duration =
    kanzei_base::atomic_file::DEFAULT_LOCK_BUDGET;
/// 无法外置时 Inline 截断保留的头/尾字节数(与 R-376 外置预览同口径)。
const TOOL_RESULT_TRUNCATE_HEAD_BYTES: usize = 8 * 1024;
const TOOL_RESULT_TRUNCATE_TAIL_BYTES: usize = 4 * 1024;
/// 影子遥测子目录。它不是外置原文,不计入配额:计入会让配额扫描的基数随调用
/// 次数只增不减,而清理计划又看不到它(F1/#8、#30)。
const TOOL_RESULT_SHADOW_DIR: &str = "shadow";

fn lock_tool_result_storage(
    project_root: &Path,
    budget: std::time::Duration,
) -> std::io::Result<Option<kanzei_base::atomic_file::FileLock>> {
    let target = project_root.join(".kanzei/artifacts/tool-results-quota");
    let parent = target.parent().expect("quota lock has parent directory");
    std::fs::create_dir_all(parent)?;
    kanzei_base::atomic_file::try_lock_exclusive(&target, budget)
}

/// 统计 tool-results 下外置 artifact 的总字节数。`shadow` 子目录(影子遥测)
/// 不计入,口径与 store/session.rs 存储报告的 artifact_bytes 一致。
fn tool_result_storage_bytes(root: &Path) -> std::io::Result<u64> {
    fn visit(directory: &Path, total: &mut u64) -> std::io::Result<()> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "tool-results contains a symlink; quota cannot be measured safely",
                ));
            }
            if file_type.is_dir() {
                if entry.file_name() == TOOL_RESULT_SHADOW_DIR {
                    continue;
                }
                visit(&entry.path(), total)?;
            } else if file_type.is_file() {
                *total = total.saturating_add(entry.metadata()?.len());
            }
        }
        Ok(())
    }

    let metadata = match std::fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "tool-results is not a regular directory",
        ));
    }
    let mut total = 0;
    visit(root, &mut total)?;
    Ok(total)
}

fn record_tool_result_shadow_telemetry(
    ctx: &ToolCtx,
    tool_name: &str,
    bytes: usize,
    sha256: Option<&str>,
    actual_spilled: bool,
) {
    let safe_tool_name: String = tool_name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    let run_id = ctx
        .run_id
        .as_deref()
        .unwrap_or("unknown-run")
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let dir = ctx
        .project_root
        .join(".kanzei/artifacts/tool-results")
        .join(TOOL_RESULT_SHADOW_DIR);
    let path = dir.join(format!(
        "{safe_tool_name}-{run_id}-{}-{stamp}.json",
        std::process::id()
    ));
    let payload = serde_json::json!({
        "kind": "tool_result_shadow",
        "tool_name": tool_name,
        "bytes": bytes,
        "shadow_threshold": TOOL_RESULT_SHADOW_THRESHOLD,
        "would_spill": bytes > TOOL_RESULT_SHADOW_THRESHOLD,
        "actual_spilled": actual_spilled,
        "sha256": sha256,
    });
    if let Ok(encoded) = serde_json::to_string(&payload) {
        let _ = kanzei_base::atomic_file::write_atomic(&path, &encoded);
    }
}

/// 把超过 1 MiB 的工具结果写入 durable artifact,再把紧凑引用回喂给模型/UI。
///
/// 该函数位于所有工具的统一消费出口,工具权限与错误码不变。各分支实际保留的内容:
/// - ≤ 1 MiB:content 原样保留,只写一条影子遥测;不取配额锁、不扫描目录。
/// - 超过 1 MiB 且同名(同 sha256)、同长度的 artifact 已存在:直接复用,
///   不取锁、不计配额。
/// - 否则在跨进程配额锁内计量 tool-results(不含 shadow 子目录),写入后总占用
///   不超过 2 GiB 才写 artifact;content 换成外置引用 + 首行预览,原文可按
///   retrieval_hint 回读。
/// - 超过 2 GiB(artifact_quota_exceeded)、配额锁拿不到(quota_lock_unavailable)
///   或占用无法计量(quota_unmeasurable)时降级为 Inline 截断:首行
///   `[tool_result_truncated reason=.. ..]` 标记,正文只保留头 8 KiB + 尾 4 KiB
///   (按字符边界切),中间一行写明省略字节数;不创建 artifact,**原文不可回取**。
///   工具自己的 is_error/outcome/code 不变;display 为对象时原样保留并追加
///   `quota_truncated`,为空时才写 `{"kind":"truncated",..}`。
/// - 只有 artifact 路径被非普通文件占用或写入失败时,结果转为
///   TOOL_RESULT_SPILL_FAILED,避免事件看起来像一次成功的外置。
pub(crate) fn materialize_tool_output(
    output: &mut kanzei_harness::ToolOutput,
    ctx: &ToolCtx,
    tool_name: &str,
) {
    materialize_tool_output_with_quota(
        output,
        ctx,
        tool_name,
        TOOL_RESULT_STORAGE_QUOTA_BYTES,
        TOOL_RESULT_QUOTA_LOCK_BUDGET,
    );
}

/// `materialize_tool_output` 的可注入版本:配额与锁等待预算由调用方给(测试用),
/// 各分支保留的内容与上面的说明完全一致。
fn materialize_tool_output_with_quota(
    output: &mut kanzei_harness::ToolOutput,
    ctx: &ToolCtx,
    tool_name: &str,
    quota_bytes: u64,
    lock_budget: std::time::Duration,
) {
    // F1/#8:小结果只写遥测,不取锁、不扫描(遥测存废归 R-376)。
    if output.content.len() <= TOOL_RESULT_SPILL_THRESHOLD {
        record_tool_result_shadow_telemetry(ctx, tool_name, output.content.len(), None, false);
        return;
    }

    let original = std::mem::take(&mut output.content);
    let mut digest = sha2::Sha256::new();
    sha2::Digest::update(&mut digest, original.as_bytes());
    let sha256 = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let safe_tool_name: String = tool_name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    let artifact_id = format!("tool-{safe_tool_name}-{sha256}");
    let relative_path = format!(".kanzei/artifacts/tool-results/{artifact_id}.txt");
    let path = ctx.project_root.join(&relative_path);

    match store_tool_result_artifact(
        &ctx.project_root,
        &path,
        &original,
        quota_bytes,
        lock_budget,
        tool_name,
    ) {
        SpillDecision::Stored => {
            record_tool_result_shadow_telemetry(
                ctx,
                tool_name,
                original.len(),
                Some(&sha256),
                true,
            );
            externalize_tool_output(output, &original, artifact_id, relative_path, sha256);
        }
        SpillDecision::Truncate {
            reason,
            storage_used_bytes,
        } => {
            record_tool_result_shadow_telemetry(
                ctx,
                tool_name,
                original.len(),
                Some(&sha256),
                false,
            );
            truncate_tool_output(
                output,
                &original,
                &sha256,
                reason,
                storage_used_bytes,
                quota_bytes,
            );
        }
        SpillDecision::Failed(reason) => {
            record_tool_result_shadow_telemetry(
                ctx,
                tool_name,
                original.len(),
                Some(&sha256),
                false,
            );
            fail_tool_result_spill(output, original.len(), &sha256, &reason);
        }
    }
}

/// 外置存储的决定。
enum SpillDecision {
    /// artifact 已完整在盘上(新写入,或复用同内容的既有文件)。
    Stored,
    /// 无法外置,降级为 Inline 截断;`storage_used_bytes` 仅在计量成功时有值。
    Truncate {
        reason: TruncateReason,
        storage_used_bytes: Option<u64>,
    },
    /// artifact 路径被非普通文件占用,或写入失败。
    Failed(String),
}

/// 在配额约束下把原文落到 `path`,只做存储决定,不改 ToolOutput。
fn store_tool_result_artifact(
    project_root: &Path,
    path: &Path,
    original: &str,
    quota_bytes: u64,
    lock_budget: std::time::Duration,
    tool_name: &str,
) -> SpillDecision {
    let original_bytes = original.len() as u64;
    // F1/#13:文件名即内容哈希,已完整存在就先于取锁与配额判断直接复用(不新增字节)。
    // 这里的错误与非普通文件留到锁内再定性。
    if let Ok(StoredArtifact::Complete) = inspect_stored_artifact(path, original_bytes) {
        return SpillDecision::Stored;
    }

    let _quota_guard = match lock_tool_result_storage(project_root, lock_budget) {
        Ok(Some(guard)) => guard,
        other => {
            // F1/#10:锁错误文本("稍后重试/删锁文件")只进日志,不回喂模型。
            let error = other
                .err()
                .map_or_else(|| "等待超时".to_string(), |error| error.to_string());
            tracing::warn!(tool = tool_name, %error, "工具结果配额锁不可用,结果降级为 Inline 截断");
            return SpillDecision::Truncate {
                reason: TruncateReason::LockUnavailable,
                storage_used_bytes: None,
            };
        }
    };
    let existing_bytes = match inspect_stored_artifact(path, original_bytes) {
        // 等锁期间别的进程可能已写入同一内容。
        Ok(StoredArtifact::Complete) => return SpillDecision::Stored,
        Ok(StoredArtifact::Missing) => 0,
        Ok(StoredArtifact::Mismatched(bytes)) => bytes,
        Ok(StoredArtifact::NotRegular) => {
            return SpillDecision::Failed("artifact path is not a regular file".into());
        }
        Err(error) => {
            tracing::warn!(tool = tool_name, %error, "无法读取 artifact 元数据,结果降级为 Inline 截断");
            return SpillDecision::Truncate {
                reason: TruncateReason::Unmeasurable,
                storage_used_bytes: None,
            };
        }
    };
    let artifact_root = project_root.join(".kanzei/artifacts/tool-results");
    let used_bytes = match tool_result_storage_bytes(&artifact_root) {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(tool = tool_name, %error, "无法计量工具结果存储占用,结果降级为 Inline 截断");
            return SpillDecision::Truncate {
                reason: TruncateReason::Unmeasurable,
                storage_used_bytes: None,
            };
        }
    };
    let projected_bytes = used_bytes
        .saturating_sub(existing_bytes)
        .saturating_add(original_bytes);
    if projected_bytes > quota_bytes {
        return SpillDecision::Truncate {
            reason: TruncateReason::QuotaExceeded,
            storage_used_bytes: Some(used_bytes),
        };
    }
    match kanzei_base::atomic_file::write_atomic(path, original) {
        Ok(()) => SpillDecision::Stored,
        Err(error) => SpillDecision::Failed(error.to_string()),
    }
}

/// artifact 路径的现状。文件名含 sha256(内容寻址),同名且长度一致即视为已存储,
/// 不整读比对(F1/#13)。
enum StoredArtifact {
    Missing,
    Complete,
    /// 同名但长度不符(写入中断等),需要覆盖;带既有字节数供配额扣减。
    Mismatched(u64),
    NotRegular,
}

fn inspect_stored_artifact(path: &Path, expected_bytes: u64) -> std::io::Result<StoredArtifact> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            if metadata.len() == expected_bytes {
                Ok(StoredArtifact::Complete)
            } else {
                Ok(StoredArtifact::Mismatched(metadata.len()))
            }
        }
        Ok(_) => Ok(StoredArtifact::NotRegular),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(StoredArtifact::Missing),
        Err(error) => Err(error),
    }
}

fn externalize_tool_output(
    output: &mut kanzei_harness::ToolOutput,
    original: &str,
    artifact_id: String,
    relative_path: String,
    sha256: String,
) {
    let artifact = kanzei_harness::ToolArtifact {
        retrieval_hint: format!("read path={relative_path}"),
        artifact_id,
        relative_path,
        bytes: original.len() as u64,
        sha256,
    };
    output.content = format!(
        "[tool_result_externalized artifact_id={} bytes={} sha256={}]\nPreview: {}\n完整原文已外置；请按 retrieval_hint 回读。",
        artifact.artifact_id,
        original.len(),
        artifact.sha256,
        preview(original),
    );
    output.display = Some(serde_json::json!({
        "kind": "artifact",
        "artifact_id": artifact.artifact_id,
        "bytes": artifact.bytes,
        "sha256": artifact.sha256,
        "retrieval_hint": artifact.retrieval_hint,
    }));
    output.artifact = Some(artifact);
}

/// 无法外置时的降级原因(写入 marker 与 display)。
#[derive(Clone, Copy)]
enum TruncateReason {
    QuotaExceeded,
    LockUnavailable,
    Unmeasurable,
}

impl TruncateReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::QuotaExceeded => "artifact_quota_exceeded",
            Self::LockUnavailable => "quota_lock_unavailable",
            Self::Unmeasurable => "quota_unmeasurable",
        }
    }

    fn explanation(self) -> &'static str {
        match self {
            Self::QuotaExceeded => {
                "结果因工具结果存储达到配额而截断；未创建 artifact。清理无引用 artifact 后才会恢复外置。"
            }
            Self::LockUnavailable => "结果因暂时拿不到工具结果存储锁而截断；未创建 artifact。",
            Self::Unmeasurable => "结果因无法计量工具结果存储占用而截断；未创建 artifact。",
        }
    }
}

/// 头 8 KiB + 尾 4 KiB 的有界正文,切点落在字符边界;中间一行写明省略字节数、
/// 原文不可回取,并建议用更窄的输出重跑。原文不超过头尾之和时原样返回。
fn inline_truncate(original: &str) -> String {
    let len = original.len();
    if len <= TOOL_RESULT_TRUNCATE_HEAD_BYTES + TOOL_RESULT_TRUNCATE_TAIL_BYTES {
        return original.to_string();
    }
    let mut head_end = TOOL_RESULT_TRUNCATE_HEAD_BYTES;
    while !original.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = len - TOOL_RESULT_TRUNCATE_TAIL_BYTES;
    while !original.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let omitted = tail_start - head_end;
    format!(
        "{}\n…(中间省略 {omitted} 字节;原文未保存,无法回取。需要这部分时请用更窄的输出重跑,如 | tail、grep 或 offset/limit)…\n{}",
        &original[..head_end],
        &original[tail_start..],
    )
}

/// F1(#9/#10/#12):无法外置时的 Inline 截断降级。只改 content、补 display 标注、
/// 清 artifact;is_error/outcome/code 保持工具自己的结果——截断的是回喂量,
/// 不是工具的执行结果。
fn truncate_tool_output(
    output: &mut kanzei_harness::ToolOutput,
    original: &str,
    sha256: &str,
    reason: TruncateReason,
    storage_used_bytes: Option<u64>,
    quota_bytes: u64,
) {
    let used_text =
        storage_used_bytes.map_or_else(|| "unknown".to_string(), |bytes| bytes.to_string());
    output.content = format!(
        "[tool_result_truncated reason={} bytes={} storage_used_bytes={used_text} quota_bytes={quota_bytes} sha256={sha256}]\n{}仅保留头 {} KiB 与尾 {} KiB。\n{}",
        reason.as_str(),
        original.len(),
        reason.explanation(),
        TOOL_RESULT_TRUNCATE_HEAD_BYTES / 1024,
        TOOL_RESULT_TRUNCATE_TAIL_BYTES / 1024,
        inline_truncate(original),
    );
    let annotation = serde_json::json!({
        "reason": reason.as_str(),
        "storage_used_bytes": storage_used_bytes,
        "quota_bytes": quota_bytes,
    });
    match output.display.as_mut() {
        Some(serde_json::Value::Object(display)) => {
            display.insert("quota_truncated".into(), annotation);
        }
        // 非对象 display 无处挂标注,原样保留。
        Some(_) => {}
        None => {
            output.display = Some(serde_json::json!({
                "kind": "truncated",
                "reason": reason.as_str(),
                "bytes": original.len(),
                "storage_used_bytes": storage_used_bytes,
                "quota_bytes": quota_bytes,
                "sha256": sha256,
                "preview": preview(original),
            }));
        }
    }
    output.artifact = None;
}

fn fail_tool_result_spill(
    output: &mut kanzei_harness::ToolOutput,
    bytes: usize,
    sha256: &str,
    reason: &str,
) {
    output.content = format!("[tool_result_spill_failed bytes={bytes} sha256={sha256}]: {reason}");
    output.is_error = true;
    output.outcome = kanzei_harness::ToolOutcome::Failed;
    output.code = Some("TOOL_RESULT_SPILL_FAILED");
    output.artifact = None;
}

/// 返回 (下标, ToolResult, 该结果附带的图片 Part)。
///
/// 图片**不能**混进 results 向量:那里 `results[i] ↔ calls[i]` 是硬约定
/// (note_step 的 debug_assert 锁着)。它们由调用方单独收集,统一追加到
/// tool_results 消息尾部——Anthropic 要求 tool_result 块排在 user 消息最前,
/// 所以只能后缀不能前插。
pub(crate) async fn execute_prepared_tools(
    calls: Vec<PreparedToolCall>,
    ctx: &ToolCtx,
    max_parallel: usize,
    images_supported: bool,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> Vec<(usize, Part, Vec<Part>)> {
    let mut results = Vec::new();
    // 进度旁路:每个调用带自己 id 的 ProgressHandle,共用一条通道;
    // 收集循环里边等完成边转发增量输出,UI 才能在长任务执行中看到"跑到哪了"。
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<kanzei_harness::progress::ProgressChunk>();
    for wave in build_tool_execution_waves_with(max_parallel, calls) {
        let mut jobs: futures::stream::FuturesUnordered<_> = wave
            .into_iter()
            .map(|call| {
                let progress_tx = progress_tx.clone();
                async move {
                    let PreparedToolCall {
                        index,
                        id,
                        name,
                        input,
                        tool,
                        concurrency: _,
                    } = call;
                    // D-174:专用文档工具的执行区间就是它的合法写入窗口,后台守卫
                    // 据此把"专用工具改的"和"后台进程偷改的"分开。非写工具零开销。
                    // R-259:执行包装(wrap_execute:progress 注入)收编——串行/并行
                    // 共用同一 wrapper,工具 body 不再各自实现 progress 注入。
                    let output = kanzei_harness::managed_fence::tool_scope(
                        &name,
                        kanzei_harness::tool_pipeline::wrap_execute(
                            id.clone(),
                            Some(progress_tx.clone()),
                            None,
                            tool.execute(input, ctx),
                        ),
                    )
                    .await;
                    (index, id, name, output)
                }
            })
            .collect();
        loop {
            tokio::select! {
                // 先清进度再收终态:同一调用的增量输出必须排在它的 ToolEnd 之前。
                biased;
                Some((id, chunk)) = progress_rx.recv() => {
                    on_event(RunEvent::ToolProgress { id, chunk });
                }
                job = jobs.next() => {
                    let Some((index, id, name, mut output)) = job else { break };
                    materialize_tool_output(&mut output, ctx, &name);
                    record_tool_failure(ctx, &id, &name, &output);
                    while let Ok((pid, chunk)) = progress_rx.try_recv() {
                        on_event(RunEvent::ToolProgress { id: pid, chunk });
                    }
                    on_event(RunEvent::tool_end(id.clone(), name, &output));
                    let mut model_content = output.model_content();
                    let (images, dropped_note) =
                        tool_images_to_parts(&output, images_supported);
                    if let Some(note) = dropped_note {
                        model_content.push_str(&note);
                    }
                    results.push((
                        index,
                        Part::ToolResult {
                            call_id: id,
                            content: model_content,
                            is_error: output.is_error,
                        },
                        images,
                    ));
                }
            }
        }
    }
    results.sort_by_key(|(index, _, _)| *index);
    results
}

pub(crate) enum Gate {
    Pass,
    Deny(String),
    NonInteractive(String),
    UserDeclined,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
    use kanzei_llm::Part;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct ProbeTool {
        name: &'static str,
        concurrency: ToolConcurrency,
        in_flight: Arc<AtomicUsize>,
        max_in_flight: Arc<AtomicUsize>,
    }

    #[async_trait]

    impl Tool for ProbeTool {
        fn name(&self) -> &'static str {
            self.name
        }

        fn description(&self) -> String {
            "test probe".into()
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        fn concurrency(&self, _input: &serde_json::Value, _ctx: &ToolCtx) -> ToolConcurrency {
            self.concurrency.clone()
        }

        async fn execute(&self, input: serde_json::Value, _ctx: &ToolCtx) -> ToolOutput {
            let active = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(
                input["delay_ms"].as_u64().unwrap_or(10),
            ))
            .await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            let label = input["label"].as_str().unwrap_or("probe");
            if input["fail"].as_bool().unwrap_or(false) {
                ToolOutput::error(format!("{label} failed"))
            } else {
                ToolOutput::ok(format!("{label} ok"))
            }
        }
    }

    fn probe_call(
        index: usize,
        id: &str,
        input: serde_json::Value,
        tool: Arc<ProbeTool>,
    ) -> PreparedToolCall {
        PreparedToolCall {
            index,
            id: id.into(),
            name: tool.name().into(),
            concurrency: tool.concurrency(
                &input,
                &ToolCtx::new(std::env::temp_dir(), std::env::temp_dir()),
            ),
            input,
            tool,
        }
    }

    #[tokio::test]
    async fn 普通只读工具真实并发_失败隔离且结果按调用顺序归位() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let tool = Arc::new(ProbeTool {
            name: "probe_read",
            concurrency: ToolConcurrency::Shared("worktree:test".into()),
            in_flight: in_flight.clone(),
            max_in_flight: max_in_flight.clone(),
        });
        let calls = vec![
            probe_call(
                0,
                "call_slow",
                serde_json::json!({"label": "slow", "delay_ms": 60}),
                tool.clone(),
            ),
            probe_call(
                1,
                "call_fast_fail",
                serde_json::json!({"label": "fast", "delay_ms": 5, "fail": true}),
                tool,
            ),
        ];
        let ctx = ToolCtx::new(std::env::temp_dir(), std::env::temp_dir());
        let mut completed = Vec::new();
        let mut contents = Vec::new();
        let mut on_event = |event| {
            if let RunEvent::ToolEnd {
                id,
                content,
                content_bytes,
                ..
            } = event
            {
                completed.push(id);
                contents.push((content, content_bytes));
            }
        };
        let results = execute_prepared_tools(
            calls,
            &ctx,
            super::MAX_PARALLEL_TOOLS_PER_WAVE,
            true,
            &mut on_event,
        )
        .await;

        assert!(
            max_in_flight.load(Ordering::SeqCst) >= 2,
            "只读调用没有重叠执行"
        );
        assert_eq!(completed, vec!["call_fast_fail", "call_slow"]);
        // UI-0926 #6:ToolEnd 带与历史同源的正文(不含 outcome 机器头)。
        assert_eq!(
            contents,
            vec![("fast failed".to_string(), 11), ("slow ok".to_string(), 7)]
        );
        assert!(matches!(
            &results[0].1,
            Part::ToolResult { call_id, is_error: false, content } if call_id == "call_slow" && content.contains("slow ok")
        ));
        assert!(matches!(
            &results[1].1,
            Part::ToolResult { call_id, is_error: true, content } if call_id == "call_fast_fail" && content.contains("fast failed")
        ));
    }

    #[tokio::test]
    async fn 同一工作树读写与写写冲突严格串行() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let writer = Arc::new(ProbeTool {
            name: "probe_write",
            concurrency: ToolConcurrency::WorktreeWrite("worktree:test".into()),
            in_flight: in_flight.clone(),
            max_in_flight: max_in_flight.clone(),
        });
        let reader = Arc::new(ProbeTool {
            name: "probe_read",
            concurrency: ToolConcurrency::Shared("worktree:test".into()),
            in_flight,
            max_in_flight: max_in_flight.clone(),
        });
        let calls = vec![
            probe_call(
                0,
                "write_1",
                serde_json::json!({"delay_ms": 15}),
                writer.clone(),
            ),
            probe_call(1, "read_1", serde_json::json!({"delay_ms": 15}), reader),
            probe_call(2, "write_2", serde_json::json!({"delay_ms": 15}), writer),
        ];
        let ctx = ToolCtx::new(std::env::temp_dir(), std::env::temp_dir());
        let mut on_event = |_event| {};
        let results = execute_prepared_tools(
            calls,
            &ctx,
            super::MAX_PARALLEL_TOOLS_PER_WAVE,
            true,
            &mut on_event,
        )
        .await;

        assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);
        assert_eq!(
            results
                .iter()
                .map(|(_, part, _)| match part {
                    Part::ToolResult { call_id, .. } => call_id.as_str(),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>(),
            vec!["write_1", "read_1", "write_2"]
        );
    }

    fn quota_test_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-r245-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    /// 首尾可区分、含多字节字符的超阈值原文:头尾切点都落在中文字符中间。
    fn head_tail_fixture() -> String {
        let mut original = String::from("HEAD-MARK");
        while original.len() <= super::TOOL_RESULT_SPILL_THRESHOLD {
            original.push_str("中文填充");
        }
        original.push_str("TAIL-MARK");
        original
    }

    fn sha256_hex(text: &str) -> String {
        sha2::Sha256::digest(text.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn has_spilled_txt(artifact_root: &std::path::Path) -> bool {
        std::fs::read_dir(artifact_root)
            .map(|entries| {
                entries.filter_map(Result::ok).any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "txt")
                })
            })
            .unwrap_or(false)
    }

    /// 在另一个线程持有配额锁(FileLock 是 !Send,只能在持有线程上获取与释放)。
    /// 向返回的 Sender 发一次消息即释放。
    fn hold_quota_lock(
        root: &std::path::Path,
    ) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let holder_root = root.to_path_buf();
        let holder = std::thread::spawn(move || {
            let guard =
                super::lock_tool_result_storage(&holder_root, super::TOOL_RESULT_QUOTA_LOCK_BUDGET)
                    .unwrap()
                    .expect("持锁线程应拿到配额锁");
            locked_tx.send(()).unwrap();
            let _ = release_rx.recv();
            drop(guard);
        });
        locked_rx.recv().unwrap();
        (release_tx, holder)
    }

    const SHORT_LOCK_BUDGET: std::time::Duration = std::time::Duration::from_millis(50);

    #[test]
    fn tool_result_quota_is_two_gib_and_excludes_shadow_telemetry() {
        assert_eq!(
            super::TOOL_RESULT_STORAGE_QUOTA_BYTES,
            2 * 1024 * 1024 * 1024
        );
        let root = quota_test_root("quota-scan");
        let artifact_root = root.join(".kanzei/artifacts/tool-results");
        std::fs::create_dir_all(artifact_root.join("shadow/nested")).unwrap();
        std::fs::create_dir_all(artifact_root.join("nested")).unwrap();
        std::fs::write(artifact_root.join("artifact.bin"), b"12345").unwrap();
        std::fs::write(artifact_root.join("nested/other.bin"), b"678").unwrap();
        std::fs::write(artifact_root.join("shadow/top.json"), b"xx").unwrap();
        std::fs::write(artifact_root.join("shadow/nested/telemetry.json"), b"6789").unwrap();

        // F1/#8:影子遥测不计入配额,扫描基数只剩真实外置 artifact。
        assert_eq!(super::tool_result_storage_bytes(&artifact_root).unwrap(), 8);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tool_result_over_quota_is_truncated_without_spill_artifact() {
        let root = quota_test_root("quota-truncate");
        let artifact_root = root.join(".kanzei/artifacts/tool-results");
        std::fs::create_dir_all(&artifact_root).unwrap();
        std::fs::write(artifact_root.join("existing.data"), b"seed").unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let original = head_tail_fixture();
        let quota = 4 + original.len() as u64 - 1;
        let mut output = ToolOutput::ok(original.clone());

        super::materialize_tool_output_with_quota(
            &mut output,
            &ctx,
            "bash",
            quota,
            super::TOOL_RESULT_QUOTA_LOCK_BUDGET,
        );

        assert!(!output.is_error);
        assert_eq!(output.outcome, kanzei_harness::ToolOutcome::Success);
        assert_eq!(output.code, None);
        let first_line = output.content.lines().next().unwrap();
        assert!(
            first_line.starts_with("[tool_result_truncated reason=artifact_quota_exceeded bytes=")
        );
        assert!(first_line.contains("storage_used_bytes=4 "));
        assert!(first_line.contains(&format!("quota_bytes={quota} ")));
        // F1/#9:头 8 KiB + 尾 4 KiB,切点按字符边界回退/前进。
        let head_end = (0..=super::TOOL_RESULT_TRUNCATE_HEAD_BYTES)
            .rev()
            .find(|&index| original.is_char_boundary(index))
            .unwrap();
        let tail_start = (original.len() - super::TOOL_RESULT_TRUNCATE_TAIL_BYTES..=original.len())
            .find(|&index| original.is_char_boundary(index))
            .unwrap();
        assert_ne!(
            head_end,
            super::TOOL_RESULT_TRUNCATE_HEAD_BYTES,
            "夹具应让头部切点落在字符中间"
        );
        assert_ne!(
            tail_start,
            original.len() - super::TOOL_RESULT_TRUNCATE_TAIL_BYTES,
            "夹具应让尾部切点落在字符中间"
        );
        assert!(output.content.contains(&original[..head_end]));
        assert!(output.content.ends_with(&original[tail_start..]));
        assert!(output.content.contains("HEAD-MARK"));
        assert!(output.content.contains("TAIL-MARK"));
        assert!(output
            .content
            .contains(&format!("中间省略 {} 字节", tail_start - head_end)));
        assert!(output.content.contains("无法回取"));
        assert!(
            output.content.len()
                <= super::TOOL_RESULT_TRUNCATE_HEAD_BYTES
                    + super::TOOL_RESULT_TRUNCATE_TAIL_BYTES
                    + 1024
        );
        assert!(output.artifact.is_none());
        let display = output.display.as_ref().unwrap();
        assert_eq!(display["kind"], "truncated");
        assert_eq!(display["reason"], "artifact_quota_exceeded");
        assert_eq!(display["storage_used_bytes"], 4);
        assert_eq!(display["quota_bytes"], quota);
        assert!(display["preview"]
            .as_str()
            .unwrap()
            .starts_with("HEAD-MARK"));
        assert!(!has_spilled_txt(&artifact_root));
        assert!(super::tool_result_storage_bytes(&artifact_root).unwrap() <= quota);
        assert_eq!(super::inline_truncate("短结果"), "短结果");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn over_quota_truncation_keeps_tool_display_and_outcome() {
        let root = quota_test_root("quota-display");
        let artifact_root = root.join(".kanzei/artifacts/tool-results");
        std::fs::create_dir_all(&artifact_root).unwrap();
        std::fs::write(artifact_root.join("existing.data"), b"seed").unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let original = head_tail_fixture();
        let quota = 4 + original.len() as u64 - 1;
        let mut output =
            ToolOutput::failed("PROBE_EXIT_NONZERO", original).with_display(serde_json::json!({
                "kind": "terminal",
                "command": "cargo test",
                "exitCode": 1,
                "full": "terminal full output",
            }));

        super::materialize_tool_output_with_quota(
            &mut output,
            &ctx,
            "bash",
            quota,
            super::TOOL_RESULT_QUOTA_LOCK_BUDGET,
        );

        // F1/#12:工具自己的终态与 terminal display 原样保留,只追加配额标注。
        assert!(output.is_error);
        assert_eq!(output.outcome, kanzei_harness::ToolOutcome::Failed);
        assert_eq!(output.code, Some("PROBE_EXIT_NONZERO"));
        let display = output.display.as_ref().unwrap();
        assert_eq!(display["kind"], "terminal");
        assert_eq!(display["command"], "cargo test");
        assert_eq!(display["exitCode"], 1);
        assert_eq!(display["full"], "terminal full output");
        assert_eq!(
            display["quota_truncated"],
            serde_json::json!({
                "reason": "artifact_quota_exceeded",
                "storage_used_bytes": 4,
                "quota_bytes": quota,
            })
        );
        assert!(output
            .content
            .starts_with("[tool_result_truncated reason=artifact_quota_exceeded"));
        assert!(output.artifact.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn quota_lock_unavailable_degrades_to_truncation_without_failing_tool() {
        let root = quota_test_root("quota-lock");
        let artifact_root = root.join(".kanzei/artifacts/tool-results");
        std::fs::create_dir_all(&artifact_root).unwrap();
        let (release, holder) = hold_quota_lock(&root);
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let mut output = ToolOutput::ok(head_tail_fixture());

        super::materialize_tool_output_with_quota(
            &mut output,
            &ctx,
            "bash",
            super::TOOL_RESULT_STORAGE_QUOTA_BYTES,
            SHORT_LOCK_BUDGET,
        );
        release.send(()).unwrap();
        holder.join().unwrap();

        // F1/#10:拿不到锁只降级截断,不把已成功的工具判为失败。
        assert!(!output.is_error);
        assert_eq!(output.outcome, kanzei_harness::ToolOutcome::Success);
        assert_eq!(output.code, None);
        assert!(output
            .content
            .starts_with("[tool_result_truncated reason=quota_lock_unavailable"));
        assert!(output.content.contains("storage_used_bytes=unknown"));
        assert!(output.content.contains("HEAD-MARK"));
        assert!(output.content.contains("TAIL-MARK"));
        assert!(
            !output.content.contains("稍后重试"),
            "锁错误文本不应回喂模型"
        );
        assert!(output.artifact.is_none());
        let display = output.display.as_ref().unwrap();
        assert_eq!(display["kind"], "truncated");
        assert_eq!(display["reason"], "quota_lock_unavailable");
        assert!(display["storage_used_bytes"].is_null());
        assert!(!has_spilled_txt(&artifact_root));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn small_tool_result_never_waits_for_quota_lock() {
        let root = quota_test_root("quota-small");
        let (release, holder) = hold_quota_lock(&root);
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let mut output = ToolOutput::ok("small result");

        // 生产入口:锁预算 3 s;小结果若仍取锁,这里至少要等满预算。
        let started = std::time::Instant::now();
        super::materialize_tool_output(&mut output, &ctx, "bash");
        let elapsed = started.elapsed();
        release.send(()).unwrap();
        holder.join().unwrap();

        assert!(
            elapsed < std::time::Duration::from_millis(1500),
            "小结果不应取配额锁: {elapsed:?}"
        );
        assert_eq!(output.content, "small result");
        assert!(output.artifact.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unmeasurable_storage_degrades_to_truncation_without_failing_tool() {
        let root = quota_test_root("quota-unmeasurable");
        std::fs::create_dir_all(root.join(".kanzei/artifacts")).unwrap();
        std::fs::write(
            root.join(".kanzei/artifacts/tool-results"),
            "not a directory",
        )
        .unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let mut output = ToolOutput::ok(head_tail_fixture());

        super::materialize_tool_output(&mut output, &ctx, "bash");

        assert!(!output.is_error);
        assert_eq!(output.outcome, kanzei_harness::ToolOutcome::Success);
        assert_eq!(output.code, None);
        assert!(output
            .content
            .starts_with("[tool_result_truncated reason=quota_unmeasurable"));
        assert!(output.content.contains("TAIL-MARK"));
        assert!(output.artifact.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn identical_artifact_is_reused_before_quota_and_lock() {
        let root = quota_test_root("quota-reuse");
        let artifact_root = root.join(".kanzei/artifacts/tool-results");
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let original = head_tail_fixture();
        let mut first = ToolOutput::ok(original.clone());
        super::materialize_tool_output(&mut first, &ctx, "git");
        let artifact = first.artifact.clone().expect("首次应正常外置");
        let used = super::tool_result_storage_bytes(&artifact_root).unwrap();

        // F1/#13:配额已低于占用、配额锁也被别人持有,同内容 artifact 仍直接复用。
        let (release, holder) = hold_quota_lock(&root);
        let mut second = ToolOutput::ok(original.clone());
        super::materialize_tool_output_with_quota(
            &mut second,
            &ctx,
            "git",
            used - 1,
            SHORT_LOCK_BUDGET,
        );
        release.send(()).unwrap();
        holder.join().unwrap();

        assert_eq!(second.artifact.as_ref(), Some(&artifact));
        assert!(!second.is_error);
        assert!(second.content.starts_with("[tool_result_externalized"));
        assert_eq!(
            super::tool_result_storage_bytes(&artifact_root).unwrap(),
            used
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn same_name_artifact_with_wrong_length_is_rewritten() {
        let root = quota_test_root("quota-rewrite");
        let artifact_root = root.join(".kanzei/artifacts/tool-results");
        std::fs::create_dir_all(&artifact_root).unwrap();
        let original = head_tail_fixture();
        let path = artifact_root.join(format!("tool-git-{}.txt", sha256_hex(&original)));
        std::fs::write(&path, &original.as_bytes()[..100]).unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let mut output = ToolOutput::ok(original.clone());

        super::materialize_tool_output(&mut output, &ctx, "git");

        assert!(output.artifact.is_some());
        assert_eq!(std::fs::read(&path).unwrap(), original.as_bytes());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn same_name_artifact_rewrite_deducts_existing_bytes_from_quota() {
        let root = quota_test_root("quota-rewrite-boundary");
        let artifact_root = root.join(".kanzei/artifacts/tool-results");
        std::fs::create_dir_all(&artifact_root).unwrap();
        std::fs::write(artifact_root.join("existing.data"), b"seed").unwrap();
        let original = head_tail_fixture();
        let path = artifact_root.join(format!("tool-git-{}.txt", sha256_hex(&original)));
        std::fs::write(&path, &original.as_bytes()[..100]).unwrap();
        let used = super::tool_result_storage_bytes(&artifact_root).unwrap();
        assert_eq!(used, 4 + 100);
        // 恰好容纳「重写后」的占用:既有的 100 字节会被覆盖,必须先扣掉再算;
        // 不扣除时 used + original 超出配额,会被误判为超配额而截断。
        let quota = used - 100 + original.len() as u64;
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let mut output = ToolOutput::ok(original.clone());

        super::materialize_tool_output_with_quota(
            &mut output,
            &ctx,
            "git",
            quota,
            super::TOOL_RESULT_QUOTA_LOCK_BUDGET,
        );

        assert!(
            output.artifact.is_some(),
            "扣除既有字节后恰好容纳,应重写外置: {}",
            output.content.lines().next().unwrap_or_default()
        );
        assert_eq!(std::fs::read(&path).unwrap(), original.as_bytes());
        assert_eq!(
            super::tool_result_storage_bytes(&artifact_root).unwrap(),
            quota
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tool_result_at_quota_boundary_can_still_be_externalized() {
        let root = quota_test_root("quota-boundary");
        let artifact_root = root.join(".kanzei/artifacts/tool-results");
        std::fs::create_dir_all(&artifact_root).unwrap();
        std::fs::write(artifact_root.join("existing.data"), b"seed").unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let original = "x".repeat(super::TOOL_RESULT_SPILL_THRESHOLD + 17);
        let quota = 4 + original.len() as u64;
        let mut output = ToolOutput::ok(original.clone());

        super::materialize_tool_output_with_quota(
            &mut output,
            &ctx,
            "git",
            quota,
            super::TOOL_RESULT_QUOTA_LOCK_BUDGET,
        );

        let artifact = output.artifact.expect("配额恰好容纳时应正常 spill");
        assert_eq!(
            std::fs::read(root.join(artifact.relative_path)).unwrap(),
            original.as_bytes()
        );
        assert_eq!(
            super::tool_result_storage_bytes(&artifact_root).unwrap(),
            quota
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shadow_telemetry_records_32k_without_changing_model_input() {
        let root = std::env::temp_dir().join(format!(
            "kz-d245-shadow-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let ctx = ToolCtx::new(root.clone(), root.clone()).with_identity(
            "worktree-shadow".into(),
            "project-shadow".into(),
            "run-shadow".into(),
            "process-shadow".into(),
        );
        let original = "x".repeat(super::TOOL_RESULT_SHADOW_THRESHOLD + 1);
        let mut output = ToolOutput::ok(original.clone());

        super::materialize_tool_output(&mut output, &ctx, "bash");

        assert_eq!(output.content, original);
        assert!(output.artifact.is_none());
        let records = std::fs::read_dir(root.join(".kanzei/artifacts/tool-results/shadow"))
            .unwrap()
            .map(|entry| {
                serde_json::from_slice::<serde_json::Value>(
                    &std::fs::read(entry.unwrap().path()).unwrap(),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["tool_name"], "bash");
        assert_eq!(records[0]["bytes"], original.len());
        assert_eq!(records[0]["would_spill"], true);
        assert_eq!(records[0]["actual_spilled"], false);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn oversized_tool_output_is_externalized_with_recoverable_bytes() {
        let root = std::env::temp_dir().join(format!(
            "kz-d349-spill-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let original = "x".repeat(super::TOOL_RESULT_SPILL_THRESHOLD + 17);
        let mut output = ToolOutput::ok(original.clone()).with_display(serde_json::json!({
            "kind": "terminal",
            "full": "must not remain in the event",
        }));

        super::materialize_tool_output(&mut output, &ctx, "git");

        let artifact = output
            .artifact
            .as_ref()
            .expect("大结果必须有 artifact 引用");
        assert_eq!(artifact.bytes, original.len() as u64);
        assert_eq!(artifact.sha256.len(), 64);
        assert!(output.content.contains("tool_result_externalized"));
        drop(ctx);
        let restarted_ctx = ToolCtx::new(root.clone(), root.clone());
        assert_eq!(
            std::fs::read(restarted_ctx.project_root.join(&artifact.relative_path)).unwrap(),
            original.as_bytes()
        );
        assert!(artifact.retrieval_hint.contains(&artifact.relative_path));
        assert_eq!(output.display.as_ref().unwrap()["kind"], "artifact");
        assert!(output.display.as_ref().unwrap().get("full").is_none());
        // UI-0926 #6:外置后发给 UI 的正文是外置标记文本,不是 1 MB 原文。
        let RunEvent::ToolEnd {
            content,
            content_bytes,
            ..
        } = RunEvent::tool_end("c1".into(), "git".into(), &output)
        else {
            panic!("tool_end 必须构造 ToolEnd");
        };
        assert!(
            content.starts_with("[tool_result_externalized"),
            "{content}"
        );
        assert_eq!(content, output.content);
        assert_eq!(content_bytes, output.content.len());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn artifact_write_failure_is_visible_without_success_reference() {
        let root = quota_test_root("spill-failure");
        let original = "x".repeat(super::TOOL_RESULT_SPILL_THRESHOLD + 1);
        // artifact 路径被目录(非普通文件)占用:走 inspect_stored_artifact 的 NotRegular
        // 分支,根本不会调用 write_atomic;这不属于配额计量问题,仍按失败处理。
        // write_atomic 真正失败的分支见 artifact_rename_failure_is_visible_without_success_reference。
        std::fs::create_dir_all(root.join(format!(
            ".kanzei/artifacts/tool-results/tool-bash-{}.txt",
            sha256_hex(&original)
        )))
        .unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let mut output = ToolOutput::ok(original);

        super::materialize_tool_output(&mut output, &ctx, "bash");

        assert!(output.is_error);
        assert_eq!(output.code, Some("TOOL_RESULT_SPILL_FAILED"));
        assert!(output.artifact.is_none());
        assert!(output.content.contains("tool_result_spill_failed"));
        let _ = std::fs::remove_dir_all(root);
    }

    /// write_atomic 本身失败(Err → SpillDecision::Failed):同名、长度不符的 artifact
    /// 被 share_mode(0) 独占句柄占住,重写时 rename 替换不了目标(与 atomic_file 自己
    /// 模拟 rename 失败的办法相同)。
    #[cfg(windows)]
    #[test]
    fn artifact_rename_failure_is_visible_without_success_reference() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = quota_test_root("spill-rename-failure");
        let artifact_root = root.join(".kanzei/artifacts/tool-results");
        std::fs::create_dir_all(&artifact_root).unwrap();
        let original = "x".repeat(super::TOOL_RESULT_SPILL_THRESHOLD + 1);
        let path = artifact_root.join(format!("tool-bash-{}.txt", sha256_hex(&original)));
        std::fs::write(&path, b"stale partial artifact").unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let mut output = ToolOutput::ok(original);

        super::materialize_tool_output(&mut output, &ctx, "bash");
        drop(lock);

        assert!(output.is_error);
        assert_eq!(output.code, Some("TOOL_RESULT_SPILL_FAILED"));
        assert!(output.artifact.is_none());
        assert!(output.content.contains("tool_result_spill_failed"));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"stale partial artifact",
            "rename 失败时目标原样未动"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn declined_tool_batch_keeps_real_and_placeholder_results_paired() {
        let calls = vec![
            (
                "call_done".into(),
                "write".into(),
                serde_json::json!({}),
                "{}".into(),
            ),
            (
                "call_declined".into(),
                "edit".into(),
                serde_json::json!({}),
                "{}".into(),
            ),
            (
                "call_pending".into(),
                "bash".into(),
                serde_json::json!({}),
                "{}".into(),
            ),
        ];
        let mut results = vec![Part::ToolResult {
            call_id: "call_done".into(),
            content: "真实写入结果".into(),
            is_error: false,
        }];
        append_declined_tool_results(&mut results, &calls, 1);

        assert_eq!(results.len(), 3);
        assert!(matches!(
            &results[0],
            Part::ToolResult { call_id, content, is_error: false }
                if call_id == "call_done" && content == "真实写入结果"
        ));
        assert!(matches!(
            &results[1],
            Part::ToolResult { call_id, is_error: true, content }
                if call_id == "call_declined" && content.contains("declined")
        ));
        assert!(matches!(
            &results[2],
            Part::ToolResult { call_id, is_error: true, content }
                if call_id == "call_pending" && content.contains("cancelled")
        ));
    }

    /// R-171 批2:writer 阶段 max in-flight=1(wave 上限 1 时逐条执行且不重叠)。
    /// drive 在 ReadParallelWriteSerial 下直接走串行路径不调 wave;此测试锚定
    /// wave 路径若被复用(如防御性回退)同样满足「任意时刻最多一个工具执行」。
    #[tokio::test]
    async fn max_parallel_1_强制串行_结果按下标归位() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let tool = Arc::new(ProbeTool {
            name: "probe_read",
            concurrency: ToolConcurrency::Shared("worktree:test".into()),
            in_flight: in_flight.clone(),
            max_in_flight: max_in_flight.clone(),
        });
        let calls = vec![
            probe_call(
                0,
                "call_1",
                serde_json::json!({"label": "first", "delay_ms": 20}),
                tool.clone(),
            ),
            probe_call(
                1,
                "call_2",
                serde_json::json!({"label": "second", "delay_ms": 20}),
                tool.clone(),
            ),
            probe_call(
                2,
                "call_3",
                serde_json::json!({"label": "third", "delay_ms": 20}),
                tool,
            ),
        ];
        let ctx = ToolCtx::new(std::env::temp_dir(), std::env::temp_dir());
        let mut on_event = |_event| {};
        let results = execute_prepared_tools(calls, &ctx, 1, true, &mut on_event).await;

        assert_eq!(
            max_in_flight.load(Ordering::SeqCst),
            1,
            "writer 阶段任意时刻最多一个工具执行"
        );
        assert_eq!(results.len(), 3);
        // 结果按下标与调用顺序对齐。
        for (idx, (call_index, part, images)) in results.iter().enumerate() {
            assert!(images.is_empty(), "纯文本工具不得产生图片 Part");
            assert_eq!(*call_index, idx);
            let expect_id = format!("call_{}", idx + 1);
            assert!(matches!(
                part,
                Part::ToolResult { call_id, is_error: false, .. }
                    if *call_id == expect_id
            ));
        }
    }

    // ---- R-249:图片投递与降级 ----

    fn output_with_images(n: usize) -> ToolOutput {
        ToolOutput::ok("done").with_images(
            (0..n)
                .map(|i| kanzei_harness::ToolImage {
                    media_type: "image/png".into(),
                    data: format!("payload{i}"),
                })
                .collect(),
        )
    }

    #[test]
    fn images_pass_through_when_provider_supports_them() {
        let (parts, note) = tool_images_to_parts(&output_with_images(2), true);
        assert_eq!(parts.len(), 2);
        assert!(note.is_none(), "支持图片时不应产生降级说明");
        assert!(matches!(
            &parts[0],
            Part::Image { media_type, data } if media_type == "image/png" && data == "payload0"
        ));
    }

    #[test]
    fn images_degrade_to_explicit_note_when_unsupported() {
        // 关键不变式:不支持时**一个 Image part 都不能进历史**。进了历史就会跟着
        // 每一轮重发,client.rs 的硬拒绝会让这条对话在该 provider 上永久不可用。
        let (parts, note) = tool_images_to_parts(&output_with_images(3), false);
        assert!(parts.is_empty(), "不支持图片时不得放行任何 Image part");
        let note = note.expect("必须给出显式降级说明,不能静默丢弃");
        assert!(note.contains('3'), "说明里要写清丢了几张: {note}");
        assert!(
            note.contains("did NOT see"),
            "必须明确告诉模型它没看到图,否则它会照着文本编内容: {note}"
        );
    }

    #[test]
    fn no_images_means_no_note_on_either_path() {
        // 回归:纯文本工具的返回在两种能力下都必须逐字节不变。
        for supported in [true, false] {
            let (parts, note) = tool_images_to_parts(&ToolOutput::ok("plain"), supported);
            assert!(parts.is_empty());
            assert!(note.is_none());
        }
    }

    // ---- D-661:切波的并行度与顺序不变式 ----

    /// 只关心并发契约的轻量调用构造(不执行,只喂给切波函数)。
    fn wave_call(index: usize, concurrency: ToolConcurrency) -> PreparedToolCall {
        let tool = Arc::new(ProbeTool {
            name: "probe_wave",
            concurrency: concurrency.clone(),
            in_flight: Arc::new(AtomicUsize::new(0)),
            max_in_flight: Arc::new(AtomicUsize::new(0)),
        });
        PreparedToolCall {
            index,
            id: format!("call_{index}"),
            name: "probe_wave".into(),
            input: serde_json::json!({}),
            tool,
            concurrency,
        }
    }

    fn wave_shape(waves: &[Vec<PreparedToolCall>]) -> Vec<Vec<usize>> {
        waves
            .iter()
            .map(|wave| wave.iter().map(|call| call.index).collect())
            .collect()
    }

    #[test]
    fn 切波_不相交冲突对不再各自占一波() {
        // D-661 见证:A↔B 冲突、C↔D 冲突、跨对互不冲突。
        // 旧实现顺序扫描封波得 [[A],[B,C],[D]] 三波;两波足够,且不动任何冲突对的先后。
        let a = ToolConcurrency::WorktreeWrite("tree:a".into());
        let c = ToolConcurrency::WorktreeWrite("tree:c".into());
        let waves = build_tool_execution_waves_with(
            8,
            vec![
                wave_call(0, a.clone()),
                wave_call(1, a),
                wave_call(2, c.clone()),
                wave_call(3, c),
            ],
        );
        assert_eq!(
            wave_shape(&waves),
            vec![vec![0, 2], vec![1, 3]],
            "两条独立冲突链应当并肩推进,而不是串成三波"
        );
    }

    #[test]
    fn 切波_冲突对的先后顺序绝不颠倒() {
        // 不变式护栏:i<j 且冲突 => wave(i) < wave(j)。
        // 朴素 first-fit 装箱会把 2 塞进 wave0(与 0 不冲突),于是 2 跑在 1 前面——
        // 同一棵树上的两次写被颠倒,那是换语义不是提速。别改成 first-fit。
        let shared = ToolConcurrency::Shared("tree:x".into());
        let write = ToolConcurrency::WorktreeWrite("tree:x".into());
        let waves = build_tool_execution_waves_with(
            8,
            vec![
                wave_call(0, shared.clone()),
                wave_call(1, write.clone()),
                wave_call(2, shared),
            ],
        );
        let shape = wave_shape(&waves);
        let wave_of = |index: usize| {
            shape
                .iter()
                .position(|wave| wave.contains(&index))
                .expect("每个调用都必须落在某个波里")
        };
        assert!(wave_of(0) < wave_of(1), "读(0)与写(1)冲突,0 必须先跑");
        assert!(wave_of(1) < wave_of(2), "写(1)与读(2)冲突,1 必须先跑");
        assert_eq!(shape, vec![vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn 切波_全不冲突时按容量装满() {
        let waves = build_tool_execution_waves_with(
            2,
            (0..5)
                .map(|i| wave_call(i, ToolConcurrency::Shared("tree:x".into())))
                .collect(),
        );
        assert_eq!(wave_shape(&waves), vec![vec![0, 1], vec![2, 3], vec![4]]);
    }

    #[test]
    fn 切波_exclusive_仍然独占且串行() {
        // Exclusive 与任何东西冲突(含彼此),必须一个一波、顺序不变。
        let waves = build_tool_execution_waves_with(
            8,
            vec![
                wave_call(0, ToolConcurrency::Exclusive),
                wave_call(1, ToolConcurrency::Shared("tree:x".into())),
                wave_call(2, ToolConcurrency::Exclusive),
            ],
        );
        assert_eq!(wave_shape(&waves), vec![vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn 切波_容量满时只向后顺延不破坏不变式() {
        // 容量 1:即使互不冲突也得排队,且顺序保持。
        let waves = build_tool_execution_waves_with(
            1,
            (0..3)
                .map(|i| wave_call(i, ToolConcurrency::Shared("tree:x".into())))
                .collect(),
        );
        assert_eq!(wave_shape(&waves), vec![vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn 切波_空输入与单调用() {
        assert!(build_tool_execution_waves_with(8, Vec::new()).is_empty());
        let waves =
            build_tool_execution_waves_with(8, vec![wave_call(0, ToolConcurrency::Exclusive)]);
        assert_eq!(wave_shape(&waves), vec![vec![0]]);
    }
}
