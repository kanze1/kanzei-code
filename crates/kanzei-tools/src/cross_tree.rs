//! 跨树越界保护(R-186):bash 围栏从「托管文档」扩到「不属于本线的 worktree」。
//!
//! 并行下真正要防的不是恶意,是串台——A 线的命令跑进 B 线的树、把人家**未提交**
//! 的活覆盖了。命令语法闸门防不住这个(`cd ../other && rm -rf` 里没有一个可疑
//! token,`cargo` 也是合法程序、其 build.rs 能干任何事),所以这里沿用 D-173/D-174
//! 的**结果侧**判定:动作之前拍下其它线工作树的镜像,动作之后再比一次。
//!
//! 与托管文档围栏的关系:托管文档(`.kanzei/project`、`.kanzei/memory`)仍由
//! [`crate::managed`] 单独保护(既有行为一字不改);本模块只新增「其它线工作树」
//! 这一保护面,两者的报告互不干扰。
//!
//! # 现行姿态:检测 + 归因 + 隔离留证,**不自动回滚**(D-407)
//!
//! 本模块判不出「A 线越界写了 B 树」与「B 线在自己树里正常干活」的区别(D-395),
//! 而并行自举下后者是常态——于是"检测到就写回"的对象大概率是**另一条线的正当
//! 工作**。2026-08-16 它把主树活 SQLite 的 WAL 回滚成旧版本、把 228 MB 的 state.db
//! 写到只读都打不开,还把正在修本文件的改动一并回滚。故自动回滚/删除整体停用,
//! 待 D-395 的归属判定落地后再开;现在只报告、归因、把改后内容隔离留证供取回。
//!
//! # 快照策略(现状,不是承诺)
//!
//! 执行前后各拍一次其它线树的**全文件内容**(受限镜像)逐路径比对:内容不同 → 记为
//! 变化并留证;内容相同(`touch` 之类)→ 不算越界。**mtime/len 粗筛尚未实现**
//! (D-397 在册):当前每条前台 bash 对每棵其它线树做两次全量读,大仓下开销可观。
//!
//! # 性能与安全边界
//!
//! - 单文件镜像上限 4 MiB、单树文件数上限 2000(与托管文档同口径):超限时只记
//!   `None`,该文件的变化检测不出来(D-396 在册,语义待收敛);
//! - 非 git 仓库 / `git worktree list` 失败:返回空快照,**不阻塞 bash**(不是所有
//!   bash 都跑在 git 仓里,更没有其它线可保护——此时保护面就是空集);
//! - 运行态与派生产物不入镜像:`.kanzei`(活 SQLite)、`target`、`node_modules`、
//!   `dist`、`.git`——见 [`EXCLUDED_TREE_DIRS`];
//! - 本线的树由调用方给出(`self_tree`,主树进程传主根),git 清单里排除它。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// D-407:**运行态与派生产物不入保护面**——这些目录里的东西不是「另一条线未提交的
/// 心血」,而是随时在变的活状态与可重建产物,把它们纳入保护面只会两头坏事:
/// ①`.kanzei/` 下是活的 SQLite(state.db 与其 -wal/-shm)——回滚一份旧 WAL 盖到
/// 正在被打开的库上,直接把库写坏(2026-08-16 实况:研究会话 disk I/O error,
/// 只读都打不开);②`target/`、`node_modules/`、`dist/` 是构建产物,分钟级全量变化,
/// 既撑爆 2000 文件上限又让每条 bash 白读几百兆。
///
/// 主根托管文档的保护由 ManagedSnapshot 独立承担(D-173/D-174),不依赖本模块;
/// 其它树里的 `.kanzei/` 是分支副本,不是权威真源,漏保护不造成事实丢失。
const EXCLUDED_TREE_DIRS: &[&str] = &[".kanzei", "target", "node_modules", "dist", ".git"];
/// 可重建的生成产物路径：仅按完整相对路径排除，不能把所有同名 `schemas` 目录误伤。
const EXCLUDED_TREE_PATHS: &[&[&str]] = &[&["gen", "schemas"]];

