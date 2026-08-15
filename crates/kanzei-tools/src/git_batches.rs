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

/// D-377:提交标题缓存,键是**解析出来的 HEAD sha**。
///
/// `git log HEAD --format=%s` 扫全history(本仓 1,527 条),实测 73~107ms,而它挂在
/// `docs_snapshot` 上——文档面板每次刷新、每轮 `kz:done`、每次勾选都要付一遍。
/// Windows 上单是 spawn 一个 git 进程就 ~45ms,所以缓存必须做到「命中时一个进程都不起」:
/// 键取自直接读 `.git/HEAD` 与它指向的 ref 文件(纯文件读,微秒级),sha 没变则历史没变。
/// 解析不出 sha(packed-refs、异常 .git 布局)时不缓存,老实起 git——宁可慢也不给旧答案。
static SUBJECTS_CACHE: std::sync::Mutex<Option<(std::path::PathBuf, String, String)>> =
    std::sync::Mutex::new(None);

/// 直接从 `.git` 读出 HEAD 指向的 sha。返回 None = 无法确定,调用方必须回落到起进程。
fn head_sha(project_root: &Path) -> Option<String> {
    let dot_git = project_root.join(".git");
    // worktree 里 .git 是文件:`gitdir: <路径>`。
    let git_dir = if dot_git.is_file() {
        let pointer = std::fs::read_to_string(&dot_git).ok()?;
        let raw = pointer.strip_prefix("gitdir:")?.trim();
        let path = std::path::Path::new(raw);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            project_root.join(path)
        }
    } else {
        dot_git
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    let Some(reference) = head.strip_prefix("ref:") else {
        // detached HEAD:内容本身就是 sha。
        return (!head.is_empty()).then(|| head.to_string());
    };
    // 松散 ref 文件存在就用它;不存在说明被打包进 packed-refs,不猜,返回 None。
    let sha = std::fs::read_to_string(git_dir.join(reference.trim())).ok()?;
    let sha = sha.trim();
    (!sha.is_empty()).then(|| sha.to_string())
}

