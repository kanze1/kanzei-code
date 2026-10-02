//! 归档域(R-257 B3):终态条目归档(archive_terminal)、归档读取与缓存
//! (load_archive/ARCHIVE_CACHE)、历史修复(repair_reused_archived_id/
//! correct_archived_terminal/dedupe_archived_fields/fill_archived_placeholder/
//! normalize_archive)。以扩展 impl DocStore 定义。自 docstore.rs 原样迁出,
//! 零行为变更。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::model::Entry;
use super::parse::{
    clean_tracker_title, parse_document, DocumentTemplate, ParsedDocument, TemplateLine,
};
use super::render::render_with_template;
use super::repository::DocStore;

/// D-377:归档解析缓存,见 [`DocStore::load_archive`]。键 = 路径 → (mtime, 长度, 解析结果)。
/// 条目数上界 = 项目数 × 归档种类数,不需要淘汰策略。
type ArchiveStamp = (std::time::SystemTime, u64);
#[allow(clippy::type_complexity)]
static ARCHIVE_CACHE: Mutex<Option<HashMap<PathBuf, (ArchiveStamp, ParsedDocument)>>> =
    Mutex::new(None);

fn archive_cache_get(path: &Path, stamp: ArchiveStamp) -> Option<ParsedDocument> {
    let cache = ARCHIVE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let (cached_stamp, parsed) = cache.as_ref()?.get(path)?;
    (*cached_stamp == stamp).then(|| parsed.clone())
}

fn archive_cache_put(path: &Path, stamp: ArchiveStamp, parsed: &ParsedDocument) {
    let mut cache = ARCHIVE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .get_or_insert_with(HashMap::new)
        .insert(path.to_path_buf(), (stamp, parsed.clone()));
}