/// D-407:是否为不入保护面的目录名(见 `EXCLUDED_TREE_DIRS`)。
/// `.git` 一并收在这里:主树的 `.git` 是目录、worktree 的是文件(gitdir: 指针),
/// 两种形态都靠名字判定,与原先的单独判断等价。
fn is_excluded_entry(name: &str) -> bool {
    EXCLUDED_TREE_DIRS.contains(&name)
}

/// 目录名排除之外的路径级豁免。构建生成的 `gen/schemas` 必须按路径匹配，
/// 否则把任意名为 `schemas` 的源码目录加入全局目录黑名单会扩大保护盲区。
fn is_excluded_path(tree_root: &Path, path: &Path) -> bool {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_excluded_entry)
    {
        return true;
    }
    let Ok(relative) = path.strip_prefix(tree_root) else {
        return false;
    };
    let components = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    EXCLUDED_TREE_PATHS.iter().any(|excluded| {
        components
            .windows(excluded.len())
            .any(|window| window == *excluded)
    })
}

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
        self.files_for_tree(root).is_some()
    }

    /// `git worktree list` 在 Windows runner 上可能把同一路径输出成不同大小写、
    /// 斜杠或短路径形态。测试查询必须复用生产的一树一线键语义，不能拿原始
    /// `PathBuf` 做字面量比较，否则保护面真实存在也会被 CI 误判为缺失。
    #[cfg(test)]
    fn files_for_tree(&self, root: &Path) -> Option<&BTreeMap<String, FileImage>> {
        let expected = crate::worktree::worktree_key(root);
        self.trees
            .iter()
            .find(|(candidate, _)| crate::worktree::worktree_key(candidate) == expected)
            .map(|(_, files)| files)
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
    collect_tree_files_in(root, root, files)
}