fn commit_subjects(project_root: &Path) -> Result<String, String> {
    let head = head_sha(project_root);
    if let Some(head) = head.as_deref() {
        let cache = SUBJECTS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((root, sha, subjects)) = cache.as_ref() {
            if root == project_root && sha == head {
                return Ok(subjects.clone());
            }
        }
    }
    let mut command = Command::new("git");
    command
        .args(["log", "HEAD", "--format=%s"])
        .current_dir(project_root);
    crate::hide_console(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("无法读取 Git 提交历史: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "无法读取 Git 提交历史: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let subjects = String::from_utf8_lossy(&output.stdout).into_owned();
    if let Some(head) = head {
        *SUBJECTS_CACHE.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((project_root.to_path_buf(), head, subjects.clone()));
    }
    Ok(subjects)
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
        // 中文「批N」(如 "R-157 批3:")归入 B 命名空间:与 "R-157 B3" 语义等价,
        // 混用时自然去重。B/S 两个命名空间照旧保留(S = store 域拆分)。
        // 「批次」多为字段叙事(「批次: 3/3」),不是提交批次标记,跳过不识别。
        let (kind, number_start) = if chars[index] == '批' {
            let s = index + 1;
            if chars.get(s) == Some(&'次') {
                index = s + 1;
                continue;
            }
            ('B', s)
        } else if matches!(chars[index], 'B' | 'S') {
            (chars[index], index + 1)
        } else {
            index += 1;
            continue;
        };
        let Some((first, after_first)) = parse_number(&chars, number_start) else {
            index = number_start;
            continue;
        };
        batches.insert((kind, first));

        // 同一标题常把连续批次压缩成 `S5-S6`、`S7+S8` 或 `B1-2`(含「批1-3」)。
        // 第二个 marker 可省略也可保留,两个写法都展开到独立批次。
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
    // 不跳过前导空格:批次标记(B/S/批)必须紧邻数字才能被识别,
    // 「kanzei-tools 162」的 S+空格+162 不得被误判成 S162(D-252)。
    // 需要空格的地方调用方已先 skip_whitespace(如范围展开 S5- S6 的第二个数字)。
    let start = index;
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
    use super::{completed_batches, completed_batches_from_subjects, head_sha};

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

    #[test]
    fn parses_chinese_pi_batch_markers_without_misjudging() {
        // 当前实际提交风格:「R-157 批3:…」;「批次N」同义。归入 B 命名空间,
        // 与 "R-157 B3" 混用时自然去重(同一批只计一次)。
        let subjects = "\
R-157 批3: 设置页节奏参数透传 + 批2 接线修复\n\
R-157 批1: cadence 配置结构\n\
R-157 批2: 注入提示词参数化\n\
R-157 收口: 批次 3/3(「批」「次」是叙事词,不构成新批次)\n\
R-157 B3: 与「批3」同批,重复不重复计数\n\
R-1570 批1: 相邻 ID 不得误判\n\
R-156 批次2: 其他条目不得误判\n\
R-157 审批流程第二版: 「批」后不是数字,不构成批次\n\
普通提交: R-157 的日常改动(无批次标记)\n";
        assert_eq!(completed_batches_from_subjects(subjects, "R-157"), 3);
    }

    #[test]
    fn word_trailing_s_followed_by_number_is_not_a_batch_marker() {
        // D-252:提交标题里「kanzei-tools 162」「tools 167」「harness 64」的
        // 单词尾 S + 空格 + 数字不得被误判为 S162/S167/S64 批次。
        let subjects = "\
R-164 B1: ... kanzei-tools 162 全绿\n\
R-164 B2: ... tools 167 + harness 64 全绿\n\
R-164 B3: ... kanzei-tools 171 全绿\n\
R-164 B4: ... kanzei-tools 172 全绿\n\
chore: 测试归档同步(R-164 B1 定向测试 passed 归档)\n";
        // 只有 B1/B2/B3/B4 四个标记;S 结尾单词后的数字不再计入。
        assert_eq!(completed_batches_from_subjects(subjects, "R-164"), 4);
    }

    fn git_in(root: &std::path::Path, args: &[&str]) {
        std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap();
    }

    /// D-377:缓存的唯一风险是**给旧答案**。这里钉住失效键:新提交落地后,
    /// 同一个 project_root 必须立刻反映出来,而不是命中上一次的 subjects。
    #[test]
    fn 提交标题缓存在head变动后失效() {
        let root = std::env::temp_dir().join(format!(
            "kz-batches-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        git_in(&root, &["init", "-q"]);
        git_in(&root, &["config", "user.email", "test@example.invalid"]);
        git_in(&root, &["config", "user.name", "Kanzei Test"]);
        std::fs::write(root.join("a.txt"), "1").unwrap();
        git_in(&root, &["add", "a.txt"]);
        git_in(&root, &["commit", "-q", "-m", "R-900 B1 第一批"]);

        assert_eq!(completed_batches(&root, "R-900").unwrap(), 1);
        // 命中缓存:同一 HEAD 重复问,答案一致。
        assert_eq!(completed_batches(&root, "R-900").unwrap(), 1);

        std::fs::write(root.join("a.txt"), "2").unwrap();
        git_in(&root, &["add", "a.txt"]);
        git_in(&root, &["commit", "-q", "-m", "R-900 B2 第二批"]);
        assert_eq!(
            completed_batches(&root, "R-900").unwrap(),
            2,
            "HEAD 变了却还在返回旧的提交标题:缓存失效键失灵(D-377)"
        );

        // head_sha 必须真的解析得出来,否则缓存全程不生效、判据也就成了摆设。
        assert!(
            head_sha(&root).is_some(),
            "松散 ref 布局下 head_sha 应解析成功"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
