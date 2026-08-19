//! conventions 工具:开发规范 `.kanzei/project/conventions.md` 的专用写通道。
//!
//! D-235 与 D-173 同根因:`.kanzei/project/*` 对 write/edit 硬 deny,而 conventions.md
//! 没有任何专用工具,于是模型唯一的合法写路径不可达,只能去找 shell 旁路。本工具把
//! 那条路补上,并且比裸 write 多三层硬门禁(与 architecture 工具同源):
//!
//! 1. CAS(expected_hash):基于 **get 读到的那一版** 才能 patch,并发手改不会被静默覆盖。
//! 2. 逐字替换 + 唯一命中:old_string 必须恰好出现一次,0 次或多次都拒写并说明,
//!    防止「改错地方」和「整段内容被顶掉」这两类 edit 事故(D-004:拒绝的理由必须说
//!    出来,绝不静默)。
//! 3. 只认这一个文件:路径由引擎给定,输入里没有 path 参数可以指到别处。
//!
//! 与 architecture 的差异:conventions.md 是 15KB 级用户手写规范,整文件替换风险远高于
//! 架构索引,所以写动作收敛为**定点补丁**(patch)而不是 update(整文件替换);读动作
//! get 给出全文 + hash + 标题导航,让模型在不依赖外部快照的情况下完成补丁。

use std::path::Path;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::architecture::content_hash;

/// 开发规范(相对项目根)。
pub const CONVENTIONS_REL: &str = ".kanzei/project/conventions.md";

#[derive(Deserialize, JsonSchema)]
struct ConventionsInput {
    /// get(读全文+hash+标题导航) | patch(逐字替换,唯一命中才写)
    action: String,
    /// patch 必填:要替换的原文(必须恰好出现一次)
    #[serde(default)]
    old_string: Option<String>,
    /// patch 必填:替换后的新文本
    #[serde(default)]
    new_string: Option<String>,
    /// patch 必填:上一次 get 返回的 hash(并发写入保护)
    #[serde(default)]
    expected_hash: Option<String>,
}

pub struct ConventionsTool;

