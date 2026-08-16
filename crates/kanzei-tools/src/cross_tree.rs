//! 跨树越界保护(R-186):bash 围栏从「托管文档」扩到「不属于本线的 worktree」。
//!
//! 并行下真正要防的不是恶意,是串台——A 线的命令跑进 B 线的树、把人家**未提交**
//! 的活覆盖了。命令语法闸门防不住这个(`cd ../other && rm -rf` 里没有一个可疑
//! token,`cargo` 也是合法程序、其 build.rs 能干任何事),所以这里沿用 D-173/D-174
//! 的**结果侧**判定:动作之前拍下其它线工作树的镜像,动作之后再比一次,改了就
//! 隔离留证 + 整体回滚。
//!
//! 与托管文档围栏的关系:托管文档(`.kanzei/project`、`.kanzei/memory`)仍由
//! [`crate::managed`] 单独保护(既有行为一字不改);本模块只新增「其它线工作树」
//! 这一保护面,两者的报告与回滚互不干扰。
//!
//! # 快照策略(内容④:mtime 级粗筛、命中再细查)
//!
//! 执行前拍下其它线树的**全文件内容**(受限镜像),执行后**只按 mtime/len 粗筛**——
//! 绝大多数文件在命令窗口内不会被碰,mtime 没变就不用重新读内容;只有 mtime/len
//! 变化的文件才读内容细查。变化且内容不同 → 隔离 + 写回执行前内容;变化但内容
//! 相同(`touch` 之类)→ 吸收进基线,不算越界。
//!
//! # 性能与安全边界
//!
//! - 单文件镜像上限 4 MiB、单树文件数上限 2000(与托管文档同口径):超限时该文件
//!   只记指纹不存内容,能检测但无法回滚,报告如实说明;
//! - 非 git 仓库 / `git worktree list` 失败:返回空快照,**不阻塞 bash**(不是所有
//!   bash 都跑在 git 仓里,更没有其它线可保护——此时保护面就是空集);
//! - `.git/` 目录自身不入镜像(git 内部状态由 git 自己管,误回滚它会破坏仓库);
//! - 本线的树由调用方给出(`self_tree`,主树进程传主根),git 清单里排除它。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 单文件镜像上限:超过就只记指纹,能检测但无法回滚(会如实说明)。
const OTHER_TREE_FILE_LIMIT: u64 = 4 * 1024 * 1024;
/// 单树镜像文件数上限,防止把一整棵大树的文件都读进内存拖垮每次 bash。
const OTHER_TREE_MAX_FILES: usize = 2000;

/// 单个被保护文件的镜像三态(D-396 三态口径;D-397 加粗筛指纹):
#[derive(Debug, Clone, PartialEq)]
enum FileImage {
    /// 动作前存在,内容已镜像(≤4MiB),可逐字节回滚。len+mtime 是 D-397 粗筛指纹
    /// (执行后扫描只采指纹,命中才读内容比对,不每条 bash 全量读)。
    Content {
        bytes: Vec<u8>,
        len: u64,
        mtime_ms: u128,
    },
    /// 动作前存在但超限(或读取失败)——只记 len+mtime 指纹,改动可检出但无法回滚;
    /// 回滚时保持现状并如实报告,绝不当作「不存在」删除(D-396)。
    Fingerprint { len: u64, mtime_ms: u128 },
    /// 动作前不存在。
    Absent,
}

impl FileImage {
    /// D-397:粗筛指纹——(len, mtime) 都相同视为「未变」;任一变化命中,
    /// Content 读内容二次确认(touch 只改 mtime 会命中但内容相同→不算越界),
    /// Fingerprint 无内容可比对,命中即判改动。
    fn matches_fingerprint(&self, len: u64, mtime_ms: u128) -> bool {
        match self {
            FileImage::Content {
                len: before_len,
                mtime_ms: before_mtime,
                ..
            } => *before_len == len && *before_mtime == mtime_ms,
            FileImage::Fingerprint {
                len: before_len,
                mtime_ms: before_mtime,
            } => *before_len == len && *before_mtime == mtime_ms,
            FileImage::Absent => false,
        }
    }
}

