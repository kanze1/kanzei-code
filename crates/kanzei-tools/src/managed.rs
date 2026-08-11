//! 托管文档围栏(D-173/R-139 前台版、D-174 后台版共用)。
//!
//! 托管目录只能由专用工具写入。"能不能绕过"绝不能靠猜命令文本——
//! `[System.IO.File]::WriteAllText`、重定向、python/node 一行流、`git checkout`
//! 单文件都能避开任何字符串匹配,所以本模块一律走**结果侧**判定:
//! 动作之前拍下托管目录的镜像,动作之后再比一次,改了就隔离留证 + 整体回滚。
//!
//! 前台(bash 同步命令)与后台(D-174 受管后台任务)共用同一套快照/回滚口径,
//! 仓库里不存在第二套"不能写入"的语义。

use std::collections::BTreeMap;
use std::path::Path;

/// 托管目录:write/edit 对它们硬 deny,shell 也不许绕过去改(D-173)。
pub(crate) const MANAGED_ROOTS: &[&str] = &[".kanzei/project", ".kanzei/memory"];
/// 单文件镜像上限:超过就只记指纹,能检测但无法回滚(会如实说明)。
pub(crate) const MANAGED_SNAPSHOT_FILE_LIMIT: u64 = 4 * 1024 * 1024;
/// 镜像文件数上限,防止有人往托管目录塞进一整棵大树把每次 bash 拖垮。
pub(crate) const MANAGED_SNAPSHOT_MAX_FILES: usize = 2000;

/// 托管目录的镜像。`None` 内容 = 文件超过镜像上限,只能检测不能回滚。
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ManagedSnapshot {
    files: BTreeMap<String, Option<Vec<u8>>>,
    /// 目录规模超限时放弃镜像:此时既不检测也不回滚,并在输出里如实说明。
    truncated: bool,
}

impl ManagedSnapshot {
    pub(crate) fn capture(project_root: &Path) -> Self {
        let mut snapshot = ManagedSnapshot::default();
        for root in MANAGED_ROOTS {
            collect_files(&project_root.join(root), project_root, &mut snapshot);
            if snapshot.truncated {
                break;
            }
        }
        snapshot
    }

    pub(crate) fn is_complete(&self) -> bool {
        !self.truncated && self.files.values().all(Option::is_some)
    }

    /// 把 `current` 里 `paths` 指定的这些路径吸收进本基线(其余路径保持原样)。
    ///
    /// D-258:吸收粒度从「前缀内全部路径」收窄为「窗口期间实际变化的路径」。
    /// 整前缀吸收会让后台进程在窗口内偷写的文件(专用工具没碰、窗口前后却有变化)
    /// 也被固化进基线;按路径吸收只认「打开/关闭两次镜像之间的差异」,后台进程
    /// 必须和专用工具写同一批文件才能蒙混——写别的文件会在窗口关闭后留痕,
    /// 被守卫下一轮对账判成越界回滚。
    ///
    /// `paths` 里在 `current` 中不存在的路径视为被删除(窗口内被删的托管文件同样
    /// 要吸收,否则守卫会把"专用工具的归档移动"当成越界删除再给写回去)。
    pub(crate) fn absorb_paths(&mut self, current: &ManagedSnapshot, paths: &[&str]) {
        for &path in paths {
            match current.files.get(path) {
                Some(content) => {
                    self.files.insert(path.to_string(), content.clone());
                }
                None => {
                    self.files.remove(path);
                }
            }
        }
    }
}

/// 两次镜像之间的托管变更集。
#[derive(Debug, Default, Clone)]
pub(crate) struct ManagedChange {
    pub(crate) modified: Vec<String>,
    pub(crate) created: Vec<String>,
    pub(crate) deleted: Vec<String>,
}

impl ManagedChange {
    pub(crate) fn is_empty(&self) -> bool {
        self.modified.is_empty() && self.created.is_empty() && self.deleted.is_empty()
    }

    /// 全部被触碰的路径(修改 + 新建 + 删除)。
    pub(crate) fn touched(&self) -> Vec<&String> {
        self.modified
            .iter()
            .chain(self.created.iter())
            .chain(self.deleted.iter())
            .collect()
    }

    /// 按谓词切成两半:`keep` 为真的进左边,其余进右边。
    /// 后台守卫用它把"窗口正开着、有合法解释"的路径与真正的越界分流。
    pub(crate) fn partition(&self, keep: impl Fn(&str) -> bool) -> (ManagedChange, ManagedChange) {
        let split = |paths: &[String]| -> (Vec<String>, Vec<String>) {
            paths.iter().cloned().partition(|path| keep(path))
        };
        let (kept_modified, rest_modified) = split(&self.modified);
        let (kept_created, rest_created) = split(&self.created);
        let (kept_deleted, rest_deleted) = split(&self.deleted);
        (
            ManagedChange {
                modified: kept_modified,
                created: kept_created,
                deleted: kept_deleted,
            },
            ManagedChange {
                modified: rest_modified,
                created: rest_created,
                deleted: rest_deleted,
            },
        )
    }
}