#[async_trait]
impl Tool for ConventionsTool {
    fn name(&self) -> &'static str {
        "conventions"
    }

    fn description(&self) -> String {
        format!(
            "Read and maintain the dev rules document `{CONVENTIONS_REL}` (the ONLY write \
             channel for it — write/edit are denied there). Actions: get (full text + hash + \
             heading navigation), patch (replace exactly one occurrence of old_string with \
             new_string; refuses when the hash from the last get is stale, when old_string \
             matches 0 places, or when it matches 2+ places — nothing is written then). \
             patch is a surgical replacement, never a whole-file rewrite."
        )
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(ConventionsInput)).unwrap();
        if let Some(action) = schema
            .pointer_mut("/properties/action")
            .and_then(|v| v.as_object_mut())
        {
            action.insert("enum".into(), serde_json::json!(["get", "patch"]));
        }
        schema
    }

    /// 权限资源 = 子动作,读写可分别授权(get 只读,patch 改盘)。
    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["action"].as_str().unwrap_or("*").to_string()]
    }

    fn concurrency(&self, input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        match input["action"].as_str() {
            Some("patch") => ToolConcurrency::write_worktree(ctx),
            _ => ToolConcurrency::shared_worktree(ctx),
        }
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: ConventionsInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let path = ctx.project_root.join(CONVENTIONS_REL);

        match input.action.as_str() {
            "get" => {
                if !path.exists() {
                    return ToolOutput::error(format!(
                        "{CONVENTIONS_REL} does not exist at {}",
                        path.display()
                    ));
                }
                let current = read_text(&path);
                let headings: Vec<&str> = current
                    .lines()
                    .filter(|l| l.trim_start().starts_with('#'))
                    .collect();
                let mut out = format!(
                    "path: {CONVENTIONS_REL}\nhash: {}\nlines: {}\nheadings:\n",
                    content_hash(&current),
                    current.lines().count(),
                );
                for heading in headings.iter().take(60) {
                    out.push_str(&format!("  {heading}\n"));
                }
                if headings.is_empty() {
                    out.push_str("  (no markdown headings)\n");
                }
                out.push_str("---\n");
                out.push_str(&current);
                ToolOutput::ok(out)
            }
            "patch" => {
                let Some(old_string) = input.old_string else {
                    return ToolOutput::error(
                        "`old_string` (the exact text to replace) is required for patch",
                    );
                };
                let Some(new_string) = input.new_string else {
                    return ToolOutput::error(
                        "`new_string` (the replacement text) is required for patch",
                    );
                };
                // agent→工具参数在协议层常把换行转义成字面 `\n`/`\r\n`/`\r` 文本
                // (测试代码直接构造 JSON 时则是真换行)。统一解码,两种输入等价。
                let old_string = decode_escaped_newlines(&old_string);
                let new_string = decode_escaped_newlines(&new_string);
                if old_string.is_empty() {
                    return ToolOutput::error(
                        "`old_string` must not be empty — an empty needle matches every position",
                    );
                }
                let Some(expected) = input.expected_hash else {
                    return ToolOutput::error(format!(
                        "`expected_hash` is required for patch — call `get` first and pass the \
                         hash it returned. Current hash: {}",
                        content_hash(&read_text(&path))
                    ));
                };
                if !path.exists() {
                    return ToolOutput::error(format!(
                        "{CONVENTIONS_REL} does not exist at {}",
                        path.display()
                    ));
                }
                let current = read_text(&path);
                let current_hash = content_hash(&current);
                if expected != current_hash {
                    return ToolOutput::error(format!(
                        "stale expected_hash `{expected}`; the file is now `{current_hash}` \
                         (someone edited {CONVENTIONS_REL} since your last get). Re-run `get`, \
                         apply your patch onto the current text, and patch again — do NOT \
                         resubmit the old text."
                    ));
                }
                let matches: Vec<_> = current.match_indices(&old_string).collect();
                if matches.len() != 1 {
                    // CRLF 文件里,跨行 old_string 几乎不可能用 LF 参数逐字命中;
                    // 先按 LF 归一化重试一次,再决定报错文案。
                    let lf_current = normalize_lf(&current);
                    let lf_old = normalize_lf(&old_string);
                    let lf_matches: Vec<_> = lf_current.match_indices(&lf_old).collect();
                    if lf_matches.len() == 1 {
                        let (start, end) =
                            map_lf_range(&current, lf_matches[0].0, lf_matches[0].1.len());
                        let mut new_content = current.clone();
                        new_content.replace_range(start..end, &new_string);
                        return write_patch_with_log(ctx, &path, current, new_content, &expected);
                    }
                }
                match matches.len() {
                    0 => {
                        return ToolOutput::error(format!(
                            "`old_string` not found in {CONVENTIONS_REL} (0 occurrences). \
                             Nothing was written. Re-read the current text via `get` and \
                             align old_string to it exactly (whitespace included)."
                        ));
                    }
                    1 => {}
                    n => {
                        return ToolOutput::error(format!(
                            "`old_string` matches {n} locations in {CONVENTIONS_REL}; refusing \
                             to patch non-uniquely. Nothing was written. Widen old_string to a \
                             unique region (surrounding line context), or patch each occurrence \
                             separately."
                        ));
                    }
                }
                let new_content = current.replacen(&old_string, &new_string, 1);
                write_patch_with_log(ctx, &path, current, new_content, &expected)
            }
            other => ToolOutput::error(format!("unknown action `{other}`; valid: get | patch")),
        }
    }
}

/// D-398:conventions 专用写者写盘成功后记写日志(围栏收口归因凭据,此前零接入)。
fn write_patch_with_log(
    ctx: &ToolCtx,
    path: &Path,
    current: String,
    new_content: String,
    expected: &str,
) -> ToolOutput {
    let out = write_patch(path, current, new_content, expected);
    if !out.is_error {
        crate::record_write_log(ctx, CONVENTIONS_REL, path);
    }
    out
}

/// 执行写盘 + 产出 diff 显示。new_content 里的裸换行统一为文件既有风格(CRLF 文件不
/// 因 LF 参数产生混合换行)。
fn write_patch(path: &Path, current: String, new_content: String, expected: &str) -> ToolOutput {
    let new_content = if current.contains("\r\n") {
        normalize_lf(&new_content).replace('\n', "\r\n")
    } else {
        new_content
    };
    // D-364:conventions.md 与 bash 围栏共用同一把锁——围栏命令窗口内,并发 patch
    // 在锁上等待,命令结束后落盘,不会被围栏的快照回滚误抹掉。CAS 防并发手改,
    // 锁防围栏误回滚,两者互补。expected 若是窗口前读的,等锁拿到后 CAS 会如实报
    // stale,绝不假成功。
    let _lock = match crate::atomic_file::lock_exclusive(path) {
        Ok(lock) => lock,
        Err(e) => return ToolOutput::error(format!("cannot lock {CONVENTIONS_REL}: {e}")),
    };
    if let Err(e) = crate::atomic_file::write_atomic_cas(path, &new_content, expected, content_hash)
    {
        return ToolOutput::error(e);
    }
    let diff_lines = new_content.lines().count() as i64 - current.lines().count() as i64;
    ToolOutput::ok(format!(
        "patched {CONVENTIONS_REL} ({} lines, {diff_lines:+} vs before)\nhash: {}",
        new_content.lines().count(),
        content_hash(&new_content),
    ))
    .with_display(serde_json::json!({
        "kind": "diff",
        "path": CONVENTIONS_REL,
        "before": current,
        "after": new_content,
    }))
}

