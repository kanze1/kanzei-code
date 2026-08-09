//! 从当前 Git 历史推导追踪条目的已完成批次。
//!
//! 批次提交标题是提交当下产生的事实；`requirements.md` 里的 `批次` 是展示性
//! 副本，不能再作为唯一进度来源。这里故意只认 `R-123 B4` / `R-123 S5-S6`
//! 这种明确标记，普通提交和相邻 ID 都不会被误计。

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// 读取当前 `HEAD` 可达的提交标题，返回某条目的去重批次数。
///
/// `B` 与 `S` 是两个批次命名空间：`B1` 和 `S1` 是两批。返回错误时调用方应
/// 回落到文档字段，避免非 Git 目录或临时 Git 故障让文档列表不可用。
pub fn completed_batches(project_root: &Path, entry_id: &str) -> Result<u32, String> {
    let subjects = commit_subjects(project_root)?;
    Ok(completed_batches_from_subjects(&subjects, entry_id))
}

/// 一次 Git 调用为一份文档快照里的多个条目推导批次，避免每行都启动 Git 进程。
pub fn completed_batches_for_entries(
    project_root: &Path,
    entry_ids: impl IntoIterator<Item = String>,
) -> Result<std::collections::BTreeMap<String, u32>, String> {
    let subjects = commit_subjects(project_root)?;
    Ok(entry_ids
        .into_iter()
        .filter_map(|id| {
            let done = completed_batches_from_subjects(&subjects, &id);
            (done > 0).then_some((id, done))
        })
        .collect())
}

fn commit_subjects(project_root: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["log", "HEAD", "--format=%s"])
        .current_dir(project_root)
        .output()
        .map_err(|error| format!("无法读取 Git 提交历史: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "无法读取 Git 提交历史: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 纯解析入口，供单测覆盖混编、乱序与误匹配边界。
pub fn completed_batches_from_subjects(subjects: &str, entry_id: &str) -> u32 {
    let target = entry_id.to_ascii_uppercase();
    let mut batches = BTreeSet::new();
    for subject in subjects.lines() {
        let upper = subject.to_ascii_uppercase();
        if !contains_entry_id(&upper, &target) {
            continue;
        }
        collect_marked_batches(&upper, &mut batches);
    }
    batches.len() as u32
}

fn contains_entry_id(subject: &str, entry_id: &str) -> bool {
    let mut offset = 0;
    while let Some(found) = subject[offset..].find(entry_id) {
        let start = offset + found;
        let end = start + entry_id.len();
        let before = subject[..start].chars().next_back();
        let after = subject[end..].chars().next();
        if !before.is_some_and(is_id_char) && !after.is_some_and(is_id_char) {
            return true;
        }
        offset = end;
    }
    false
}

fn is_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

fn collect_marked_batches(subject: &str, batches: &mut BTreeSet<(char, u32)>) {
    let chars: Vec<char> = subject.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let kind = chars[index];
        if !matches!(kind, 'B' | 'S') {
            index += 1;
            continue;
        }
        let Some((first, after_first)) = parse_number(&chars, index + 1) else {
            index += 1;
            continue;
        };
        batches.insert((kind, first));

        // 同一标题常把连续批次压缩成 `S5-S6` 或 `S7+S8`。第二个
        // marker 可省略（`B1-2`）也可保留，两个写法都展开到独立批次。
        let mut cursor = skip_whitespace(&chars, after_first);
        if matches!(chars.get(cursor), Some('-' | '–' | '~' | '+')) {
            cursor = skip_whitespace(&chars, cursor + 1);
            if chars.get(cursor) == Some(&kind) {
                cursor = skip_whitespace(&chars, cursor + 1);
            }
            if let Some((last, after_last)) = parse_number(&chars, cursor) {
                for number in first.min(last)..=first.max(last) {
                    batches.insert((kind, number));
                }
                index = after_last;
                continue;
            }
        }
        index = after_first;
    }
}

fn skip_whitespace(chars: &[char], mut index: usize) -> usize {
    while chars.get(index).is_some_and(|c| c.is_whitespace()) {
        index += 1;
    }
    index
}

fn parse_number(chars: &[char], index: usize) -> Option<(u32, usize)> {
    let start = skip_whitespace(chars, index);
    let mut end = start;
    while chars.get(end).is_some_and(|c| c.is_ascii_digit()) {
        end += 1;
    }
    if end == start {
        return None;
    }
    let number = chars[start..end]
        .iter()
        .collect::<String>()
        .parse::<u32>()
        .ok()?;
    Some((number, end))
}

#[cfg(test)]
mod tests {
    use super::completed_batches_from_subjects;

    #[test]
    fn parses_mixed_out_of_order_and_compact_batch_markers() {
        let subjects = "\
R-155 S7+S8: 收口\n\
R-155 B2: 指标域\n\
普通提交: 不属于任何条目\n\
R-155 store 域 S5-S6: 拆分\n\
R-155 B1-2: 早期批次（重复不重复计数）\n\
R-155 B8: 驱动域\n\
R-155 关闭: 批次 16/16\n\
R-1550 B9: 相邻 ID 不得误判\n\
R-154 B9: 其他条目不得误判\n";
        assert_eq!(completed_batches_from_subjects(subjects, "R-155"), 7);
    }
}