/// D-406:递归必须携带**树根**与当前目录两个参数——此前递归把子目录当新 root,
/// `strip_prefix` 永远相对直接父目录,深层文件的镜像键全被扁平成裸 basename:
/// 键跨目录碰撞让对账从未按正确路径比过,回滚更把别树深层文件按裸名写到树根
/// (主根 defects.md/bin-kz/dep-lib-* 平铺垃圾的来源),树根同名文件还有被误删风险。
fn collect_tree_files_in(
    tree_root: &Path,
    dir: &Path,
    files: &mut BTreeMap<String, FileImage>,
) -> bool {
    if files.len() >= OTHER_TREE_MAX_FILES {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
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
        // D-407:运行态与可重建产物不入镜像；gen/schemas 由完整路径级规则排除。
        if is_excluded_path(tree_root, &path) {
            continue;
        }
        if file_type.is_dir() {
            truncated |= collect_tree_files_in(tree_root, &path, files);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(tree_root) else {
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
    collect_tree_metadata_in(root, root, files)
}

/// D-406 同款纪律:递归携带树根+当前目录,键始终相对树根;
/// D-407 同款排除:与 collect_tree_files 完全同一份 is_excluded_entry,
/// 否则 target/node_modules 会在指纹面出现、在内容面缺席,整批误报为新增。
fn collect_tree_metadata_in(
    tree_root: &Path,
    dir: &Path,
    files: &mut BTreeMap<String, (u64, u128)>,
) -> bool {
    if files.len() >= OTHER_TREE_MAX_FILES {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
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
        if is_excluded_path(tree_root, &path) {
            continue;
        }
        if file_type.is_dir() {
            truncated |= collect_tree_metadata_in(tree_root, &path, files);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(tree_root) else {
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
/// 有差异 → 逐路径查写日志(R-268/D-395):窗口内该树自己线的专用工具写入
/// (write/edit/insert 留了日志,路径=相对树根、指纹=写后内容)视为**合法自写**,
/// 吸收不进报告;无日志解释的变化 → 隔离留证 + 报告(D-407 姿态,不自动回滚)。
/// 无差异 → 返回 None。**非 git 仓库 / git 失败 → 返回 None 不阻塞**
/// (没有其它线树可保护,保护面为空集,与执行前的判定一致)。
///
/// `before` 为空快照时同样直接返回 None:执行前就拍不到任何树,执行后也拍不到,
/// 无从对账——这时静默放行比编一个「保护了一切」的假报告诚实。
///
/// `owner_run` / `owner_process` 是执行这条命令的线身份(R-186 验收②归因):
/// 越界报告必须点名是哪条线越的界,轨迹(ToolOutput error)里据此可审计。
///
/// `window_start_ms` 是命令窗口起点(拍 `before` 快照的时刻):写日志对账只认
/// 这个时刻之后的条目——窗口之前的历史写入不参与本次对账,与托管围栏同口径
/// (R-268)。
pub(crate) fn enforce_other_trees(
    project_root: &Path,
    self_tree: &Path,
    before: &OtherTreesSnapshot,
    owner_run: Option<&str>,
    owner_process: Option<&str>,
    window_start_ms: u128,
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
    // D-395:写日志对账——窗口内该树自己线的专用工具写入(有日志且写后指纹 ==
    // 当前快照指纹)视为合法自写,吸收进基线不回滚不报告。与托管围栏同口径
    // (R-268):只认窗口起点之后的日志,终态一致才算命中。三态适配(D-396):
    // 只有拿得到写后内容(Content)或删除(按空内容)才可对账吸收;Fingerprint
    // 无内容可验指纹,宁可保留报告也不吸收。
    let logs = crate::write_log::entries_after(project_root, window_start_ms);
    let covered_by_log = |rel: &str, after_image: &Option<FileImage>| -> bool {
        let fingerprint = match after_image {
            Some(FileImage::Content { bytes, .. }) => crate::content_hash(bytes),
            Some(FileImage::Fingerprint { .. }) | Some(FileImage::Absent) => return false,
            None => crate::content_hash(&[]),
        };
        logs.iter().any(|entry| {
            entry.path == rel && entry.fingerprint == fingerprint && entry.at_ms >= window_start_ms
        })
    };
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
        // D-395:吸收合法自写——有写日志解释(路径+指纹+窗口内)的变化是
        // 该树自己线的正常写入,不是本命令越界;从未被吸收的才留下。
        tree_changes.retain(|rel, (_, after_content)| !covered_by_log(rel, after_content));
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
        "[cross-tree] DETECTED (report-only, evidence quarantined; automatic rollback is \
         DISABLED pending D-395). Files in another line's worktree changed during this command \
         — those trees are protected (R-186). Verify whether this command crossed into another \
         line's tree; if it did, restore from the quarantine copy listed below."
            .to_string(),
        owner_line,
    ];
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
            // D-407:**自动回滚已停用**(2026-08-16 拍板,当时回滚把主树活动 SQLite 的
            // WAL 写回旧版本,写坏 228 MB state.db)。本机制无法充分区分「A 线越界写
            // 了 B 树」与「B 线在自己树里正常干活」;D-395 写日志吸收(本文件上方)已
            // 接入吃掉后者的大头,但重开「检测到就写回」需在真实并行场景验证吸收充分
            // 后另行决策,不随收编顺带打开。
            //
            // 现在的姿态:照常检测、照常隔离留证(改后内容一份不丢)、照常归因进报告,
            // 但**不动磁盘上的现状**——可见性保住,破坏面清零。要恢复请从上面隔离
            // 目录里取回,或用 git 自己的手段。
            // D-396:超限文件(前后任一侧无内容镜像)点名报告——改后内容隔离不到,
            // 或被删且原内容未镜像,都不能编「已留证/可恢复」。
            let oversized_changed = matches!(after_image, Some(FileImage::Fingerprint { .. }));
            let oversized_deleted =
                matches!(before_image, FileImage::Fingerprint { .. }) && after_image.is_none();
            if oversized_changed || oversized_deleted {
                unrestored.push(format!("{root_display}/{rel}"));
            }
            let _ = (&absolute, before_image);
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
            "  · 超限文件(>{other_limit} 字节)改动已检出但无内容镜像可隔离,保持现状(不删除): {list}",
            other_limit = OTHER_TREE_FILE_LIMIT,
            list = unrestored.join(", ")
        ));
    }
    lines.push(format!(
        "  touched: {} (检测到 {} 个文件变化;**未自动回滚**,现状保持不变,改后内容已隔离留证)",
        touched.join(", "),
        touched.len()
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

    /// A 线(主树)的 bash 写 B 线工作树 → 检出、归因、隔离留证。
    /// D-407:自动回滚已停用(见 enforce_other_trees 内注释),现状保持不变——
    /// 断言从「逐字节复原」改为「改后内容原样保留 + 隔离目录里有可取回的副本」。
    #[test]
    fn a线bash写b线树_检出归因并隔离留证_不动现状() {
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

        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"), 0)
            .expect("必须检出越界");
        assert!(
            report.contains("seed.txt") && report.contains("new-file.txt"),
            "报告要点名被改文件: {report}"
        );
        assert!(
            report.contains("run-a") && report.contains("proc-a"),
            "归因必须点名 owner run/process(验收②): {report}"
        );
        // D-407:不再自动回滚——磁盘现状保持「改后」的样子。
        assert_eq!(
            std::fs::read_to_string(b.join("seed.txt")).unwrap(),
            "A 线覆盖!\n",
            "报告态下不得改动磁盘现状(自动回滚已停用)"
        );
        assert!(
            report.contains("未自动回滚"),
            "报告须明说未回滚,不能让人以为已复原: {report}"
        );
        assert!(!b.join(".kanzei").is_dir()); // 无托管根,隔离目录在主根。
                                              // 隔离留证仍是硬要求:改后内容一份不丢,可原样取回。
        let quarantine_root = root.join(".kanzei/quarantine");
        assert!(quarantine_root.is_dir(), "隔离留证目录必须存在");
        let saved: Vec<_> = walk_files(&quarantine_root);
        assert!(
            saved.iter().any(|p| p.ends_with("seed.txt")),
            "隔离目录里必须留有改后副本: {saved:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// D-395:并行双线真场景——A 线长 bash 窗口内,B 线用专用工具(write/edit)
    /// 写自己的树。写日志是该线合法自写的凭据:围栏收口对账时命中(路径=相对树根、
    /// 指纹=写后内容、时刻在窗口内)即**吸收**,不得把 B 线的正当工作误判为 A 越界。
    #[test]
    fn 并行双线_b线窗口内自写有写日志_被吸收不误报() {
        let root = git_repo("kz-ct-d395");
        let b = add_worktree(&root, "line-b");
        std::fs::write(b.join("b-own.txt"), "B 线原文\n").unwrap();
        let window_start = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        // A 线(主树)执行前拍快照,保护面含 B 树。
        let before = capture_other_trees(&root, &root).expect("快照失败");
        assert!(before.contains_tree(&b), "B 线工作树必须在保护面内");

        // 窗口内 B 线专用工具(write 语义)写自己的树,并留写日志凭据——
        // 路径用相对树根(worktree 线 cwd==树根),与跨树快照 key 同口径。
        let rel = "b-own.txt";
        let new_content = "B 线窗口内自己的修改\n".as_bytes();
        std::fs::write(b.join(rel), new_content).unwrap();
        crate::write_log::record(
            &root,
            &crate::write_log::WriteLogEntry {
                at_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
                path: rel.to_string(),
                fingerprint: crate::content_hash(new_content),
                content: new_content.to_vec(),
                run_id: Some("run-b".into()),
                process_id: Some("proc-b".into()),
            },
        )
        .unwrap();

        // 收口:A 的命令没碰 B 树——变化全部有 B 线写日志解释,必须吸收。
        let report = enforce_other_trees(
            &root,
            &root,
            &before,
            Some("run-a"),
            Some("proc-a"),
            window_start,
        );
        assert!(
            report.is_none(),
            "B 线窗口内自写(有写日志凭据)必须被吸收,不得误报: {report:?}"
        );
        assert_eq!(
            std::fs::read_to_string(b.join(rel)).unwrap(),
            "B 线窗口内自己的修改\n",
            "B 线的自写必须原样保留"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// D-395:同一窗口内 A 越界写 B 树(无写日志凭据)照旧检出——吸收只认
    /// 「B 线自己的专用工具写入」,不豁免真正的越界。
    #[test]
    fn 并行双线_无写日志的越界写照旧检出() {
        let root = git_repo("kz-ct-d395b");
        let b = add_worktree(&root, "line-b");
        std::fs::write(b.join("seed.txt"), "B 线原文\n").unwrap();
        let window_start = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let before = capture_other_trees(&root, &root).expect("快照失败");
        // A 的命令越界改 B 树——没有对应写日志。
        std::fs::write(b.join("seed.txt"), "A 线越界覆盖\n").unwrap();

        let report = enforce_other_trees(
            &root,
            &root,
            &before,
            Some("run-a"),
            Some("proc-a"),
            window_start,
        )
        .expect("无写日志的越界必须检出");
        assert!(
            report.contains("seed.txt"),
            "报告必须点名被改文件: {report}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 递归收集目录下全部文件(测试用:核对隔离留证内容)。
    fn walk_files(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk_files(&path));
            } else {
                out.push(path);
            }
        }
        out
    }

    /// D-406:深层文件的镜像键必须是完整相对路径——此前递归按父目录 strip_prefix,
    /// 键被扁平成裸 basename:跨目录碰撞让对账失真,回滚把深层文件按裸名拍到树根
    /// (主根 defects.md/bin-kz/dep-lib-* 平铺垃圾的现场)。既有测试全用顶层文件
    /// (basename==相对路径)所以一路假绿,本测试用嵌套+根级同名文件做判别。
    #[test]
    fn 深层文件键为完整相对路径_留证按原层级不落树根() {
        let root = git_repo("kz-ct-d406");
        let b = add_worktree(&root, "line-d406");
        std::fs::create_dir_all(b.join("sub/inner")).unwrap();
        std::fs::write(b.join("sub/inner/deep.txt"), "B 线深层原文\n").unwrap();
        std::fs::write(b.join("deep.txt"), "B 线根级同名\n").unwrap();

        let before = capture_other_trees(&root, &root).expect("快照失败");
        // seed.txt(基础提交检出)+ deep.txt + sub/inner/deep.txt = 3;
        // 带 D-406 bug 时深层与根级同名碰撞成一个键,只剩 2。
        assert_eq!(
            before.file_count(),
            3,
            "深层与根级同名必须是两个独立键(裸 basename 键碰撞即 D-406 复现)"
        );

        std::fs::write(b.join("sub/inner/deep.txt"), "A 线越界覆盖\n").unwrap();
        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"), 0)
            .expect("必须检出越界");
        assert!(
            report.contains("sub/inner/deep.txt"),
            "报告须点名完整相对路径: {report}"
        );
        // 键正确性的判别落在隔离留证的层级上:必须是 sub/inner/deep.txt,
        // 不是裸 deep.txt(后者即 D-406 扁平化复现)。
        let saved = walk_files(&root.join(".kanzei/quarantine"));
        assert!(
            saved.iter().any(|p| p.ends_with("sub\\inner\\deep.txt")
                || p.to_string_lossy()
                    .replace('\\', "/")
                    .ends_with("sub/inner/deep.txt")),
            "隔离留证须按完整层级存放,不得扁平成裸文件名: {saved:?}"
        );
        assert_eq!(
            std::fs::read_to_string(b.join("deep.txt")).unwrap(),
            "B 线根级同名\n",
            "根级同名文件不得被深层内容污染(键碰撞判别)"
        );
        assert!(
            !root.join("deep.txt").exists(),
            "任何内容都不得被拍到本线树根(平铺垃圾判别)"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gen_schemas构建产物按路径排除而同名源码目录保留() {
        let root = git_repo("kz-ct-schema");
        let b = add_worktree(&root, "line-schema");
        std::fs::create_dir_all(b.join("gen/schemas")).unwrap();
        std::fs::create_dir_all(b.join("src/schemas")).unwrap();
        std::fs::write(b.join("gen/schemas/desktop-schema.json"), "generated\n").unwrap();
        std::fs::write(b.join("src/schemas/domain.json"), "source\n").unwrap();

        let before = capture_other_trees(&root, &root).expect("快照失败");
        let mut keys = before.files_for_tree(&b).expect("B 线应在保护面").keys();
        assert!(!keys.any(|key| key == "gen/schemas/desktop-schema.json"));
        assert!(before
            .trees
            .get(&b)
            .unwrap()
            .contains_key("src/schemas/domain.json"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// D-407:运行态与派生产物不入保护面——`.kanzei/`(活 SQLite)、`target/`、
    /// `node_modules/`、`dist/` 一律跳过。这条是本次事故的直接护栏:回滚一份旧
    /// WAL 盖到正在被打开的库上会把库写坏(2026-08-16 实况)。
    #[test]
    fn 运行态与派生产物不入保护面() {
        let root = git_repo("kz-ct-d407");
        let b = add_worktree(&root, "line-d407");
        for (dir, name) in [
            (".kanzei", "state.db-wal"),
            ("target", "artifact.bin"),
            ("node_modules", "pkg.js"),
            ("dist", "bundle.js"),
        ] {
            std::fs::create_dir_all(b.join(dir)).unwrap();
            std::fs::write(b.join(dir).join(name), "before\n").unwrap();
        }
        std::fs::write(b.join("src.txt"), "源码\n").unwrap();

        let before = capture_other_trees(&root, &root).expect("快照失败");
        assert_eq!(
            before.file_count(),
            2,
            "保护面只应含 seed.txt 与 src.txt:运行态与派生产物必须被排除"
        );

        // 改这些被排除的文件:不得被判越界(活库 WAL 变化是常态,判越界会毁库)。
        for (dir, name) in [
            (".kanzei", "state.db-wal"),
            ("target", "artifact.bin"),
            ("node_modules", "pkg.js"),
            ("dist", "bundle.js"),
        ] {
            std::fs::write(b.join(dir).join(name), "after\n").unwrap();
        }
        assert!(
            enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"), 0).is_none(),
            "被排除目录的变化不得触发越界报告"
        );
        assert_eq!(
            std::fs::read_to_string(b.join(".kanzei/state.db-wal")).unwrap(),
            "after\n",
            "活库文件绝不能被动"
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
        assert!(enforce_other_trees(&dir, &dir, &before, None, None, 0).is_none());
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

        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"), 0);
        assert!(
            report.is_none(),
            "内容相同的 touch 不应算越界,实得: {report:?}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "未提交内容\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 越界**新建**文件(执行前不存在):被删除回滚。
    #[test]
    fn 越界新建文件_被检出但不删除() {
        let root = git_repo("kz-ct-b4");
        let b = add_worktree(&root, "line-b");

        let before = capture_other_trees(&root, &root).expect("快照失败");
        std::fs::write(b.join("intruder.txt"), "A 线塞进来的\n").unwrap();

        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"), 0)
            .expect("必须检出");
        assert!(report.contains("intruder.txt"), "{report}");
        // D-407:不再自动删除——删的可能正是另一条线自己新建的文件(D-395)。
        assert!(
            b.join("intruder.txt").exists(),
            "报告态下不得删除文件(自动回滚已停用)"
        );
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

        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"), 0)
            .expect("build.rs 写别的树必须被抓");
        assert!(
            report.contains("victim.txt"),
            "报告必须点名 build.rs 写入的文件: {report}"
        );
        // D-407:报告态——检出与归因照旧(这正是本条相对语法闸门的核心优势),
        // 但不再自动删除;改后内容进隔离目录留证。
        assert!(
            b.join("victim.txt").exists(),
            "报告态下不得删除文件(自动回滚已停用)"
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
        // 改动已镜像的文件 → 报告必须点名截断警告;D-407 report-only,现状保持。
        std::fs::write(b.join("f-0000.txt"), "CHANGED\n").unwrap();
        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"), 0)
            .expect("必须检出越界");
        assert!(
            report.contains("截断") && report.contains("不完整"),
            "截断显式报告: {report}"
        );
        assert_eq!(
            std::fs::read_to_string(b.join("f-0000.txt")).unwrap(),
            "CHANGED\n",
            "report-only:不自动回滚,现状保持"
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
        let bfiles = before.files_for_tree(&b).expect("B 线应在保护面");
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

        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"), 0)
            .expect("必须检出越界");
        // D-407 report-only:超限与小文件都保持现状,只报告不回滚。
        let after_big = std::fs::read(b.join("big.bin")).unwrap();
        assert_eq!(after_big, big2, "超限文件改动保持现状");
        assert_eq!(
            std::fs::read_to_string(b.join("small.txt")).unwrap(),
            "CHANGED\n",
            "report-only:小文件也不回滚,现状保持"
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
        let report = enforce_other_trees(&root, &root, &before, Some("run-a"), Some("proc-a"), 0)
            .expect("必须检出越界");
        assert!(
            !b.join("big.bin").exists(),
            "超限文件被删无法回滚原内容(保持删除),如实说明"
        );
        assert!(report.contains("超限"), "报告必须点名超限: {report}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