impl DocStore {
    /// D-377:按 (mtime, 长度) 命中解析缓存。
    ///
    /// 归档是**只增不改**的大文件(本仓 defects-archive 699KB/367 条、
    /// requirements-archive 522KB/244 条),实测解析一遍 4.9 + 3.3 = 8.2ms;
    /// 而 `docs_snapshot` 每次刷新、每轮 `kz:done`、每次勾选都要读它一遍,
    /// 只为算依赖状态。文件没动就没必要重新分词。
    ///
    /// 键用 (mtime, 长度) 而不是内容 hash:hash 要先把 1.2MB 读进来,那正是要省的。
    /// 归档只被 append/rewrite,两者都同时改这两个量;取不到元数据就不缓存。
    pub fn load_archive(&self) -> std::io::Result<Vec<Entry>> {
        let path = self.archive_file();
        let stamp = std::fs::metadata(&path)
            .ok()
            .and_then(|meta| Some((meta.modified().ok()?, meta.len())));
        if let Some(stamp) = stamp {
            if let Some(parsed) = archive_cache_get(&path, stamp) {
                *self.preserved_archive.lock().unwrap() = Some(parsed.template.clone());
                return Ok(parsed.entries);
            }
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let parsed = parse_document(self.kind, &text);
                *self.preserved_archive.lock().unwrap() = Some(parsed.template.clone());
                if let Some(stamp) = stamp {
                    archive_cache_put(&path, stamp, &parsed);
                }
                Ok(parsed.entries)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// 修复“历史归档 ID 被后来的活动条目复用”：保留活动条目的当前 ID，
    /// 把语义不同的历史归档条目迁到下一个未使用 ID。若两份内容相同，说明更像
    /// 归档半途而废，不能靠改号掩盖，应人工判断哪一份才该保留。
    pub fn repair_reused_archived_id(&self, id: &str) -> std::io::Result<String> {
        // 改号要基于「读到的那一版」活动+归档,整段必须在锁内。
        let _lock = self.lock()?;
        let active = self.load()?;
        let mut archived = self.load_archive()?;
        let Some(active_entry) = active.iter().find(|entry| entry.id == id) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{id} 不在活动文档中"),
            ));
        };
        let Some(archived_pos) = archived.iter().position(|entry| entry.id == id) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{id} 不在归档文档中"),
            ));
        };
        if active_entry == &archived[archived_pos] {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{id} 的活动与归档内容相同，疑似未完成归档，拒绝自动改号"),
            ));
        }
        let issues = self.integrity_issues(&active);
        if issues.len() != 1 || !issues[0].contains(id) || issues[0].contains(',') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("除 {id} 复用外仍有其他完整性问题: {}", issues.join("; ")),
            ));
        }

        let new_id = self.next_id(&active);
        let archived_entry = &mut archived[archived_pos];
        archived_entry.id = new_id.clone();
        archived_entry.title = archived_entry.title.replace(id, &new_id);
        for (_, value) in &mut archived_entry.fields {
            *value = value.replace(id, &new_id);
        }

        let mut template = self
            .preserved_archive
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "归档模板未加载")
            })?;
        let entry_template = template
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("归档模板中找不到 {id}"),
                )
            })?;
        entry_template.id = new_id.clone();
        for line in &mut entry_template.lines {
            if let TemplateLine::Raw(text) = line {
                *text = text.replace(id, &new_id);
            }
        }

        let text = render_with_template(self.kind, &archived, &template).replacen(
            &format!("# {}\n", self.kind.heading),
            &format!("# {} Archive\n", self.kind.heading),
            1,
        );
        crate::atomic_file::write_atomic(&self.archive_file(), &text)?;
        *self.preserved_archive.lock().unwrap() = Some(template);
        Ok(new_id)
    }

    /// 终态条目移入归档文件(追加,幂等):活跃文件只留进行中的,前端与
    /// 上下文注入都不再被完成项干扰;历史仍可随时翻(get 会回落到归档)。
    /// 返回被移动的条目 ID——调用方必须能告知"哪些条目去了哪个文件"(D-112)。
    ///
    /// D-316:写回前对**整个归档**做净化——按 id 去重(保留先归档的那份)与
    /// 每条目字段收敛(同 (key,value) 去重、删空 `阻塞`;口径详见
    /// normalize_archive 的 D-328 说明)。历史脏数据(重复条目 D-309、
    /// 误切进归档的孤儿字段 D-289)会在任意一次归档动作时被收敛;净化有变化
    /// 时即使没有新终态条目也强制写回(archived 动作 = 清理通道)。
    pub fn archive_terminal(&self) -> std::io::Result<Vec<String>> {
        // 事务锁必须罩住 load:两个进程各自 load 到同一份活动条目、各自算出同一批
        // 终态条目、再各自写归档,归档里就会出现重复条目。
        let _lock = self.lock()?;
        let entries = self.load()?;
        let (terminal, live): (Vec<Entry>, Vec<Entry>) = entries
            .into_iter()
            .partition(|e| self.kind.terminal.contains(&e.status.as_str()));
        let mut archived = self.load_archive()?;
        // D-316 净化:按 id 去重(保留先归档)+ 条目内字段收敛(D-328 口径)。
        let before_len = archived.len();
        archived = Self::normalize_archive(archived);
        let cleaned = archived.len() != before_len;
        if terminal.is_empty() && !cleaned {
            return Ok(Vec::new());
        }
        let moved: Vec<String> = terminal.iter().map(|e| e.id.clone()).collect();
        let active_template = self.preserved.lock().unwrap().clone();
        let mut archive_template =
            self.preserved_archive
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(DocumentTemplate {
                    preamble: Vec::new(),
                    entries: Vec::new(),
                });
        if let Some(active_template) = active_template {
            for entry in &terminal {
                if archive_template
                    .entries
                    .iter()
                    .all(|template| template.id != entry.id)
                {
                    if let Some(template) = active_template
                        .entries
                        .iter()
                        .find(|template| template.id == entry.id)
                    {
                        archive_template.entries.push(template.clone());
                    }
                }
            }
        }
        // D-316:Entry 列表按 id 去重(模板去重只保证渲染不重复,列表本身
        // 会累积同 id——实测 D-309 两份)。保留先归档的那份。
        let mut seen_ids: std::collections::HashSet<String> =
            archived.iter().map(|e| e.id.clone()).collect();
        for entry in terminal {
            if seen_ids.insert(entry.id.clone()) {
                archived.push(entry);
            }
        }
        let archived_text = render_with_template(self.kind, &archived, &archive_template);
        let text = archived_text.replacen(
            &format!("# {}\n", self.kind.heading),
            &format!("# {} Archive\n", self.kind.heading),
            1,
        );
        if let Some(parent) = self.archive_file().parent() {
            std::fs::create_dir_all(parent)?;
        }
        // 写序不可调换:**先写归档、再删活动**。原子写只保证单个文件不被读成半截,
        // 保证不了两个文件之间的原子性——两步之间崩溃时,当前顺序留下的是"条目
        // 同时在两处"(integrity_issues 已能报、可人工收口),反过来留下的是
        // "两处都没有"= 真丢数据。谁想"顺手"把 save 提前,先看这段(D-112)。
        crate::atomic_file::write_atomic(&self.archive_file(), &text)?;
        self.save(&live)?;
        Ok(moved)
    }

    /// D-331:受限的归档终态纠错——只允许在当前 DocKind 的终态集合内改(fixed↔wontfix
    /// 等),必须写明 reason(追加进进展作审计),条目保持在归档、原子写入,标题里的
    /// 跨 DocKind 状态标记一并清除(那是历史写入口校验缺失时混进标题的污染,
    /// 如 D-267 的 [dropped])。返回 (old_status, new_status)。
    pub fn correct_archived_terminal(
        &self,
        id: &str,
        new_status: &str,
        reason: &str,
    ) -> std::io::Result<(String, String)> {
        // 事务锁罩住 load:并发纠错不能各自读到旧归档再互相覆盖。
        let _lock = self.lock()?;
        let mut archived = self.load_archive()?;
        let Some(pos) = archived.iter().position(|e| e.id == id) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("`{id}` not found in the archive"),
            ));
        };
        if !self.kind.terminal.contains(&new_status) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "`{new_status}` is not a terminal status for `{}`; valid: {}",
                    self.kind.prefix,
                    self.kind.terminal.join(" | ")
                ),
            ));
        }
        let reason = reason.trim();
        if reason.len() < 4 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "terminal correction requires a reason explaining the change",
            ));
        }
        let old_status = archived[pos].status.clone();
        let cleaned_title = clean_tracker_title(self.kind, &archived[pos].title);
        let note = format!(
            "[terminal-fix {}] {} → {}: {}",
            crate::memory::today(),
            old_status,
            new_status,
            reason
        );
        let entry = &mut archived[pos];
        entry.status = new_status.to_string();
        entry.title = cleaned_title;
        // 生命周期的权威值是 header status。历史直写可能留下「状态: done」等
        // 与 header 冲突的副本(D-569);修正归档时移除这些保留字段,避免继续污染取活依据。
        entry
            .fields
            .retain(|(key, _)| !key.eq_ignore_ascii_case("status") && key != "状态");
        // D-333:审计进展**合并**进既有「进展」字段,而不是 push 第二条——归档区
        // 条目大多已带原始进展,fix_terminal 再 push 一条会形成重复「进展」字段
        // (normalize 扫描实测检出 R-201/R-198/R-199/R-213/R-225/R-226 六条)。
        // 口径与 tracker.rs append_progress 一致:有则换行追加,无则新建。
        match entry.fields.iter_mut().find(|(key, _)| key == "进展") {
            Some((_, slot)) => {
                slot.push('\n');
                slot.push_str(&note);
            }
            None => entry.fields.push(("进展".into(), note)),
        }
        let template = self
            .preserved_archive
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(DocumentTemplate {
                preamble: Vec::new(),
                entries: Vec::new(),
            });
        let archived_text = render_with_template(self.kind, &archived, &template);
        let text = archived_text.replacen(
            &format!("# {}\n", self.kind.heading),
            &format!("# {} Archive\n", self.kind.heading),
            1,
        );
        if let Some(parent) = self.archive_file().parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::atomic_file::write_atomic(&self.archive_file(), &text)?;
        Ok((old_status, new_status.to_string()))
    }

    /// D-333:归档条目字段去重。归档写通道不公开整表保存,但 D-333 验收③要求
    /// 归档重复字段能收敛——这里提供一个**定向**归档字段修复,与
    /// correct_archived_terminal 共用同一把锁与写路径(load_archive → 改 →
    /// render_with_template → write_atomic),不制造第二套整表写 API。
    /// 去重口径:同 key(大小写不敏感)保留首条;「进展」例外——重复的进展
    /// **合并内容**(换行连接),因为进展是审计流水,丢任何一条都破坏证据链
    /// (fix_terminal 追加的 [terminal-fix] 与原始进展都必须保留)。
    /// 返回 (是否真的去重了, 去除的字段数)。
    pub fn dedupe_archived_fields(&self, id: &str) -> std::io::Result<(bool, usize)> {
        let _lock = self.lock()?;
        let mut archived = self.load_archive()?;
        let Some(pos) = archived.iter().position(|e| e.id == id) else {
            return Ok((false, 0));
        };
        let mut kept: Vec<(String, String)> = Vec::new();
        let mut removed = 0usize;
        for (key, value) in archived[pos].fields.drain(..) {
            let norm = key.trim().to_ascii_lowercase();
            if let Some((kept_key, kept_value)) = kept
                .iter_mut()
                .find(|(k, _)| k.trim().to_ascii_lowercase() == norm)
            {
                removed += 1;
                // 进展合并内容,其余保留首条。
                if kept_key.eq_ignore_ascii_case("进展") {
                    kept_value.push('\n');
                    kept_value.push_str(&value);
                }
            } else {
                kept.push((key, value));
            }
        }
        archived[pos].fields = kept;
        if removed == 0 {
            return Ok((false, 0));
        }
        let template = self
            .preserved_archive
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(DocumentTemplate {
                preamble: Vec::new(),
                entries: Vec::new(),
            });
        let archived_text = render_with_template(self.kind, &archived, &template);
        let text = archived_text.replacen(
            &format!("# {}\n", self.kind.heading),
            &format!("# {} Archive\n", self.kind.heading),
            1,
        );
        crate::atomic_file::write_atomic(&self.archive_file(), &text)?;
        Ok((true, removed))
    }

    /// D-713:归档区同样只允许 header 保存生命周期状态。删除正文 `状态`/`status`
    /// 副本前,把旧值追加到进展,避免把历史纠正变成无来源的静默删字段。
    pub fn reconcile_archived_status_fields(&self, id: &str) -> std::io::Result<(bool, usize)> {
        let _lock = self.lock()?;
        let mut archived = self.load_archive()?;
        let Some(pos) = archived.iter().position(|entry| entry.id == id) else {
            return Ok((false, 0));
        };
        let authoritative = archived[pos].status.clone();
        let mut old_values = Vec::new();
        archived[pos].fields.retain(|(key, value)| {
            let reserved = key.eq_ignore_ascii_case("status") || key == "状态";
            if reserved {
                if !value.trim().is_empty() {
                    old_values.push(value.trim().to_string());
                }
                false
            } else {
                true
            }
        });
        if old_values.is_empty() {
            return Ok((false, 0));
        }
        let old = old_values.join("、");
        let note = if old_values.iter().any(|value| value != &authoritative) {
            format!(
                "状态对账: 归档正文旧字段 `{old}` 与权威标题状态 `{authoritative}` 冲突;已移除正文副本。"
            )
        } else {
            format!(
                "状态对账: 归档正文旧字段 `{old}` 与权威标题状态 `{authoritative}` 重复;已移除正文副本。"
            )
        };
        match archived[pos]
            .fields
            .iter_mut()
            .find(|(key, _)| key == "进展")
        {
            Some((_, progress)) => {
                if !progress.is_empty() {
                    progress.push('；');
                }
                progress.push_str(&note);
            }
            None => archived[pos].fields.push(("进展".into(), note)),
        }
        let template = self
            .preserved_archive
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(DocumentTemplate {
                preamble: Vec::new(),
                entries: Vec::new(),
            });
        let archived_text = render_with_template(self.kind, &archived, &template);
        let text = archived_text.replacen(
            &format!("# {}\n", self.kind.heading),
            &format!("# {} Archive\n", self.kind.heading),
            1,
        );
        crate::atomic_file::write_atomic(&self.archive_file(), &text)?;
        Ok((true, old_values.len()))
    }

    /// 引擎每轮重算的机制产物字段——写进条目是走错门,清掉不留痕。
    ///
    /// 与 `reconcile_archived_status_fields` 的区别在**要不要写进展**:状态副本可能
    /// 与权威 header 冲突,冲突本身是可审计的历史;机制产物没有这种价值——它只是
    /// 一份必然过期的快照,引擎下一轮照样重算一遍,留一句"曾经写过"纯属噪音。
    /// 归档时自动清理的零消费者叙事键。它们与 `ENGINE_DERIVED_FIELDS` 分开登记：
    /// 前者是没有真实读取方的历史散文，后者是引擎每轮重算的快照。
    pub const ZERO_CONSUMER_FIELDS: &[&str] = &[
        "对账",
        "批次表",
        "背景",
        "根因",
        "执行者",
        "归属",
        "原始描述",
        "不变量",
    ];

    pub const ENGINE_DERIVED_FIELDS: &[&str] = &["取活依据"];

    /// 清掉归档条目里的机制产物字段。与 dedupe/reconcile 共用同一把锁与写路径,
    /// 不制造第二套整表写 API。
    pub fn drop_archived_engine_fields(&self, id: &str) -> std::io::Result<(bool, usize)> {
        let _lock = self.lock()?;
        let mut archived = self.load_archive()?;
        let Some(pos) = archived.iter().position(|entry| entry.id == id) else {
            return Ok((false, 0));
        };
        let before = archived[pos].fields.len();
        archived[pos]
            .fields
            .retain(|(key, _)| !Self::ENGINE_DERIVED_FIELDS.contains(&key.trim()));
        let removed = before - archived[pos].fields.len();
        if removed == 0 {
            return Ok((false, 0));
        }
        let template = self
            .preserved_archive
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(DocumentTemplate {
                preamble: Vec::new(),
                entries: Vec::new(),
            });
        let archived_text = render_with_template(self.kind, &archived, &template);
        let text = archived_text.replacen(
            &format!("# {}\n", self.kind.heading),
            &format!("# {} Archive\n", self.kind.heading),
            1,
        );
        crate::atomic_file::write_atomic(&self.archive_file(), &text)?;
        Ok((true, removed))
    }

    /// R-227:归档条目字段里的占位符测试 ID 回填。占位符形态 `T-<数字>xxx`
    /// (真实测试 ID 是 `T-<10位时间戳>`),曾出现在 R-198/R-199/D-219/D-266/D-279/
    /// D-281/D-282/D-316 关闭证据里。回填 = 把占位符替换为 test_record 落盘的真实 ID。
    /// 与 dedupe_archived_fields 共用同一把锁与写路径(load_archive → 改 →
    /// render_with_template → write_atomic),不制造第二套整表写 API。
    /// `old` 必须恰好命中一次(0 次=没找到,多次=有歧义),替换后返回真实替换次数。
    /// 要求 reason 非空,记录在返回里由调用方展示(不进条目,避免污染审计流水)。
    pub fn fill_archived_placeholder(
        &self,
        id: &str,
        old: &str,
        new: &str,
    ) -> std::io::Result<usize> {
        let _lock = self.lock()?;
        let mut archived = self.load_archive()?;
        let Some(pos) = archived.iter().position(|e| e.id == id) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no archived entry {id}"),
            ));
        };
        // 遍历全部字段值做替换,统计总命中次数(0=没找到,>1=有歧义)。
        let mut replaced = 0usize;
        for (_, value) in archived[pos].fields.iter_mut() {
            let mut count = 0usize;
            let mut rest = value.as_str();
            let mut parts = Vec::new();
            while let Some(idx) = rest.find(old) {
                parts.push(&rest[..idx]);
                parts.push(new);
                rest = &rest[idx + old.len()..];
                count += 1;
            }
            if count > 0 {
                replaced += count;
                parts.push(rest);
                *value = parts.concat();
            }
        }
        if replaced == 0 {
            return Ok(0);
        }
        if replaced > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("placeholder `{old}` matched {replaced} times in archived {id}; refuse ambiguous fill"),
            ));
        }
        let template = self
            .preserved_archive
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(DocumentTemplate {
                preamble: Vec::new(),
                entries: Vec::new(),
            });
        let archived_text = render_with_template(self.kind, &archived, &template);
        let text = archived_text.replacen(
            &format!("# {}\n", self.kind.heading),
            &format!("# {} Archive\n", self.kind.heading),
            1,
        );
        crate::atomic_file::write_atomic(&self.archive_file(), &text)?;
        Ok(replaced)
    }

    /// D-316 归档净化:按 id 去重(保留先归档的一份)+ 条目内字段收敛。
    ///
    /// D-328 收窄两条口径——净化的对象是"结构性脏数据",不是叙事内容:
    /// - 同 key 去重必须比对整个 (key, value):同名不同内容是合法叙事(同一条目
    ///   两行「验证(…)」各讲一次迁移),按 key 吃掉第二条就是删证据,实测吃掉了
    ///   D-179 系 v7 迁移的验证记录。
    /// - 空值只删 `阻塞`:多行字段的表头(`- 实测(…): `,值在续行 Raw 里)在字段
    ///   模型里同样是空值,删表头会让续行挂错归属。空 `阻塞` 是 D-289 确认的
    ///   结构垃圾;其余空字段宁可留着难看,也不替内容做主。
    fn normalize_archive(entries: Vec<Entry>) -> Vec<Entry> {
        let mut seen_ids = std::collections::HashSet::new();
        let mut out = Vec::new();
        for mut entry in entries {
            if !seen_ids.insert(entry.id.clone()) {
                continue; // 同 id 重复:保留先归档的那份
            }
            let mut seen_fields = std::collections::HashSet::new();
            entry.fields.retain(|(key, value)| {
                let key = key.trim();
                let value = value.trim();
                if key.is_empty()
                    || (value.is_empty() && key == "阻塞")
                    || Self::ENGINE_DERIVED_FIELDS.contains(&key)
                    || Self::ZERO_CONSUMER_FIELDS.contains(&key)
                {
                    return false;
                }
                seen_fields.insert((key.to_string(), value.to_string()))
            });
            out.push(entry);
        }
        out
    }
}
