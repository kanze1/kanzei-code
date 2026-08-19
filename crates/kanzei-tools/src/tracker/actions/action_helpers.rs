//! action 路由共用的校验、展示与错误文案辅助函数。
//!
//! 这些函数只服务于 `tracker::actions`，单独成文件让 action 分发文件保留在可读规模，
//! 不改变任何校验顺序、错误文本或序列化行为。

use std::path::Path;
use std::process::Command;

use crate::docstore::{DocStore, Entry};

/// R-306 验收⑤:关闭带仓库观测锚点的活动条目时，observed_head 必须已进入当前
/// HEAD 祖先链。否则拒绝关闭，迫使调用方先完成收编或在条目里登记真实的收编处置。
/// 已终态条目由 update_close 的幂等重入路径跳过本检查。
pub(super) fn check_close_source_ancestry(entry: &Entry, cwd: &Path) -> Option<String> {
    let observed_head = entry
        .fields
        .iter()
        .find(|(key, _)| key == "observed_head")
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())?;
    let merged = Command::new("git")
        .args(["merge-base", "--is-ancestor", observed_head, "HEAD"])
        .current_dir(cwd)
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if merged {
        None
    } else {
        Some(format!(
            "observed_head `{observed_head}` 不在当前 HEAD 祖先链，拒绝关闭；请先完成并记录收编，或保留条目继续推进。"
        ))
    }
}

/// R-229:收集关闭文本里的「剩余/其余 N 处」式分类断言声明的 N。
/// 只认「剩余/其余 + 数字 + 处」的形态(允许空白),如「剩余 3 处」「其余 2 处」;
/// 「剩余价值」这类无数字的用法不算断言。返回每个断言声明的处数。
fn classification_claims(text: &str) -> Vec<usize> {
    let mut claims = Vec::new();
    let mut from = 0usize;
    while from < text.len() {
        let tail = &text[from..];
        let rem = tail.find("剩余");
        let oth = tail.find("其余");
        let pos = match (rem, oth) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => break,
        };
        // 标记后跳过空白,再数数字,数字后再跳过空白,期待「处」。
        let after = tail[pos + 6..].trim_start();
        let digits_len = after
            .char_indices()
            .take_while(|(_, c)| c.is_ascii_digit())
            .map(|(i, c)| i + c.len_utf8())
            .last()
            .unwrap_or(0);
        if digits_len > 0 {
            let n: usize = after[..digits_len].parse().unwrap_or(0);
            let rest = after[digits_len..].trim_start();
            if rest.starts_with('处') && n > 0 {
                claims.push(n);
            }
        }
        // 越过本次标记继续扫:pos 相对 tail(从 from 开始的切片),标记本身 6 字节,
        // 从这里推进必然落在字符边界上(「剩余/其余」是 3 个 CJK 字符,6 字节整)。
        from += pos + 6;
    }
    claims
}

/// R-229:数文本里 `[路径/]文件名.扩展名:行号` 形态的 file:line 引证,去重。
/// 只认 ASCII 路径字符(字母数字 `.` `/` `\` `-` `_`) + 冒号 + 数字,避免把
/// 「R-199:」「12:30」之类误算成引证。
fn file_line_citations(text: &str) -> std::collections::BTreeSet<String> {
    let mut cites = std::collections::BTreeSet::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b':' {
            // 回扫 token 起点(路径字符)。
            let mut j = i;
            while j > 0
                && (bytes[j - 1].is_ascii_alphanumeric() || b"./\\-_".contains(&bytes[j - 1]))
            {
                j -= 1;
            }
            let token = &text[j..i];
            // 必须以 `.扩展名` 结尾(扩展名非空字母数字),且含点——排除 `R-199`、`12` 等。
            let has_ext = token
                .rsplit_once('.')
                .map(|(_, e)| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()))
                .unwrap_or(false);
            // 冒号后必须是数字(行号)。
            let mut k = i + 1;
            while k < bytes.len() && bytes[k].is_ascii_digit() {
                k += 1;
            }
            if has_ext && k > i + 1 {
                cites.insert(format!("{token}:{}", &text[i + 1..k]));
            }
            i = k.max(i + 1);
        } else {
            i += 1;
        }
    }
    cites
}

