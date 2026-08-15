//! 校验域(R-257 B3):编号账本(ledger_file/voided_ids/void_id/restore_entry/
//! id_number)、完整性检测(integrity_issues)、状态流转校验(transition_allowed)、
//! 游离行读删(raw_lines/delete_raw_line)。以扩展 impl DocStore 定义。
//! 自 docstore.rs 原样迁出,零行为变更。

use std::path::PathBuf;

use super::model::{Entry, RawLine};
use super::parse::TemplateLine;
use super::render::render_with_template;
use super::repository::DocStore;

impl DocStore {
    /// 编号账本:`<stem>-ids.md`,记录被**主动废弃**的编号及理由。
    ///
    /// 缺号本身只说明"这个号现在没有条目",不等于数据丢失——分配后又撤销、
    /// 手工整理时合并掉重复条目,都会留下合法空洞。把两者混为一谈的后果实测过
    /// (D-173 复盘):完整性门禁把合法空洞判成丢失,又不提供安全的交代通道,
    /// 模型只好伪造一个 `[wontfix]` 墓碑去骗过门禁,反而污染了真实缺陷统计。
    pub fn ledger_file(&self) -> PathBuf {
        match self.path.file_stem().and_then(|s| s.to_str()) {
            Some(stem) => self.path.with_file_name(format!("{stem}-ids.md")),
            None => self.path.with_extension("ids.md"),
        }
    }

    /// 已废弃编号 → 理由。解析宽容:`- D-171: 理由` 形式,认不出的行忽略。
    pub fn voided_ids(&self) -> std::collections::BTreeMap<u32, String> {
        let mut out = std::collections::BTreeMap::new();
        let Ok(text) = std::fs::read_to_string(self.ledger_file()) else {
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

    /// 主动废弃一个编号。理由必填,且该编号当前必须真的不存在于活动/归档——
    /// 拿它去"清掉"一个还活着的条目是删数据,不是记账。
    pub fn void_id(&self, id: &str, reason: &str) -> std::io::Result<()> {
        // "该编号当前不存在于活动/归档"这个前置校验,与随后的账本追加必须是
        // 一笔原子事务:中间被别人插入一条同号条目,账本就会记下与事实相反的一行。
        let _lock = self.lock()?;
        let invalid =
            |message: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, message);
        let reason = reason.trim();
        if reason.len() < 4 {
            return Err(invalid(
                "废弃编号必须写明理由(为什么这个号不该有条目、依据是什么)".into(),
            ));
        }
        let Some(number) = self.id_number(id) else {
            return Err(invalid(format!(
                "`{id}` 不是 {} 前缀的合法编号",
                self.kind.prefix
            )));
        };
        if self.load()?.iter().any(|entry| entry.id == id)
            || self.load_archive()?.iter().any(|entry| entry.id == id)
        {
            return Err(invalid(format!(
                "{id} 仍存在于活动或归档文档中,不能作为空洞注销;要终结它请用 close/archive"
            )));
        }
        if self.voided_ids().contains_key(&number) {
            return Ok(());
        }
        let path = self.ledger_file();
        let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            format!(
                "# {} ID Ledger\n\n引擎维护:记录被主动废弃的编号及理由。\n\
                 缺号只有登记在此才算已交代;其余缺号 = 账实不符,必须查清。\n",
                self.kind.heading
            )
        });
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("- {id}: {reason}\n"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // 账本同样整读整写:它是缺号"已交代"的唯一凭据,读到半截等于凭据消失,
        // 完整性门禁会立刻把合法空洞判成账实不符。
        crate::atomic_file::write_atomic(&path, &text)
    }