/// 比对两次镜像。无差异返回 None。
pub(crate) fn diff(before: &ManagedSnapshot, after: &ManagedSnapshot) -> Option<ManagedChange> {
    let mut change = ManagedChange::default();
    for (path, content) in &after.files {
        match before.files.get(path) {
            None => change.created.push(path.clone()),
            Some(old) if old != content => change.modified.push(path.clone()),
            Some(_) => {}
        }
    }
    for path in before.files.keys() {
        if !after.files.contains_key(path) {
            change.deleted.push(path.clone());
        }
    }
    (!change.is_empty()).then_some(change)
}

/// 隔离留证 + 回滚到 `before`。返回(留证目录, 实际回滚成功的文件数)。
///
/// 先隔离再回滚:哪怕这次改动其实来自用户手改,内容也一份不丢,可原样取回。
pub(crate) fn quarantine_and_restore(
    project_root: &Path,
    before: &ManagedSnapshot,
    change: &ManagedChange,
    tag: &str,
) -> (std::path::PathBuf, usize) {
    let quarantine = project_root.join(".kanzei/quarantine").join(format!(
        "{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default()
    ));
    let mut restored = 0usize;
    for path in change.modified.iter().chain(change.created.iter()) {
        let absolute = project_root.join(path);
        let saved = quarantine.join(path);
        if let Some(parent) = saved.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::copy(&absolute, &saved);
        match before.files.get(path) {
            // 动作前存在:写回原内容(内容超限没镜像的只能保持现状,由调用方点名)。
            Some(Some(original)) => {
                if std::fs::write(&absolute, original).is_ok() {
                    restored += 1;
                }
            }
            Some(None) => {}
            // 动作前不存在:删掉新建的。
            None => {
                if std::fs::remove_file(&absolute).is_ok() {
                    restored += 1;
                }
            }
        }
    }
    for path in &change.deleted {
        if let Some(Some(original)) = before.files.get(path) {
            let absolute = project_root.join(path);
            if let Some(parent) = absolute.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&absolute, original).is_ok() {
                restored += 1;
            }
        }
    }
    (quarantine, restored)
}

/// 项目是否处于 Harness 托管之下(存在 `.kanzei` 目录)。
pub(crate) fn managed_scope_exists(project_root: &Path) -> bool {
    project_root.join(".kanzei").is_dir()
}

fn collect_files(dir: &Path, project_root: &Path, snapshot: &mut ManagedSnapshot) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if snapshot.files.len() >= MANAGED_SNAPSHOT_MAX_FILES {
            snapshot.truncated = true;
            return;
        }
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => collect_files(&path, project_root, snapshot),
            Ok(kind) if kind.is_file() => {
                let Ok(relative) = path.strip_prefix(project_root) else {
                    continue;
                };
                let key = relative.display().to_string().replace('\\', "/");
                let size = entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
                let content = (size <= MANAGED_SNAPSHOT_FILE_LIMIT)
                    .then(|| std::fs::read(&path).ok())
                    .flatten();
                snapshot.files.insert(key, content);
            }
            _ => {}
        }
    }
}

/// 比对执行前后的托管目录镜像。有改动就隔离留证 + 整体回滚,并生成回喂模型的报告。
pub(crate) fn enforce_managed_files(
    project_root: &Path,
    before: ManagedSnapshot,
) -> Option<String> {
    let after = ManagedSnapshot::capture(project_root);
    if after == before {
        return None;
    }
    let after_incomplete = !after.is_complete();
    let change = diff(&before, &after)?;
    let listed = change
        .touched()
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let (quarantine, restored) = quarantine_and_restore(project_root, &before, &change, "shell");

    let incomplete = if after_incomplete {
        format!(
            "\nWARNING: the post-command snapshot exceeded its safety bound. Known changes were \
             rolled back, but the managed tree must be inspected manually before any further shell \
             command (limit: {MANAGED_SNAPSHOT_MAX_FILES} files / {MANAGED_SNAPSHOT_FILE_LIMIT} bytes per file)."
        )
    } else {
        String::new()
    };
    Some(format!(
        "[managed-files] BLOCKED AND ROLLED BACK. This command modified files under {} — those \
         paths are policy-managed and the shell is not a write channel for them, no matter which \
         mechanism is used (redirect, Set-Content/Out-File, [System.IO.File]::WriteAllText, \
         python/node one-liner, git checkout of a single file).\n\
         touched: {listed}\n\
         restored {restored} file(s) to their pre-command contents; your versions were kept at \
         {} so nothing is lost.\n\
         Redo the change through the dedicated tool (`req`/`defect`/`goal`/`decision` for tracker \
         entries, `architecture` for the architecture index, `memory_note` for memory). If no \
         tool covers what you need, that is an unimplemented capability: record it and tell the \
         user — do not look for another shell route.{incomplete}",
        MANAGED_ROOTS.join(" and "),
        quarantine.display(),
    ))
}