/// R-229 关闭门禁:出现「剩余/其余 N 处」式分类断言时,关闭文本必须逐处带
/// file:line 引证,引证数(去重)不足断言声称的总处数即拒。根因:R-199 关闭证据
/// 把完整否决误归为「非续跑否决」且无人核对(产出 D-320/D-323)。无断言不受影响。
pub(super) fn check_close_classification_evidence(entry: &Entry) -> Option<String> {
    let mut text = entry.title.clone();
    for (_, value) in &entry.fields {
        text.push('\n');
        text.push_str(value);
    }
    let claims = classification_claims(&text);
    if claims.is_empty() {
        return None; // 验收③:无分类断言的关闭不受影响。
    }
    let required: usize = claims.iter().sum();
    let cites = file_line_citations(&text);
    if cites.len() < required {
        Some(format!(
            "关闭证据含「剩余/其余 N 处」式分类断言(共声称 {required} 处:{claims:?}),\
             但只找到 {} 处 file:line 引证,引证数不足即拒(R-229)。\
             分类断言必须逐处点名 file:line 并引码,如 `crates/kanzei-app/ui/08-compose.js:643`。",
            cites.len()
        ))
    } else {
        None
    }
}

/// 2026-08-16 审计门禁(D-389/D-401 一类的机制化):验收条款对账。
/// 验收字段用带圈数字(①…⑳)列条款时,关闭时的进展字段必须逐条覆盖:每个条款号
/// 至少出现一次,且其后到下一个条款号(至多 400 字符)内带证据锚——T- 测试记录、
/// file:line 引证、7 位以上提交号,或显式的「验收降级」/「由用户」缓办声明。
/// 防的是**沉默降级**(条款被跳过而关闭文本只字不提,或整段叙述不落到条款上);
/// 证据本身的真伪由波次审计(docs/design/bootstrap_quality_audit.md)负责——
/// 语法门禁不判语义,本仓一贯口径。无编号条款的验收、无验收字段的条目不受影响。
pub(super) fn check_close_acceptance_reconciliation(entry: &Entry) -> Option<String> {
    fn is_circled(c: char) -> bool {
        ('\u{2460}'..='\u{2473}').contains(&c) // ①..⑳
    }
    /// 7 位以上十六进制小写/数字 token(提交号形态,如 `2483818`/`b0bb1fc`),
    /// 按 ASCII 词边界认定——CJK 字节不是 ASCII 字母数字,天然成边界。
    fn has_commit_token(segment: &str) -> bool {
        let bytes = segment.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let is_hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
            if is_hex(bytes[i]) {
                let start = i;
                while i < bytes.len() && is_hex(bytes[i]) {
                    i += 1;
                }
                let bounded_left = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
                let bounded_right = i >= bytes.len() || !bytes[i].is_ascii_alphanumeric();
                if i - start >= 7 && bounded_left && bounded_right {
                    return true;
                }
            } else {
                i += 1;
            }
        }
        false
    }
    fn has_anchor(segment: &str) -> bool {
        if segment.contains("验收降级") || segment.contains("由用户") {
            return true;
        }
        // T-<数字> 测试记录引用。
        if segment.match_indices("T-").any(|(i, _)| {
            segment[i + 2..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        }) {
            return true;
        }
        if !file_line_citations(segment).is_empty() {
            return true;
        }
        has_commit_token(segment)
    }
    let acceptance = entry
        .fields
        .iter()
        .find(|(k, _)| k == "验收")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let mut markers: Vec<char> = Vec::new();
    for c in acceptance.chars().filter(|c| is_circled(*c)) {
        if !markers.contains(&c) {
            markers.push(c);
        }
    }
    if markers.is_empty() {
        return None;
    }
    let progress = entry
        .fields
        .iter()
        .find(|(k, _)| k == "进展")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let chars: Vec<char> = progress.chars().collect();
    let mut unmentioned: Vec<char> = Vec::new();
    let mut unanchored: Vec<char> = Vec::new();
    for &marker in &markers {
        let occurrences: Vec<usize> = chars
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == marker)
            .map(|(i, _)| i)
            .collect();
        if occurrences.is_empty() {
            unmentioned.push(marker);
            continue;
        }
        let covered = occurrences.iter().any(|&start| {
            let cap = (start + 1 + 400).min(chars.len());
            let end = chars[start + 1..cap]
                .iter()
                .position(|c| is_circled(*c))
                .map(|off| start + 1 + off)
                .unwrap_or(cap);
            let segment: String = chars[start + 1..end].iter().collect();
            has_anchor(&segment)
        });
        if !covered {
            unanchored.push(marker);
        }
    }
    if unmentioned.is_empty() && unanchored.is_empty() {
        return None;
    }
    let fmt = |v: &[char]| v.iter().map(|c| c.to_string()).collect::<Vec<_>>().join("");
    let mut problems = Vec::new();
    if !unmentioned.is_empty() {
        problems.push(format!("{} 在进展中未提及", fmt(&unmentioned)));
    }
    if !unanchored.is_empty() {
        problems.push(format!("{} 提及了但无证据锚", fmt(&unanchored)));
    }
    Some(format!(
        "验收条款对账未过:验收列出条款 {},其中 {}。关闭前每条验收必须在进展里逐条\
         覆盖并带证据锚——T- 测试记录 / file:line / 提交号;做不到的条款要显式写\
         『验收降级: <条款号> 原文→实际+理由』或『<条款号> 由用户执行』,沉默跳过即拒。\
         (证据真伪由波次审计另查,见 docs/design/bootstrap_quality_audit.md)",
        fmt(&markers),
        problems.join(";")
    ))
}