/// 其它线工作树的执行前镜像。
///
/// 外层 key = 规范化后的树根绝对路径;内层 key = 相对该树根的路径(含子目录,
/// 统一 `/` 分隔);value = 文件内容镜像(超限为 None)。目录不入 map。
#[derive(Debug, Default, Clone)]
pub(crate) struct OtherTreesSnapshot {
    trees: BTreeMap<PathBuf, BTreeMap<String, FileImage>>,
    /// D-397:快照是否因文件数上限截断(保护面不完整,对账时显式报告)。
    truncated: bool,
}

impl OtherTreesSnapshot {
    pub(crate) fn is_empty(&self) -> bool {
        self.trees.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn contains_tree(&self, root: &Path) -> bool {
        self.trees.contains_key(root)
    }

    #[cfg(test)]
    pub(crate) fn file_count(&self) -> usize {
        self.trees.values().map(|files| files.len()).sum()
    }
}

/// 拍下 `project_root` 所在仓库里、除 `self_tree` 之外的全部工作树镜像。
///
/// `self_tree` 传 bash 实际工作的那棵树(通常即 `ctx.cwd` 规范化的树根);git 清单
/// 里与它同路径的树是本线自己的树,不在保护面内。`self_tree` 规范化失败(目录
/// 不存在等)时按「本线 = 主树」处理——主树是 git 清单里的第一项,保护其它树。
pub(crate) fn capture_other_trees(
    project_root: &Path,
    self_tree: &Path,
) -> Result<OtherTreesSnapshot, String> {
    let entries = crate::worktree::git_worktrees(project_root)?;
    let self_key = crate::worktree::worktree_key(self_tree);
    let mut snapshot = OtherTreesSnapshot::default();
    let mut truncated = false;
    for entry in entries {
        if entry.bare {
            continue;
        }
        if crate::worktree::worktree_key(&entry.path) == self_key {
            continue; // 本线自己的树。
        }
        let mut files = BTreeMap::new();
        truncated |= collect_tree_files(&entry.path, &mut files);
        if !files.is_empty() {
            snapshot.trees.insert(entry.path.clone(), files);
        }
    }
    snapshot.truncated = truncated;
    Ok(snapshot)
}

/// 递归收集树内文件镜像。返回 true = 达到文件数上限被截断(保护面不完整,
/// D-397 显式报告,不再静默)。
fn collect_tree_files(root: &Path, files: &mut BTreeMap<String, FileImage>) -> bool {
    if files.len() >= OTHER_TREE_MAX_FILES {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    let mut truncated = false;
    for entry in entries.flatten() {
        if files.len() >= OTHER_TREE_MAX_FILES {
            return true;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // git 内部状态不入镜像:误回滚 .git 会直接破坏仓库完整性。
        // 注意主树的 .git 是目录,worktree 的 .git 是**文件**(gitdir: 指针),
        // 两种形态都要跳过。
        let is_git_dir = path.file_name().and_then(|name| name.to_str()) == Some(".git");
        if is_git_dir {
            continue;
        }
        if file_type.is_dir() {
            truncated |= collect_tree_files(&path, files);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let key = relative.display().to_string().replace('\\', "/");
        // D-396:三态镜像——≤4MiB 读内容;超限/读取失败只记 len+mtime 指纹。
        // D-397:Content 也带 len+mtime 指纹(执行后粗筛用)。
        let metadata = entry.metadata().ok();
        let size = metadata.as_ref().map(|m| m.len()).unwrap_or(u64::MAX);
        let mtime_ms = metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let image = if size <= OTHER_TREE_FILE_LIMIT {
            match std::fs::read(&path) {
                Ok(bytes) => FileImage::Content {
                    len: size,
                    mtime_ms,
                    bytes,
                },
                Err(_) => FileImage::Fingerprint {
                    len: size,
                    mtime_ms,
                },
            }
        } else {
            FileImage::Fingerprint {
                len: size,
                mtime_ms,
            }
        };
        files.insert(key, image);
    }
    truncated
}

/// D-397:执行后对账的指纹粗筛——只采 (len, mtime),不读文件内容。
/// 返回 (指纹表, 是否截断)。与 collect_tree_files 同构(.git 跳过、递归、
/// 2000 上限),但零内容读取:真仓(含 target/node_modules 未跟踪大目录)下
/// 每条 bash 的扫描开销从「全量读内容」降为「全量 stat」。
fn collect_tree_metadata(root: &Path, files: &mut BTreeMap<String, (u64, u128)>) -> bool {
    if files.len() >= OTHER_TREE_MAX_FILES {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    let mut truncated = false;
    for entry in entries.flatten() {
        if files.len() >= OTHER_TREE_MAX_FILES {
            return true;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let is_git_dir = path.file_name().and_then(|name| name.to_str()) == Some(".git");
        if is_git_dir {
            continue;
        }
        if file_type.is_dir() {
            truncated |= collect_tree_metadata(&path, files);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let key = relative.display().to_string().replace('\\', "/");
        let metadata = entry.metadata().ok();
        let size = metadata.as_ref().map(|m| m.len()).unwrap_or(u64::MAX);
        let mtime_ms = metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        files.insert(key, (size, mtime_ms));
    }
    truncated
}

/// 执行后对账:重扫其它线树,比对执行前镜像。
///
/// 有差异 → 隔离留证 + 回滚到执行前内容,并返回回喂模型的报告;
/// 无差异 → 返回 None。**非 git 仓库 / git 失败 → 返回 None 不阻塞**
/// (没有其它线树可保护,保护面为空集,与执行前的判定一致)。
///
/// `before` 为空快照时同样直接返回 None:执行前就拍不到任何树,执行后也拍不到,
/// 无从对账——这时静默放行比编一个「保护了一切」的假报告诚实。
///
/// `owner_run` / `owner_process` 是执行这条命令的线身份(R-186 验收②归因):
/// 越界报告必须点名是哪条线越的界,轨迹(ToolOutput error)里据此可审计。
pub(crate) fn enforce_other_trees(
    project_root: &Path,
    self_tree: &Path,
    before: &OtherTreesSnapshot,
    owner_run: Option<&str>,
    owner_process: Option<&str>,
) -> Option<String> {
    // D-397:执行后对账按 before 的树目录直接扫描(不再重新枚举 git worktrees,
    // 也不需排除本线),self_tree 参数保留仅为兼容调用方签名。
    let _ = self_tree;
    if before.is_empty() {
        return None;
    }
    // D-397:执行后对账只采 (len, mtime) 指纹(全量 stat,零内容读取)——真仓
    // (含 target/node_modules 未跟踪大目录)下每条 bash 的扫描开销不再翻倍全量读。
    let mut after_meta: BTreeMap<PathBuf, BTreeMap<String, (u64, u128)>> = BTreeMap::new();
    let mut after_truncated = false;
    for root in before.trees.keys() {
        let mut meta = BTreeMap::new();
        after_truncated |= collect_tree_metadata(root, &mut meta);
        after_meta.insert(root.clone(), meta);
    }
    // 执行前存在、执行后目录消失的树:整棵树被删(rm -rf 目标树)是最严重的越界,
    // 必须点名。此时无法回滚整棵树(镜像里只有文件内容没有目录语义),如实说明。
    let removed_trees = before
        .trees
        .keys()
        .filter(|root| !root.exists())
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>();
    let mut touched = Vec::new();
    let mut changes: BTreeMap<PathBuf, BTreeMap<String, (FileImage, Option<FileImage>)>> =
        BTreeMap::new();
    for (root, before_files) in &before.trees {
        let after_files = after_meta.get(root).cloned().unwrap_or_default();
        let mut tree_changes = BTreeMap::new();
        for (rel, before_image) in before_files {
            match after_files.get(rel) {
                // 执行后不存在:被删。回滚 = 写回执行前内容(超限无法回滚,保持删除并报告)。
                None => {
                    tree_changes.insert(rel.clone(), (before_image.clone(), None));
                }
                // 指纹粗筛命中(执行后与执行前 len/mtime 不同):Content 读内容
                // 二次确认(touch 只改 mtime 内容相同 → 不算越界);Fingerprint 判改动。
                Some((len, mtime)) if !before_image.matches_fingerprint(*len, *mtime) => {
                    let after_image = match before_image {
                        FileImage::Content { bytes, .. } => match std::fs::read(root.join(rel)) {
                            Ok(after_bytes) if after_bytes == *bytes => continue,
                            Ok(after_bytes) => Some(FileImage::Content {
                                len: *len,
                                mtime_ms: *mtime,
                                bytes: after_bytes,
                            }),
                            Err(_) => Some(FileImage::Fingerprint {
                                len: *len,
                                mtime_ms: *mtime,
                            }),
                        },
                        FileImage::Fingerprint { .. } => Some(FileImage::Fingerprint {
                            len: *len,
                            mtime_ms: *mtime,
                        }),
                        FileImage::Absent => unreachable!("before_files 只含存在文件"),
                    };
                    tree_changes.insert(rel.clone(), (before_image.clone(), after_image));
                }
                Some(_) => {} // 指纹相同:无变化(零内容读取)。
            }
        }
        // 执行后新增的文件(执行前没有):越界新建,回滚 = 删除。
        for (rel, (len, mtime)) in &after_files {
            if !before_files.contains_key(rel) {
                tree_changes.insert(
                    rel.clone(),
                    (
                        FileImage::Absent,
                        Some(FileImage::Fingerprint {
                            len: *len,
                            mtime_ms: *mtime,
                        }),
                    ),
                );
            }
        }
        if !tree_changes.is_empty() {
            changes.insert(root.clone(), tree_changes);
        }
    }
    if changes.is_empty() && removed_trees.is_empty() {
        return None;
    }

    let owner_line = match (owner_run, owner_process) {
        (Some(run), Some(proc)) => format!(
            "  attributed to owner run: {run} (process {proc}) — this command crossed into \
             another line's worktree"
        ),
        (Some(run), None) => {
            format!("  attributed to owner run: {run} — this command crossed into another line's worktree")
        }
        (None, Some(proc)) => format!(
            "  attributed to process: {proc} — this command crossed into another line's worktree"
        ),
        (None, None) => {
            "  attributed to: unknown owner (no run/process identity bound)".to_string()
        }
    };
    let mut lines = vec![
        "[cross-tree] BLOCKED AND ROLLED BACK. This command modified files in another line's \
         worktree — those trees are protected (R-186): another line's uncommitted work must \
         not be overwritten, no matter which mechanism is used (redirect, cp/mv, cargo build.rs, \
         python/node one-liner, git checkout of a single file)."
            .to_string(),
        owner_line,
    ];
    let mut restored = 0usize;
    let mut unrestored: Vec<String> = Vec::new();
    let mut quota = std::collections::BTreeMap::new();
    for (root, tree_changes) in &changes {
        let root_display = root.display().to_string();
        let quarantine = project_root.join(".kanzei/quarantine").join(format!(
            "cross-tree-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or_default()
        ));
        for (rel, (before_image, after_image)) in tree_changes {
            let absolute = root.join(rel);
            // 隔离留证:改后版本(有内容镜像的)一份不丢,可原样取回;
            // 超限改动体积过大不隔离,如实报告。
            if let Some(FileImage::Content {
                bytes: after_content,
                ..
            }) = after_image
            {
                let saved = quarantine.join(rel);
                if let Some(parent) = saved.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&saved, after_content);
            }
            match before_image {
                FileImage::Content {
                    bytes: original, ..
                } => {
                    // 执行前存在且有内容镜像:写回原内容(逐字节复原)。
                    if std::fs::write(&absolute, original).is_ok() {
                        restored += 1;
                    }
                }
                FileImage::Fingerprint { .. } => {
                    // D-396:执行前存在但超限——原内容无法回滚,保持现状,绝不删除;
                    // 改动已由 len+mtime 指纹检出,点名报告。
                    unrestored.push(format!("{root_display}/{rel}"));
                }
                FileImage::Absent => {
                    // 执行前不存在:删除这个新建文件。
                    if std::fs::remove_file(&absolute).is_ok() {
                        restored += 1;
                    }
                }
            }
            touched.push(format!("{root_display}/{rel}"));
        }
        // 每棵树只留一个隔离目录,记录到该树的报告行。
        quota.insert(root_display.clone(), quarantine.display().to_string());
    }
    for (root_display, quarantine) in &quota {
        lines.push(format!("  · {root_display}: 隔离留证于 {quarantine}"));
    }
    if before.truncated || after_truncated {
        lines.push(format!(
            "  · WARNING: 快照文件数达上限 {OTHER_TREE_MAX_FILES},保护面不完整——\
             截断显式报告(D-397),超出部分不在保护面内。"
        ));
    }
    if !removed_trees.is_empty() {
        lines.push(format!(
            "  · 整树消失(执行前存在、执行后不可见): {}. 镜像只含文件内容,无法重建目录 \
             — 请从 git 或备份恢复。",
            removed_trees.join(", ")
        ));
    }
    if !unrestored.is_empty() {
        lines.push(format!(
            "  · 超限文件(>{other_limit} 字节)改动已检出但无法回滚,保持现状(不删除): {list}",
            other_limit = OTHER_TREE_FILE_LIMIT,
            list = unrestored.join(", ")
        ));
    }
    lines.push(format!(
        "  touched: {} (共 {restored} 个文件已恢复执行前内容)",
        touched.join(", ")
    ));
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique(tag: &str) -> String {
        format!(
            "{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git 执行失败");
        assert!(
            output.status.success(),
            "git {} 失败: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn git_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(unique(tag));
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "test@kanzei.dev"]);
        git(&dir, &["config", "user.name", "kanzei test"]);
        std::fs::write(dir.join("seed.txt"), "seed\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-qm", "seed"]);
        dir
    }

    /// 建一棵 worktree 并 checkout 到新分支。
    fn add_worktree(root: &Path, name: &str) -> PathBuf {
        let (path, branch) = crate::worktree::worktree_target(root, name).unwrap();
        git(root, &["branch", &branch, "HEAD"]);
        git(
            root,
            &[
                "worktree",
                "add",
                &crate::worktree::git_arg_path(&path),
                &branch,
            ],
        );
        path
    }

    /// A 线(主树)的 bash 写 B 线工作树 → 检出、隔离、回滚,B 线逐字节复原。
    #[test]
    fn a线bash写b线树_检出隔离回滚_b线逐字节复原() {
        let root = git_repo("kz-ct-b1");
        let b = add_worktree(&root, "line-b");
        // B 线有未提交的活。
        std::fs::write(b.join("seed.txt"), "B 线的未提交修改\n").unwrap();
        std::fs::write(b.join("new-file.txt"), "B 线新建\n").unwrap();

        // A 线(主树)执行前拍快照,保护面应含 B 树、不含主树自己。
        let before = capture_other_trees(&root, &root).expect("快照失败");
        assert!(before.contains_tree(&b), "B 线工作树必须在保护面内");
        assert!(!before.contains_tree(&root), "本线(主树)自己不能进保护面");
        assert_eq!(before.file_count(), 2, "B 树两个文件都应镜像");

        // A 线的命令越界改了 B 树(模拟 cd <B线树> && 写操作)。
        std::fs::write(b.join("seed.txt"), "A 线覆盖!\n").unwrap();
        std::fs::write(b.join("new-file.txt"), "A 线追加\n").unwrap();

        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"))
            .expect("必须检出越界");
        assert!(
            report.contains("seed.txt") && report.contains("new-file.txt"),
            "报告要点名被改文件: {report}"
        );
        assert!(
            report.contains("run-a") && report.contains("proc-a"),
            "归因必须点名 owner run/process(验收②): {report}"
        );
        assert_eq!(
            std::fs::read_to_string(b.join("seed.txt")).unwrap(),
            "B 线的未提交修改\n",
            "B 线未提交修改必须逐字节复原"
        );
        assert_eq!(
            std::fs::read_to_string(b.join("new-file.txt")).unwrap(),
            "B 线新建\n",
            "B 线新建文件必须逐字节复原"
        );
        assert!(!b.join(".kanzei").is_dir()); // 无托管根,隔离目录在主根。
        assert!(
            root.join(".kanzei/quarantine").is_dir(),
            "隔离留证目录必须存在"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A 线在 worktree 里跑 bash:保护面 = 主树 + 其它 worktree,排除自己那棵。
    #[test]
    fn worktree线_保护面含主树与其它树_排除自身() {
        let root = git_repo("kz-ct-b2");
        let a = add_worktree(&root, "line-a");
        let b = add_worktree(&root, "line-b");
        std::fs::write(b.join("b-only.txt"), "b\n").unwrap();

        let before = capture_other_trees(&root, &a).expect("快照失败");
        assert!(
            before.contains_tree(&root),
            "主树必须在保护面内(从 worktree 视角)"
        );
        assert!(before.contains_tree(&b), "其它 worktree 必须在保护面内");
        assert!(!before.contains_tree(&a), "本线 worktree 不能进保护面");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 非 git 目录:快照为空、enforce 放行,不阻塞 bash。
    #[test]
    fn 非git目录_空保护面_不阻塞() {
        let dir = std::env::temp_dir().join(unique("kz-ct-nogit"));
        std::fs::create_dir_all(&dir).unwrap();
        let before = capture_other_trees(&dir, &dir);
        assert!(before.is_err() || before.unwrap().is_empty());
        // 没有树可保护:enforce 直接 None,不报假成功。
        let before = capture_other_trees(&dir, &dir).unwrap_or_default();
        assert!(enforce_other_trees(&dir, &dir, &before, None, None).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 命令窗口内 mtime 变但内容相同(touch):不算越界。
    #[test]
    fn touch文件_内容不变_不越界() {
        let root = git_repo("kz-ct-b3");
        let b = add_worktree(&root, "line-b");
        std::fs::write(b.join("seed.txt"), "未提交内容\n").unwrap();

        let before = capture_other_trees(&root, &root).expect("快照失败");
        // 等一个时钟 tick,保证 mtime 真的变化(touch 同秒内可能 mtime 相同)。
        std::thread::sleep(std::time::Duration::from_millis(20));
        // 触一下 mtime,内容不动。
        let file = b.join("seed.txt");
        let _ = std::fs::File::options().write(true).open(&file).unwrap();
        // 只 open 不改写,内容不变;再确保 mtime 前进。
        std::fs::write(&file, "未提交内容\n").unwrap();

        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"));
        assert!(
            report.is_none(),
            "内容相同的 touch 不应算越界,实得: {report:?}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "未提交内容\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 越界**新建**文件(执行前不存在):被删除回滚。
    #[test]
    fn 越界新建文件_被删除回滚() {
        let root = git_repo("kz-ct-b4");
        let b = add_worktree(&root, "line-b");

        let before = capture_other_trees(&root, &root).expect("快照失败");
        std::fs::write(b.join("intruder.txt"), "A 线塞进来的\n").unwrap();

        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"))
            .expect("必须检出");
        assert!(report.contains("intruder.txt"), "{report}");
        assert!(!b.join("intruder.txt").exists(), "新建越界文件必须被删");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 验收③(本条相对命令闸门的核心优势):**`cargo run` 里 build.rs 写别人的树**
    /// 同样被抓。命令语法闸门对这条完全无效——`cargo` 是合法程序、命令文本里没有
    /// 任何可疑 token;只有结果侧快照对比能发现「命令窗口内别的线工作树变了」。
    ///
    /// 构造:主树 A 里放一个 cargo 项目,build.rs 在构建期写 B 线树文件;跑
    /// `cargo build`(与 cargo run 共享同一构建期,build.rs 在两者中都会执行),
    /// 再对账——必须检出越界并回滚,新建的 victim 文件被删。
    #[test]
    fn cargo_build的build_rs写b线树_同样被抓() {
        let root = git_repo("kz-ct-cargo");
        let b = add_worktree(&root, "line-b");
        std::fs::write(b.join("b-own.txt"), "B 线自己的活\n").unwrap();

        // 主树 A 里造一个最小 cargo 项目,无网络依赖,本地冷编译几秒内完成。
        let proj = root.join("cargo-proj");
        std::fs::create_dir_all(proj.join("src")).unwrap();
        std::fs::write(
            proj.join("Cargo.toml"),
            "[package]\nname = \"buildrs-cross\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(proj.join("src/main.rs"), "fn main() {}\n").unwrap();
        // build.rs 用绝对路径写 B 线树(temp 路径不含引号,raw string 安全)。
        let b_path = b.display().to_string();
        std::fs::write(
            proj.join("build.rs"),
            format!(
                "fn main() {{ std::fs::write(r#\"{b_path}/victim.txt\"#, \"build.rs 越界写入\\n\").unwrap(); }}\n"
            ),
        )
        .unwrap();

        // A 线(主树)执行前拍快照,保护面应含 B 树。
        let before = capture_other_trees(&root, &root).expect("快照失败");
        assert!(before.contains_tree(&b), "B 线工作树必须在保护面内");

        // 跑 cargo build 触发 build.rs(命令窗口 = build 全程)。
        let build = std::process::Command::new("cargo")
            .arg("build")
            .arg("--quiet")
            .current_dir(&proj)
            .output()
            .expect("cargo 启动失败");
        assert!(
            build.status.success(),
            "cargo build 应成功: {}",
            String::from_utf8_lossy(&build.stderr)
        );

        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"))
            .expect("build.rs 写别的树必须被抓");
        assert!(
            report.contains("victim.txt"),
            "报告必须点名 build.rs 写入的文件: {report}"
        );
        assert!(
            !b.join("victim.txt").exists(),
            "build.rs 越界新建必须被回滚删除"
        );
        assert_eq!(
            std::fs::read_to_string(b.join("b-own.txt")).unwrap(),
            "B 线自己的活\n",
            "B 线自己的未提交活必须逐字节保留"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 验收⑤性能 + D-397 真仓规模实测:快照开销随其它线树数量增长,给出实测
    /// 数字并断言上界(不随 N 线性劣化到不可用)。
    ///
    /// 构造:git 仓 + 5 棵 worktree,每棵塞 300 个小文件(≈1500 文件,接近
    /// target/node_modules 未跟踪大目录量级)。量两个阶段耗时:
    /// 执行前 `capture_other_trees`(读内容,回滚需要)与执行后
    /// `collect_tree_metadata`(粗筛,只 stat 不读内容——D-397 核心:真仓规模下
    /// 每条 bash 的执行后扫描不翻倍全量读)。上界:读内容 4s、粗筛 2s。
    #[test]
    fn 快照性能_多线树耗时上界() {
        let root = git_repo("kz-ct-perf");
        let mut trees = Vec::new();
        for i in 0..5 {
            let t = add_worktree(&root, &format!("line-{i}"));
            for f in 0..300 {
                std::fs::write(
                    t.join(format!("f-{f:03}.txt")),
                    format!("content {f} with payload padding padding padding\n"),
                )
                .unwrap();
            }
            trees.push(t);
        }
        let started = std::time::Instant::now();
        let before = capture_other_trees(&root, &root).expect("快照失败");
        let capture_elapsed = started.elapsed();
        assert_eq!(
            before.file_count(),
            5 * 301,
            "5 棵树 × (300 个 f-文件 + seed commit 的 seed.txt) 应全部镜像"
        );
        assert!(
            capture_elapsed < std::time::Duration::from_secs(4),
            "执行前快照(读内容)耗时 {capture_elapsed:?} 超 4s 上界"
        );
        // 执行后粗筛:只 stat 不读内容(D-397)。
        let started_meta = std::time::Instant::now();
        let mut meta = BTreeMap::new();
        let mut truncated = false;
        for t in &trees {
            truncated |= collect_tree_metadata(t, &mut meta);
        }
        let meta_elapsed = started_meta.elapsed();
        assert!(!truncated, "1500 文件不应触发截断");
        assert!(
            meta_elapsed < std::time::Duration::from_secs(2),
            "执行后粗筛(只 stat)耗时 {meta_elapsed:?} 超 2s 上界"
        );
        eprintln!(
            "[cross-tree perf] 5 worktrees × 300 files: capture(读内容) {capture_elapsed:?}, \
             after-metadata(粗筛) {meta_elapsed:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// D-397:文件数超上限(2000)截断——保护面不完整必须显式报告,不再静默。
    #[test]
    fn 快照截断_显式报告保护面不完整() {
        let root = git_repo("kz-ct-trunc");
        let b = add_worktree(&root, "line-b");
        // 超过 OTHER_TREE_MAX_FILES(2000)个文件。
        for f in 0..(OTHER_TREE_MAX_FILES + 1) {
            std::fs::write(b.join(format!("f-{f:04}.txt")), format!("{f}\n")).unwrap();
        }
        let before = capture_other_trees(&root, &root).expect("快照失败");
        assert!(before.truncated, "超过 2000 文件必须标记截断(不再静默)");
        // 改动已镜像的文件 → 报告必须点名截断警告,回滚照常。
        std::fs::write(b.join("f-0000.txt"), "CHANGED\n").unwrap();
        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"))
            .expect("必须检出越界");
        assert!(
            report.contains("截断") && report.contains("不完整"),
            "截断显式报告: {report}"
        );
        assert_eq!(
            std::fs::read_to_string(b.join("f-0000.txt")).unwrap(),
            "0\n",
            "回滚照常"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// D-396:超限文件(>4MiB)改动——指纹检出,回滚保持现状(不当作新建删除),
    /// 小文件照常逐字节回滚,报告点名超限。
    #[test]
    fn 超限文件改动_检出并保持现状不删除() {
        let root = git_repo("kz-ct-overlimit");
        let b = add_worktree(&root, "line-b");
        let big = vec![0x42u8; OTHER_TREE_FILE_LIMIT as usize + 1];
        std::fs::write(b.join("big.bin"), &big).unwrap();
        std::fs::write(b.join("small.txt"), "small\n").unwrap();

        let before = capture_other_trees(&root, &root).unwrap();
        let bfiles = &before.trees[&b];
        assert!(
            matches!(bfiles["big.bin"], FileImage::Fingerprint { .. }),
            "超限文件应以指纹入镜像(不占内存)"
        );
        assert!(
            matches!(bfiles["small.txt"], FileImage::Content { .. }),
            "小文件以内容入镜像"
        );

        // 越界:改超限文件内容 + 改小文件。
        let mut big2 = big.clone();
        big2[0] = 0x41;
        std::fs::write(b.join("big.bin"), &big2).unwrap();
        std::fs::write(b.join("small.txt"), "CHANGED\n").unwrap();

        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"))
            .expect("必须检出越界");
        // 超限文件保持现状(改动后内容,未被删除);小文件回滚原内容。
        let after_big = std::fs::read(b.join("big.bin")).unwrap();
        assert_eq!(after_big, big2, "超限文件改动无法回滚,保持现状");
        assert_eq!(
            std::fs::read_to_string(b.join("small.txt")).unwrap(),
            "small\n",
            "小文件逐字节回滚"
        );
        assert!(report.contains("超限"), "报告必须点名超限: {report}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// D-396:超限文件被删除——检出但无法回滚(保持删除),报告如实说明,
    /// 不编「已恢复」。
    #[test]
    fn 超限文件被删_检出并如实报告() {
        let root = git_repo("kz-ct-overdel");
        let b = add_worktree(&root, "line-b");
        let big = vec![0x42u8; OTHER_TREE_FILE_LIMIT as usize + 1];
        std::fs::write(b.join("big.bin"), &big).unwrap();
        let before = capture_other_trees(&root, &root).unwrap();

        // 越界:删除超限文件。
        std::fs::remove_file(b.join("big.bin")).unwrap();
        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"))
            .expect("必须检出越界");
        assert!(
            !b.join("big.bin").exists(),
            "超限文件被删无法回滚原内容(保持删除),如实说明"
        );
        assert!(report.contains("超限"), "报告必须点名超限: {report}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