    /// 在指定编号处补回一条丢失的条目(从 git 历史捞回来后落盘)。
    /// 只允许补真正的空洞,并且按编号插回原位——ID 顺序即分配顺序。
    pub fn restore_entry(&self, entry: Entry) -> std::io::Result<()> {
        // 「是不是空洞」的判定与插回落盘之间不能被别人插进来。
        let _lock = self.lock()?;
        let invalid =
            |message: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, message);
        let Some(number) = self.id_number(&entry.id) else {
            return Err(invalid(format!(
                "`{}` 不是 {} 前缀的合法编号",
                entry.id, self.kind.prefix
            )));
        };
        let mut entries = self.load()?;
        if entries.iter().any(|e| e.id == entry.id)
            || self.load_archive()?.iter().any(|e| e.id == entry.id)
        {
            return Err(invalid(format!("{} 已存在,不是空洞", entry.id)));
        }
        if self.voided_ids().contains_key(&number) {
            return Err(invalid(format!(
                "{} 已登记为主动废弃,先从 {} 里删掉那一行再补条目",
                entry.id,
                self.ledger_file().display()
            )));
        }
        let position = entries
            .iter()
            .position(|e| self.id_number(&e.id).is_some_and(|n| n > number))
            .unwrap_or(entries.len());
        entries.insert(position, entry);
        self.save(&entries)
    }

    fn id_number(&self, id: &str) -> Option<u32> {
        id.strip_prefix(self.kind.prefix)?
            .strip_prefix('-')?
            .parse::<u32>()
            .ok()
    }

    /// 数据完整性检测(D-112 / D-173):同一 ID 同时出现在活动与归档 = 归档半途而废;
    /// 活动∪归档∪废弃账本之外的缺号 = **账实不符**,必须查清后二选一交代掉。
    ///
    /// 注意措辞:缺号不等于"已确认的数据丢失"。ID 由引擎顺序分配,缺号说明这个号
    /// 曾被分配却没有条目,可能是丢了,也可能是合法撤销——工具无法从文件本身分辨,
    /// 所以只报"未交代",并同时给出两条**结构化**的合法出路(补回 / 注销),
    /// 而不是逼模型伪造一个墓碑条目来消音。
    pub fn integrity_issues(&self, active: &[Entry]) -> Vec<String> {
        let archived = self.load_archive().unwrap_or_default();
        let parse_num = |id: &str| {
            id.strip_prefix(self.kind.prefix)
                .and_then(|rest| rest.strip_prefix('-'))
                .and_then(|num| num.parse::<u32>().ok())
        };
        let active_ids: std::collections::BTreeSet<u32> =
            active.iter().filter_map(|e| parse_num(&e.id)).collect();
        let archive_ids: std::collections::BTreeSet<u32> =
            archived.iter().filter_map(|e| parse_num(&e.id)).collect();
        let voided = self.voided_ids();
        let mut issues = Vec::new();
        let both: Vec<u32> = active_ids.intersection(&archive_ids).copied().collect();
        if !both.is_empty() {
            issues.push(format!(
                "present in BOTH active and archive (incomplete archive?): {}",
                format_ids(self.kind.prefix, &both)
            ));
        }
        // 账本里登记为废弃、却又真的存在条目:账实不符的另一半,同样要报。
        let resurrected: Vec<u32> = voided
            .keys()
            .filter(|n| active_ids.contains(n) || archive_ids.contains(n))
            .copied()
            .collect();
        if !resurrected.is_empty() {
            issues.push(format!(
                "recorded as voided in {} but an entry exists: {} — delete the ledger line or renumber the entry",
                self.ledger_file().display(),
                format_ids(self.kind.prefix, &resurrected)
            ));
        }
        let Some(max) = active_ids
            .iter()
            .chain(archive_ids.iter())
            .chain(voided.keys())
            .max()
            .copied()
        else {
            return issues;
        };
        let missing: Vec<u32> = (1..=max)
            .filter(|n| {
                !active_ids.contains(n) && !archive_ids.contains(n) && !voided.contains_key(n)
            })
            .collect();
        if !missing.is_empty() {
            issues.push(format!(
                "UNACCOUNTED ids — absent from the active file, the archive AND the void ledger: {}. \
                 An engine-allocated id with no entry is either lost data or a withdrawn allocation, \
                 and this file cannot tell which. Settle each one: recover it \
                 (`git log -S \"## <id>\" -- {}` then `repair_missing_id`), or record why it was \
                 withdrawn (`void_id` with a reason). Do NOT invent a placeholder entry to silence \
                 this — that corrupts the real statistics.",
                format_ids(self.kind.prefix, &missing),
                self.kind.rel_path,
            ));
        }
        issues
    }

    /// 状态流转校验:前进(列表序)或进终态;后退/未知状态拒绝。
    pub fn transition_allowed(&self, from: &str, to: &str) -> Result<(), String> {
        let idx = |s: &str| self.kind.statuses.iter().position(|x| *x == s);
        let Some(to_idx) = idx(to) else {
            return Err(format!(
                "unknown status `{to}`; valid: {}",
                self.kind.statuses.join(" → ")
            ));
        };
        if self.kind.terminal.contains(&to) {
            return Ok(());
        }
        // 双向类型(目标):非终态之间自由往返(active⇄paused)。
        if self.kind.bidirectional {
            return Ok(());
        }
        match idx(from) {
            Some(from_idx) if to_idx >= from_idx => Ok(()),
            Some(_) => Err(format!(
                "cannot move backward `{from}` → `{to}`; forward only ({}). Hand-edit the markdown if you really need to reopen.",
                self.kind.statuses.join(" → ")
            )),
            // 用户手改出的未知状态:宽容,允许任意流转。
            None => Ok(()),
        }
    }

    /// R-201:某条目的游离行——解析时落在 `TemplateLine::Raw` 的行,字段体系外、
    /// 任何 update 都触及不到的历史内容。返回条目内从 1 起的稳定序号与原文,
    /// 序号即删除动作的键。读路径:依赖上次 `load()` 保存的模板。
    pub fn raw_lines(&self, id: &str) -> Vec<RawLine> {
        let preserved = self.preserved.lock().unwrap().clone();
        let Some(template) = preserved else {
            return Vec::new();
        };
        let Some(entry) = template.entries.iter().find(|e| e.id == id) else {
            return Vec::new();
        };
        entry
            .lines
            .iter()
            .filter_map(|line| match line {
                TemplateLine::Raw(text) => Some(text.clone()),
                TemplateLine::Field(_) => None,
            })
            .enumerate()
            .map(|(index, text)| RawLine {
                ordinal: index + 1,
                text,
            })
            .collect()
    }

    /// R-201:按序号删除一条游离行。只从模板里移除那一条 Raw,字段与其余行
    /// 一字不动(渲染仍走 `render_with_template`,模板里只剩没删的行)。
    ///
    /// 删除后必须把**修改后的模板**写回 preserved——否则同进程内下一次 save()
    /// 会拿着旧模板把刚删掉的行又吐回来,「删了等于没删」(幂等③)。
    pub fn delete_raw_line(&self, id: &str, ordinal: usize) -> std::io::Result<()> {
        let _lock = self.lock()?;
        if ordinal == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "ordinal 从 1 开始",
            ));
        }
        let entries = self.load()?;
        let mut template = self.preserved.lock().unwrap().clone().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "没有可用的模板")
        })?;
        let Some(entry_template) = template.entries.iter_mut().find(|e| e.id == id) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{id} 不存在或没有可清理的模板"),
            ));
        };
        // 定位第 ordinal 条 Raw 在 lines 里的下标:只数 Raw,Field 不占号。
        let mut seen = 0usize;
        let mut target = None;
        for (index, line) in entry_template.lines.iter().enumerate() {
            if let TemplateLine::Raw(_) = line {
                seen += 1;
                if seen == ordinal {
                    target = Some(index);
                    break;
                }
            }
        }
        let Some(index) = target else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{id} 只有 {seen} 条游离行,没有第 {ordinal} 条"),
            ));
        };
        entry_template.lines.remove(index);
        // 更新 preserved:同一实例后续 save() 必须基于删过的模板渲染。
        *self.preserved.lock().unwrap() = Some(template.clone());
        let text = render_with_template(self.kind, &entries, &template);
        crate::atomic_file::write_atomic(&self.path, &text)
    }
}

fn format_ids(prefix: &str, numbers: &[u32]) -> String {
    const SHOWN: usize = 10;
    let mut out: Vec<String> = numbers
        .iter()
        .take(SHOWN)
        .map(|n| format!("{prefix}-{n:03}"))
        .collect();
    if numbers.len() > SHOWN {
        out.push(format!("+{} more", numbers.len() - SHOWN));
    }
    out.join(", ")
}