/// R-232:条目的用户可见字段视图——剔除引擎维护的仓库锚点键,并把 status/title/
/// severity 一并纳入比较(close 改状态、update 改标题都算用户变更)。锚点
/// (recorded_at/observed_head/observed_worktree_hash)由「进展」落笔时随
/// progress_anchor_fields 写入,是机械指纹而非用户意图;同值 update 不刷新它们。
pub(super) fn user_visible_fields(entry: &Entry) -> Vec<(String, String)> {
    const ANCHOR_KEYS: &[&str] = &["recorded_at", "observed_head", "observed_worktree_hash"];
    let mut visible = vec![("状态".into(), entry.status.clone())];
    visible.push(("标题".into(), entry.title.clone()));
    if let Some(sev) = &entry.severity {
        visible.push(("severity".into(), sev.clone()));
    }
    visible.extend(
        entry
            .fields
            .iter()
            .filter(|(k, _)| !ANCHOR_KEYS.contains(&k.as_str()))
            .cloned(),
    );
    visible.sort_by(|a, b| a.0.cmp(&b.0));
    visible
}

/// R-232:两条用户可见字段视图的 旧→新 差异摘要。逐字段列出变化的键,
/// 格式 `字段: 旧 → 新`,多字段用 `; ` 连接。无差异返回空串(调用方先判 no-op)。
pub(super) fn field_diff_summary(
    before: &[(String, String)],
    after: &[(String, String)],
) -> String {
    let keys = {
        let mut all = std::collections::BTreeSet::new();
        for (k, _) in before {
            all.insert(k.clone());
        }
        for (k, _) in after {
            all.insert(k.clone());
        }
        all
    };
    let mut parts = Vec::new();
    for key in keys {
        let old = before
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str());
        let new = after
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str());
        if old != new {
            parts.push(format!(
                "{key}: {} → {}",
                old.unwrap_or("∅"),
                new.unwrap_or("∅")
            ));
        }
    }
    parts.join("; ")
}

pub(super) fn render_line(e: &Entry) -> String {
    let sev = e
        .severity
        .as_ref()
        .map(|s| format!(" ({s})"))
        .unwrap_or_default();
    format!("{} [{}]{sev} {}", e.id, e.status, e.title)
}

pub(super) fn unknown_id(id: &str, entries: &[Entry]) -> String {
    let known: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    format!(
        "unknown id `{id}`; existing: {}",
        if known.is_empty() {
            "(none)".into()
        } else {
            known.join(", ")
        }
    )
}

/// 活动 entries 找不到时:区分「已归档」与「真不存在」。归档条目不是 unknown——
/// 误导 agent 以为 ID 不存在而绕过专用工具手改托管文档,会破坏原子写入与审计链
/// (D-331:reopen 对归档 ID 误报 unknown id,把 D-267 的 [dropped] [fixed] 留在归档)。
pub(super) fn archived_or_unknown(
    id: &str,
    entries: &[Entry],
    store: &DocStore,
    tool: &str,
) -> String {
    let archived = store.load_archive().unwrap_or_default();
    if archived.iter().any(|e| e.id == id) {
        format!(
            "`{id}` is archived — this action does not apply to terminal entries. \
             To correct a wrong terminal status (e.g. fixed should be wontfix), use \
             `{tool} fix_terminal id={id} status=<fixed|wontfix> reason=<why>`."
        )
    } else {
        unknown_id(id, entries)
    }
}
