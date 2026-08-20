//! 记忆 ID 台账域(R-255 第三刀,从 store.rs 迁出)。
//!
//! 独立理由:ID ledger 是「编号怎么分配/注销/审计」的独立变更理由——`merge`(重复合并
//! 与墓碑)、`void_id`/`voided_ids`(缺号注销台账 D-321)、`integrity_issues`(完整性
//! 审计)互不相关,与准入/检索/收件箱正交:改一条合并策略不必读懂检索(照
//! files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):merge 保守闸——无用户确认时 primary 与每个 duplicate 必须共享
//! 至少一个 fingerprint,否则拒绝(合并销毁证据链,不能靠 manager 自觉);voided-ids.md
//! 写入口持树锁(D-368);void 登记后若编号又出现条目,integrity 必须可见(账实不符)。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::memory::fp_markers;

use super::store::MemoryStore;
use super::MemoryEntry;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DeduplicationReport {
    pub groups: usize,
    pub duplicates: Vec<String>,
    pub primaries: Vec<String>,
}

fn normalized_title(title: &str) -> String {
    title
        .chars()
        .filter(|ch| !ch.is_whitespace() && ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

impl MemoryStore {
    /// 机械合并存量重复簇：同 fingerprint 或严格规范化同标题。
    /// active 优先、正文最长优先为 primary；所有写入都复用 merge 的墓碑/归档路径。
    pub fn deduplicate_redundant_entries(&self) -> anyhow::Result<DeduplicationReport> {
        let entries = self.load_all();
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (_, entry) in &entries {
            if !matches!(entry.status.as_str(), "active" | "candidate") {
                continue;
            }
            for fingerprint in fp_markers(&entry.body) {
                groups
                    .entry(format!("fingerprint:{fingerprint}"))
                    .or_default()
                    .push(entry.id.clone());
            }
            let title = normalized_title(&entry.title);
            if !title.is_empty() {
                groups
                    .entry(format!("title:{title}"))
                    .or_default()
                    .push(entry.id.clone());
            }
        }

        let mut report = DeduplicationReport::default();
        let mut merged_ids = BTreeSet::new();
        for (key, ids) in groups {
            let mut current: Vec<MemoryEntry> = self
                .load_all()
                .into_iter()
                .filter_map(|(_, entry)| ids.contains(&entry.id).then_some(entry))
                .filter(|entry| matches!(entry.status.as_str(), "active" | "candidate"))
                .collect();
            if current.len() < 2 {
                continue;
            }
            current.sort_by(|left, right| {
                right
                    .status
                    .eq("active")
                    .cmp(&left.status.eq("active"))
                    .then_with(|| right.body.chars().count().cmp(&left.body.chars().count()))
                    .then_with(|| left.id.cmp(&right.id))
            });
            let primary = current.remove(0);
            let duplicates: Vec<String> = current.into_iter().map(|entry| entry.id).collect();
            if duplicates.iter().any(|id| merged_ids.contains(id)) {
                continue;
            }
            self.merge(
                &primary.id,
                &duplicates,
                None,
                None,
                Some(&primary.body),
                key.starts_with("title:"),
            )?;
            report.groups += 1;
            report.primaries.push(primary.id);
            report.duplicates.extend(duplicates.iter().cloned());
            merged_ids.extend(duplicates);
        }
        Ok(report)
    }

    /// ID 分配扫活跃+归档,编号绝不复用(同 tracker 哲学)。
    pub fn next_id(&self, entries: &[(PathBuf, MemoryEntry)]) -> String {
        let prefix = self.scope.prefix();
        let parse = |id: &str| {
            id.strip_prefix(prefix)?
                .strip_prefix('-')?
                .parse::<u32>()
                .ok()
        };
        let max = entries
            .iter()
            .filter_map(|(_, e)| parse(&e.id))
            .chain(self.load_archived_ids().iter().filter_map(|id| parse(id)))
            .max()
            .unwrap_or(0);
        format!("{}-{:03}", prefix, max + 1)
    }

    /// 合并重复:并入首个 id(保住最老引用),其余 stale 并留 superseded_by 墓碑链接。
    pub fn merge(
        &self,
        primary: &str,
        duplicates: &[String],
        title: Option<&str>,
        description: Option<&str>,
        body: Option<&str>,
        confirmed: bool,
    ) -> anyhow::Result<super::MemoryEntry> {
        if duplicates.is_empty() {
            anyhow::bail!("merge needs at least one duplicate id");
        }
        if duplicates.iter().any(|d| d == primary) {
            anyhow::bail!("primary id cannot appear in duplicates");
        }
        let entries = self.load_all();
        for id in std::iter::once(&primary.to_string()).chain(duplicates.iter()) {
            if !entries.iter().any(|(_, e)| e.id.as_str() == id.as_str()) {
                anyhow::bail!("unknown memory id `{id}`");
            }
        }
        // R-165 批4 merge 保守闸(⑧):评估器落地前只合并同 fingerprint 或用户确认的。
        // 无用户确认时,primary 与每个 duplicate 必须共享至少一个 [fp:...] 标记,
        // 否则拒绝——合并会销毁证据链,不能靠 manager 自觉。
        if !confirmed {
            let fps_of = |id: &str| -> Vec<String> {
                entries
                    .iter()
                    .find(|(_, e)| e.id == id)
                    .map(|(_, e)| fp_markers(&e.body))
                    .unwrap_or_default()
            };
            let primary_fps = fps_of(primary);
            for dup in duplicates {
                let shared = fps_of(dup).iter().any(|f| primary_fps.contains(f));
                if !shared {
                    anyhow::bail!(
                        "merge 保守闸: `{primary}` 与 `{dup}` 无共享 fingerprint,且未获用户确认 \
                         (confirmed=true)——评估器落地前只合并同 fingerprint 或用户确认的条目"
                    );
                }
            }
        }
        let mut merged = self.update(primary, title, description, body, None, None, false)?;
        // D-215 引擎兜底:被并条目的复发指纹与来源引用不许静默蒸发——
        // 指纹丢了复发检测就瞎了,refs 丢了记忆与来源脱钩,这两样不能赌 manager 记得带。
        let mut carried_fps: Vec<String> = Vec::new();
        let mut union_refs: Vec<String> = merged.refs();
        for id in duplicates {
            let (_, dup) = self
                .load_all()
                .into_iter()
                .find(|(_, e)| &e.id == id)
                .expect("checked above");
            for marker in fp_markers(&dup.body) {
                if !merged.body.contains(&marker) && !carried_fps.contains(&marker) {
                    carried_fps.push(marker);
                }
            }
            for r in dup.refs() {
                if !union_refs.contains(&r) {
                    union_refs.push(r);
                }
            }
        }
        if !carried_fps.is_empty() || union_refs != merged.refs() {
            let (path, mut entry) = self
                .load_all()
                .into_iter()
                .find(|(_, e)| e.id == merged.id)
                .expect("primary exists");
            if !carried_fps.is_empty() {
                entry
                    .body
                    .push_str(&format!("\n\n(并入指纹: {})", carried_fps.join(" ")));
            }
            if !union_refs.is_empty() {
                entry.extras.retain(|(k, _)| k != "refs");
                entry.extras.push(("refs".into(), union_refs.join(" ")));
            }
            self.write_entry(&entry, Some(&path))?;
            merged = entry;
        }
        for id in duplicates {
            let (path, mut entry) = self
                .load_all()
                .into_iter()
                .find(|(_, e)| &e.id == id)
                .expect("checked above");
            entry.status = "deprecated".into();
            entry.updated = super::today();
            entry
                .extras
                .retain(|(k, _)| !k.eq_ignore_ascii_case("superseded_by"));
            entry
                .extras
                .push(("superseded_by".into(), primary.to_string()));
            self.write_entry(&entry, Some(&path))?;
        }
        self.refresh_derived()?;
        Ok(merged)
    }

    /// 完整性检测(D-112 同款哲学):ID 重复、缺号、解析失败的文件都要可见。
    pub fn integrity_issues(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let entries = self.load_all();
        let mut seen = std::collections::BTreeMap::new();
        for (path, e) in &entries {
            if let Some(previous) = seen.insert(e.id.clone(), path.clone()) {
                issues.push(format!(
                    "duplicate id {}: {} and {}",
                    e.id,
                    previous.display(),
                    path.display()
                ));
            }
        }
        let prefix = self.scope.prefix();
        let parse = |id: &str| {
            id.strip_prefix(prefix)?
                .strip_prefix('-')?
                .parse::<u32>()
                .ok()
        };
        let archived = self.load_archived_ids();
        let voided = self.voided_ids();
        let mut numbers: std::collections::BTreeSet<u32> = entries
            .iter()
            .map(|(_, e)| e.id.as_str())
            .chain(archived.iter().map(String::as_str))
            .filter_map(parse)
            .collect();
        // D-321:登记在 voided-ids.md 里的编号是"已交代的缺号",不再是账实不符。
        numbers.extend(voided.keys().copied());
        // 登记为 voided 的编号如果又出现了条目(手工改号/恢复),账实不符,必须可见。
        for (number, reason) in &voided {
            let alive = entries.iter().any(|(_, e)| parse(&e.id) == Some(*number))
                || archived.iter().any(|id| parse(id) == Some(*number));
            if alive {
                issues.push(format!(
                    "{prefix}-{number:03} recorded as voided ({reason}) but an entry exists — \
                     delete the voided-ids.md line or renumber the entry"
                ));
            }
        }
        if let Some(&max) = numbers.iter().max() {
            let missing: Vec<String> = (1..=max)
                .filter(|n| !numbers.contains(n))
                .map(|n| format!("{prefix}-{n:03}"))
                .collect();
            if !missing.is_empty() {
                // D-321:文案必须诚实——只有目录真在 git 版本控制下才指引 restore from git;
                // 否则给出可执行的处置(检查回收站/备份,或登记 voided-ids.md 注销)。
                let hint = if self.under_git() {
                    "data loss? restore from git; or acknowledge the gap via voided-ids.md"
                } else {
                    "data loss (no git backup) — check recycle bin/backup, or acknowledge \
                     the gap via voided-ids.md"
                };
                issues.push(format!("MISSING ids ({hint}): {}", missing.join(", ")));
            }
        }
        issues
    }

    /// id 字符串 → 编号(M-042 → 42)。不符合本 scope 前缀返回 None。
    fn id_number(&self, id: &str) -> Option<u32> {
        let prefix = self.scope.prefix();
        id.strip_prefix(prefix)?
            .strip_prefix('-')?
            .parse::<u32>()
            .ok()
    }

    /// voided 台账文件(D-321 注销通道):删除/丢失的编号登记在此,integrity 不再当
    /// 账实不符报 MISSING;与 markdown 条目同哲学——人可编辑,行格式 `- M-xxx: 理由`。
    pub(crate) fn voided_ledger_file(&self) -> PathBuf {
        self.root.join("voided-ids.md")
    }

    /// 已注销编号 → 理由。解析宽容:认不出的行忽略。
    pub fn voided_ids(&self) -> std::collections::BTreeMap<u32, String> {
        let mut out = std::collections::BTreeMap::new();
        let Ok(text) = std::fs::read_to_string(self.voided_ledger_file()) else {
            return out;
        };
        for line in text.lines() {
            let Some(body) = line.trim().strip_prefix("- ") else {
                continue;
            };
            let Some((id, reason)) = body.split_once(':') else {
                continue;
            };
            if let Some(number) = self.id_number(id.trim()) {
                out.insert(number, reason.trim().to_string());
            }
        }
        out
    }

    /// 主动注销一个缺失编号(如误删后确认无法恢复)。理由必填,且该编号当前必须真的
    /// 不存在于活动/归档——拿它去"清掉"一个还活着的条目是删数据,不是记账。
    pub fn void_id(&self, id: &str, reason: &str) -> anyhow::Result<()> {
        // D-368:voided-ids.md 是记忆树内文件,写入口同样持树锁。
        let _tree_lock = self.tree_lock()?;
        let reason = reason.trim();
        if reason.len() < 4 {
            anyhow::bail!("废弃编号必须写明理由(为什么这个号不该有条目、依据是什么)");
        }
        let Some(number) = self.id_number(id) else {
            anyhow::bail!("`{id}` 不是 {} 前缀的合法编号", self.scope.prefix());
        };
        if self.load_all().iter().any(|(_, e)| e.id == id)
            || self.load_archived_ids().iter().any(|a| a == id)
        {
            anyhow::bail!(
                "{id} 仍存在于活动或归档中,不能作为空洞注销;要终结它请用 memory_deprecate/清理流程"
            );
        }
        if self.voided_ids().contains_key(&number) {
            return Ok(());
        }
        let path = self.voided_ledger_file();
        let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            format!(
                "# {} Memory ID Ledger\n\n引擎维护:记录被主动废弃的编号及理由。\n\
                 缺号只有登记在此才算已交代;其余缺号 = 账实不符,必须查清。\n",
                self.scope.label()
            )
        });
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("- {id}: {reason}\n"));
        std::fs::write(&path, text)?;
        Ok(())
    }

    /// 记忆根目录(或任一祖先,最多上溯 8 层)是否在 git 版本控制下——决定 MISSING
    /// 文案能否指引 restore from git(D-321:U-001~004 目录不在版本控制却提示 git 恢复,误导)。
    fn under_git(&self) -> bool {
        let mut dir: Option<&std::path::Path> = Some(&self.root);
        for _ in 0..8 {
            let Some(d) = dir else {
                break;
            };
            if d.join(".git").exists() {
                return true;
            }
            dir = d.parent();
        }
        false
    }
}