/// CRLF → LF 归一化(只删成对 `\r\n` 里的 `\r`,单独 `\r` 保留)。
fn normalize_lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// 解码协议层把换行转义成的字面序列(`\n` / `\r\n` / `\r` 文本形式),
/// 还原为真换行,使字面转义与真换行两种参数输入等价。
fn decode_escaped_newlines(s: &str) -> String {
    s.replace("\\r\\n", "\n")
        .replace("\\r", "\n")
        .replace("\\n", "\n")
}

/// 把 LF 归一化文本上的字节区间 [lf_start, lf_start + lf_len) 映射回原始文本
/// (含 `\r\n`)的字节区间。原理:归一化文本 = 原文删去每个 CRLF 的 `\r`;
/// 原文偏移 = 归一化偏移 + 前缀里被删掉的 `\r` 数。
fn map_lf_range(original: &str, lf_start: usize, lf_len: usize) -> (usize, usize) {
    let bytes = original.as_bytes();
    let mut i = 0usize;
    let mut seen = 0usize;
    let mut orig = 0usize;
    while i < bytes.len() && seen < lf_start {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            i += 1;
            orig += 1; // \r 在原文占 1 字节,\n 在下一轮计入 seen
            continue;
        }
        seen += 1;
        i += 1;
        orig += 1;
    }
    let start = orig;
    let mut walked = 0usize;
    while i < bytes.len() && walked < lf_len {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            i += 1;
            orig += 1;
            continue;
        }
        walked += 1;
        i += 1;
        orig += 1;
    }
    (start, orig)
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NONCE: AtomicUsize = AtomicUsize::new(0);

    fn tmp_dir() -> PathBuf {
        let nonce = NONCE.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "kanzei-conventions-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        dir
    }

    const SAMPLE: &str = "## 1.4 验证与提交节奏参数\n\n- 默认值: 全量测试只在关闭前跑。\n\n## 2. 代码修改原则\n\n- 小步可验证。\n";

    /// 在临时项目里写一份 SAMPLE,执行一次工具调用,返回 (输出, 项目根)。
    async fn run(
        action: &str,
        old: Option<&str>,
        new: Option<&str>,
        expected: Option<&str>,
    ) -> (ToolOutput, PathBuf) {
        let root = tmp_dir();
        std::fs::write(root.join(CONVENTIONS_REL), SAMPLE).unwrap();
        let mut input = serde_json::json!({ "action": action });
        if let Some(v) = old {
            input["old_string"] = serde_json::json!(v);
        }
        if let Some(v) = new {
            input["new_string"] = serde_json::json!(v);
        }
        if let Some(v) = expected {
            input["expected_hash"] = serde_json::json!(v);
        }
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let out = ConventionsTool.execute(input, &ctx).await;
        (out, root)
    }

    #[tokio::test]
    async fn get_returns_text_hash_and_headings() {
        let (out, _) = run("get", None, None, None).await;
        assert!(!out.is_error, "get 应成功: {}", out.content);
        let text = &out.content;
        assert!(
            text.contains("path: .kanzei/project/conventions.md"),
            "{text}"
        );
        assert!(text.contains("hash: "), "{text}");
        assert!(
            text.contains("## 1.4 验证与提交节奏参数"),
            "标题导航缺失: {text}"
        );
        assert!(text.contains("## 2. 代码修改原则"), "标题导航缺失: {text}");
        assert!(text.contains("小步可验证"), "全文未返回: {text}");
        assert!(
            text.contains(&content_hash(SAMPLE)),
            "get 未返回当前 hash: {text}"
        );
    }

    #[tokio::test]
    async fn get_missing_file_is_error() {
        let root = tmp_dir();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let out = ConventionsTool
            .execute(serde_json::json!({ "action": "get" }), &ctx)
            .await;
        assert!(out.is_error, "缺失文件应报错: {}", out.content);
        assert!(out.content.contains("does not exist"), "{}", out.content);
    }

    #[tokio::test]
    async fn patch_unique_replace_writes_and_returns_new_hash() {
        let root = tmp_dir();
        std::fs::write(root.join(CONVENTIONS_REL), SAMPLE).unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let out = ConventionsTool
            .execute(
                serde_json::json!({
                    "action": "patch",
                    "old_string": "全量测试只在关闭前跑",
                    "new_string": "全量测试只在复杂度中/大条目关闭前跑(引擎已接管)",
                    "expected_hash": content_hash(SAMPLE),
                }),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "patch 应成功: {}", out.content);
        let written = std::fs::read_to_string(root.join(CONVENTIONS_REL)).unwrap();
        assert!(written.contains("(引擎已接管)"), "补丁未落盘: {written}");
        assert!(
            written.contains("## 2. 代码修改原则"),
            "无关小节被改动: {written}"
        );
        assert!(
            out.content.contains(&content_hash(&written)),
            "返回 hash 与落盘内容不一致: {}",
            out.content
        );
    }

    /// D-398:conventions patch 成功后记写日志(围栏收口归因凭据,此前零接入)。
    #[tokio::test]
    async fn patch_写盘后记写日志() {
        let root = tmp_dir();
        std::fs::write(root.join(CONVENTIONS_REL), SAMPLE).unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let out = ConventionsTool
            .execute(
                serde_json::json!({
                    "action": "patch",
                    "old_string": "全量测试只在关闭前跑",
                    "new_string": "全量测试只在关闭前跑(改)",
                    "expected_hash": content_hash(SAMPLE),
                }),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let logs = crate::write_log::entries_after(&root, 0);
        assert!(
            logs.iter().any(|l| l.path.ends_with(CONVENTIONS_REL)),
            "conventions.md 应有写日志: {:?}",
            logs.iter().map(|l| &l.path).collect::<Vec<_>>()
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn patch_zero_matches_refuses() {
        let (out, root) = run(
            "patch",
            Some("这句话在文件里不存在"),
            Some("替换"),
            Some(&content_hash(SAMPLE)),
        )
        .await;
        assert!(out.is_error, "0 命中不应成功: {}", out.content);
        assert!(out.content.contains("0 occurrences"), "{}", out.content);
        assert_eq!(
            std::fs::read_to_string(root.join(CONVENTIONS_REL)).unwrap(),
            SAMPLE,
            "拒绝时不得改动文件"
        );
    }

    #[tokio::test]
    async fn patch_multiple_matches_refuses() {
        // SAMPLE 里两个标题都以 "## " 开头,old_string 取 "## " 命中 2 处。
        let (out, root) = run(
            "patch",
            Some("## "),
            Some("# "),
            Some(&content_hash(SAMPLE)),
        )
        .await;
        assert!(out.is_error, "多命中不应成功: {}", out.content);
        assert!(
            out.content.contains("matches 2 locations"),
            "{}",
            out.content
        );
        assert_eq!(
            std::fs::read_to_string(root.join(CONVENTIONS_REL)).unwrap(),
            SAMPLE,
            "拒绝时不得改动文件"
        );
    }

    #[tokio::test]
    async fn patch_stale_hash_refuses() {
        let (out, root) = run(
            "patch",
            Some("小步可验证"),
            Some("小步、可验证、可回滚"),
            Some("deadbeef"),
        )
        .await;
        assert!(out.is_error, "陈旧 hash 不应成功: {}", out.content);
        assert!(
            out.content.contains("stale expected_hash"),
            "{}",
            out.content
        );
        assert_eq!(
            std::fs::read_to_string(root.join(CONVENTIONS_REL)).unwrap(),
            SAMPLE,
            "拒绝时不得改动文件"
        );
    }

    #[tokio::test]
    async fn patch_requires_expected_hash() {
        let (out, _) = run("patch", Some("小步可验证"), Some("替换"), None).await;
        assert!(out.is_error, "缺 expected_hash 不应成功: {}", out.content);
        assert!(out.content.contains("expected_hash"), "{}", out.content);
    }

    /// CRLF 文件 + LF 参数的跨行 patch:逐字匹配必然 0 命中,归一化后应成功,
    /// 且写回不产生混合换行(整文件仍 CRLF)。
    #[tokio::test]
    async fn patch_tolerates_crlf_files_with_lf_old_string() {
        let root = tmp_dir();
        let crlf = "## 1. 语言与沟通\r\n\r\n- 旧行一。\r\n- 旧行二。\r\n\r\n## 2. 之后\r\n";
        std::fs::write(root.join(CONVENTIONS_REL), crlf).unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let input = serde_json::json!({
            "action": "patch",
            "old_string": "- 旧行一。\n- 旧行二。",
            "new_string": "- 新行。",
            "expected_hash": content_hash(crlf),
        });
        let out = ConventionsTool.execute(input.clone(), &ctx).await;
        assert!(
            !out.is_error,
            "CRLF 文件 + LF old_string 应可 patch: {}",
            out.content
        );
        let after = std::fs::read_to_string(root.join(CONVENTIONS_REL)).unwrap();
        assert_eq!(
            after, "## 1. 语言与沟通\r\n\r\n- 新行。\r\n\r\n## 2. 之后\r\n",
            "替换内容正确且换行统一为 CRLF"
        );
        assert!(!after.replace("\r\n", "").contains('\n'), "不得混合换行");
        // LF 文件 + CRLF old_string 也容忍。
        let root2 = tmp_dir();
        let lf = "## 1.\n\n- 甲。\n\n## 2.\n";
        std::fs::write(root2.join(CONVENTIONS_REL), lf).unwrap();
        let ctx2 = ToolCtx::new(root2.clone(), root2.clone());
        let input2 = serde_json::json!({
            "action": "patch",
            "old_string": "- 甲。\r\n\r\n## 2.",
            "new_string": "- 乙。\r\n\r\n## 2.",
            "expected_hash": content_hash(lf),
        });
        let out2 = ConventionsTool.execute(input2.clone(), &ctx2).await;
        assert!(
            !out2.is_error,
            "LF 文件 + CRLF old_string 也应可 patch: {}",
            out2.content
        );
    }

    /// 复现真实 conventions.md 场景:跨行 old_string 在 CRLF 文件上匹配。
    #[tokio::test]
    async fn multi_line_old_string_matches_crlf_file() {
        let root = tmp_dir();
        let crlf = "## 1.1 需求取活与阻塞调度\r\n\r\n- 需求列表按文件顺序自上而下扫描，顺序代表用户意图；priority 只用于背景判断，不得改变取活顺序。\r\n- **`阻塞:` 字段只留给外部阻塞**——即解除权不在 agent 手里的四类：①已经把方案发给用户、正在等回复；②缺凭据/权限/环境；③依赖外部服务或他人；④用户明确宣布该条自己直营。写入时必须带**具名的解除人**（谁做什么才能解除），写不出具名解除人的就不是外部阻塞。\r\n\r\n## 1.2 关闭边界\r\n";
        std::fs::write(root.join(CONVENTIONS_REL), crlf).unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let input = serde_json::json!({
            "action": "patch",
            "old_string": "- 需求列表按文件顺序自上而下扫描，顺序代表用户意图；priority 只用于背景判断，不得改变取活顺序。\n- **`阻塞:` 字段只留给外部阻塞**——即解除权不在 agent 手里的四类：①已经把方案发给用户、正在等回复；②缺凭据/权限/环境；③依赖外部服务或他人；④用户明确宣布该条自己直营。写入时必须带**具名的解除人**（谁做什么才能解除），写不出具名解除人的就不是外部阻塞。",
            "new_string": "- 替换行。",
            "expected_hash": content_hash(crlf),
        });
        let out = ConventionsTool.execute(input.clone(), &ctx).await;
        assert!(
            !out.is_error,
            "多行 old_string 应可匹配 CRLF 文件: {}",
            out.content
        );
        let after = std::fs::read_to_string(root.join(CONVENTIONS_REL)).unwrap();
        assert_eq!(
            after, "## 1.1 需求取活与阻塞调度\r\n\r\n- 替换行。\r\n\r\n## 1.2 关闭边界\r\n",
            "替换内容正确"
        );
    }

    /// 归一化区间映射:验证 LF 偏移 → CRLF 原文偏移的换算。
    #[test]
    fn map_lf_range_maps_across_crlf() {
        let original = "aa\r\nbb\r\ncc";
        // 归一化文本: aa\nbb\ncc
        // lf 偏移 0..2 → 原文 0..2(aa)
        assert_eq!(map_lf_range(original, 0, 2), (0, 2));
        // lf 偏移 3..5(bb)→ 原文 4..6(每个 \r\n 前多一个 \r)
        assert_eq!(map_lf_range(original, 3, 2), (4, 6));
        // lf 偏移 6..8(cc)→ 原文 8..10
        assert_eq!(map_lf_range(original, 6, 2), (8, 10));
        // 空匹配
        assert_eq!(map_lf_range(original, 0, 0), (0, 0));
    }
}
