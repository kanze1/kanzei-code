//! 结构化文档引擎:需求/缺陷/来源/发现 的统一底座。
//! 真源是纯 markdown(用户可任意编辑器手改,解析宽容);
//! 结构(ID 分配、状态机、格式)由本引擎在写入侧强制——文档永远写不坏。
//!
//! 条目格式:
//! ```markdown
//! ## R-001 标题 [doing] (high)
//! - 验收: ...
//! - refs: S-001 S-002
//! ```

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 跨 DocKind 的全部状态标记(D-331):标题里出现 `[X]`(X ∈ 此集合)就是把状态机
/// 标记写进了标题——header 渲染会变成 `[X] [status]` 双终态(如 D-267 的
/// `[dropped] [fixed]`),调度/统计/审计同时看到两个矛盾终态。
pub const ALL_STATUS_TOKENS: &[&str] = &[
    "todo",
    "doing",
    "done",
    "dropped", // requirements / findings
    "open",
    "fixing",
    "fixed",
    "wontfix", // defects
    "active",
    "archived",
    "paused",
    "achieved", // sources
    "inbox",
    "split", // ideas
    "draft",
    "confirmed",
    "accepted",
    "superseded",
    "rejected", // findings / decisions
];

/// 标题中是否含跨 DocKind 状态标记(形如 `[done]` / `[dropped]`,大小写不敏感)。
/// 状态的家是 header 方括号,不是标题——标题带状态标记即污染(D-331)。
pub fn title_status_marker(title: &str) -> Option<&'static str> {
    let lower = title.to_ascii_lowercase();
    ALL_STATUS_TOKENS
        .iter()
        .find(|tok| lower.contains(&format!("[{tok}]")))
        .copied()
}

/// 清除标题里的全部跨 DocKind 状态标记(D-331 纠错用):反复移除 `[token]`
/// (大小写不敏感)直到干净,再把多余空白折叠。只删标记,其余标题逐字保留。
pub fn strip_status_markers(title: &str) -> String {
    let mut out = title.to_string();
    loop {
        let lower = out.to_ascii_lowercase();
        let found = ALL_STATUS_TOKENS.iter().find_map(|tok| {
            let needle = format!("[{tok}]");
            lower.find(&needle).map(|idx| (idx, needle.len()))
        });
        match found {
            Some((idx, len)) => {
                out.replace_range(idx..idx + len, "");
            }
            None => break,
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, Copy)]
pub struct DocKind {
    /// 相对项目根,如 ".kanzei/project/requirements.md"。
    pub rel_path: &'static str,
    pub heading: &'static str,
    /// ID 前缀(R/D/S/F)。
    pub prefix: &'static str,
    /// 有序状态列表,首个为初始态。
    pub statuses: &'static [&'static str],
    /// 终态(close 的合法目标)。
    pub terminal: &'static [&'static str],
    pub severities: Option<&'static [&'static str]>,
    pub priorities: Option<&'static [&'static str]>,
    /// 标签受控词表(conventions §1.35 用户定调):None = 该文档不参与标签分类。
    /// 写入口校验:「标签:」值必须命中词表,词表外拒绝并提示合法值。
    pub tags: Option<&'static [&'static str]>,
    /// 非终态之间允许自由往返(目标 active⇄paused);false = 只进不退。
    pub bidirectional: bool,
    /// 允许经 `reopen` 退回初始态的非终态列表(空 = 不支持退回)。
    /// D-241:fixing 长期无人续推时没有合法退回通道,agent 只能手改 markdown 或
    /// 让僵尸条目永远占着「进行中」语义;reopen 把「推不动就退回」变成可执行动作。
    pub reopen_from: &'static [&'static str],
}

pub const REQUIREMENTS: DocKind = DocKind {
    rel_path: ".kanzei/project/requirements.md",
    heading: "Requirements",
    prefix: "R",
    statuses: &["todo", "doing", "done", "dropped"],
    terminal: &["done", "dropped"],
    severities: None,
    priorities: Some(&["P0", "P1", "P2", "P3"]),
    tags: Some(&["核心", "后端", "前端", "模型", "发布", "流程"]),
    bidirectional: false,
    reopen_from: &["doing"],
};

pub const DEFECTS: DocKind = DocKind {
    rel_path: ".kanzei/project/defects.md",
    heading: "Defects",
    prefix: "D",
    statuses: &["open", "fixing", "fixed", "wontfix"],
    terminal: &["fixed", "wontfix"],
    severities: Some(&["high", "medium", "low"]),
    priorities: Some(&["P0", "P1", "P2", "P3"]),
    tags: Some(&["核心", "后端", "前端", "模型", "发布", "流程"]),
    bidirectional: false,
    reopen_from: &["fixing"],
};

pub const SOURCES: DocKind = DocKind {
    rel_path: ".kanzei/research/sources.md",
    heading: "Sources",
    prefix: "S",
    statuses: &["active", "archived"],
    terminal: &["archived"],
    severities: None,
    priorities: None,
    tags: None,
    bidirectional: false,
    reopen_from: &[],
};

pub const FINDINGS: DocKind = DocKind {
    rel_path: ".kanzei/research/findings.md",
    heading: "Findings",
    prefix: "F",
    statuses: &["draft", "confirmed", "dropped"],
    terminal: &["confirmed", "dropped"],
    severities: None,
    priorities: None,
    tags: None,
    bidirectional: false,
    reopen_from: &[],
};

/// 跨会话记忆(R-098):记"已确认的事实与踩过的坑",与追踪文档职责分离——
/// 追踪文档记"要做什么/做到哪",记忆记"已经查清楚了什么"。
/// stale 是终态:被推翻或过期的结论 archive 出去,不再进入上下文。
pub const MEMORY: DocKind = DocKind {
    rel_path: ".kanzei/project/memory.md",
    heading: "Memory",
    prefix: "M",
    statuses: &["active", "stale"],
    terminal: &["stale"],
    severities: None,
    priorities: None,
    tags: None,
    // 结论可能被重新确认,允许 active⇄stale 往返。
    bidirectional: true,
    reopen_from: &[],
};

/// 设计决策(R-110):讨论与设计的沉淀——像需求/缺陷一样可追踪、可引用、可检索。
/// accepted 是常驻态(决策生效中);被新决策取代时 superseded 并归档,拒绝即 rejected。
pub const DECISIONS: DocKind = DocKind {
    rel_path: ".kanzei/project/decisions.md",
    heading: "Decisions",
    prefix: "A",
    statuses: &["draft", "accepted", "superseded", "rejected"],
    terminal: &["superseded", "rejected"],
    severities: None,
    priorities: None,
    tags: None,
    bidirectional: false,
    reopen_from: &[],
};

/// 原始想法收件箱(R-252):用户侧未经拆解的设计需求/想法,原样录入不过模型;
/// 由人点「拆解」派子代理转成 requirements/defects 条目。旧的长期目标线已于
/// 同批退役(R-252 验收②:现存条目推 dropped 并归档,全仓 grep 零残留)。
/// 状态机 inbox → split/dropped:split=已拆解出 R/D(转 split 有 refs 硬门禁,
/// 见 tracker actions::update_close),dropped=用户放弃。想法不是待办——
/// 取活引擎(work)与鞭挞推进指令都不看这条线。
pub const IDEAS: DocKind = DocKind {
    rel_path: ".kanzei/project/ideas.md",
    heading: "Ideas",
    prefix: "I",
    statuses: &["inbox", "split", "dropped"],
    terminal: &["split", "dropped"],
    severities: None,
    priorities: None,
    tags: None,
    bidirectional: false,
    reopen_from: &[],
};

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub status: String,
    pub severity: Option<String>,
    /// 自由字段(bullet),refs 也存这里(key = "refs")。
    pub fields: Vec<(String, String)>,
}

/// R-201:游离行的稳定标识——条目内从 1 起的序号 + 原文。
/// 序号是删除动作的键(`delete_raw_line` 用 ordinal),原文供删除前核对。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLine {
    pub ordinal: usize,
    pub text: String,
}

/// 单条目批次数上限(2026-08-10 用户定调:批数由 agent 按实际工作量自定,上限 10)。
/// 这是**写入侧**门禁的判据,读路径不做钳制——理由见 declared_batch_progress。
pub const MAX_BATCHES: u32 = 10;

/// 条目的批次进度 (已完成, 总数)。
///
/// 总数**只认条目自己声明的 `批次: k/N`**;没声明 = 不分批(1 格),既不画进度条
/// 也不受关闭门禁约束——批数由 agent 定,引擎不替他猜(2026-08-10 定调)。原先按
/// 复杂度给的固定默认值(中 3/大 8)已删:它经关闭门禁直接支配了没声明批次的中/大
/// 条目,让它们必然撞门(D-242 影响①)。
///
/// 判据只有这一份:UI 的格子与关闭门禁都从这里取,不要在前端再抄一张映射表。
pub fn batch_progress(entry: &Entry) -> (u32, u32) {
    match declared_batch_progress(entry) {
        // 显式声明优先:总数为 0 视为没声明,避免除零与"0/0 格"这种空表达。
        Some((done, total)) if total > 0 => (done.min(total), total),
        _ => (0, 1),
    }
}

/// 从条目字段读取手写的批次副本。`None` 表示未声明或格式无效。
///
/// 读路径故意**不按 MAX_BATCHES 钳制**,两条理由:①归档里真实存在 11/11、16/16 的
/// 条目,钳到 10 会把历史显示成假数;②关闭门禁走的也是这里——把声明的 12 钳成 10,
/// git 推导出 10 个批次标记时门禁就会放行两个根本没做的批次,硬门禁被静默降级成软
/// 提示。上限因此加在写入侧(check_declared_batches)。
pub fn declared_batch_progress(entry: &Entry) -> Option<(u32, u32)> {
    entry
        .fields
        .iter()
        .find(|(k, _)| k == "批次" || k.eq_ignore_ascii_case("batches"))
        .and_then(|(_, v)| parse_batches(v))
        .filter(|(_, total)| *total > 0)
}

/// Git 提交历史可用时，以它的已完成数覆盖手写副本；总批数仍只由条目声明决定。
pub fn batch_progress_with_derived_done(entry: &Entry, derived_done: Option<u32>) -> (u32, u32) {
    let (declared_done, total) = batch_progress(entry);
    let done = derived_done.unwrap_or(declared_done).min(total);
    (done, total)
}

/// 解析 `3/11`;宽容对待空格与全角斜杠(手写文档常见)。
fn parse_batches(raw: &str) -> Option<(u32, u32)> {
    let normalized = raw.replace('／', "/");
    let (done, total) = normalized.trim().split_once('/')?;
    Some((done.trim().parse().ok()?, total.trim().parse().ok()?))
}

/// 写入侧校验:条目**本次声明**批次时的唯一判据。
///
/// `existing_total` = 该条目**当前已有**的批次总数;新建(add)没有既有值,传 `None`。
/// 上限只约束「新声明或被抬高的总数」:本次总数**不高于既有值**就放行(哪怕既有值是
/// 11、16),高于既有值且超过上限才拒。理由:归档/存量里真实存在 11/11、16/16 的历史
/// 条目,`3/11` → `4/11` 是它们的**正常逐批推进**,不是新声明——拦下来只会逼 agent
/// 为了动一条历史条目去篡改它的总数;而抬高总数(`3/11` → `3/16`)是货真价实的新声明,
/// 必须撞门。新建没有既有值,按 `<= MAX_BATCHES` 严格约束。
///
/// 选择「拒绝并报错」而不是「钳到 10」:钳制既会改写归档里的真值,又会把关闭门禁
/// 静默放宽(声明 12 被钳成 10 时,做完 10 批就放行)。拒绝发生在写入当下,agent
/// 拿到的是可执行的出路(拆后续条目),而不是事后撞门。
///
/// 调用方必须只把**本次传入的字段值**喂进 `raw`,绝不能拿合并后的整条目来校验——
/// 否则归档里 11/11、16/16 的历史条目一旦被触碰就再也关不掉。既有值只作为上限的
/// 基准出现在 `existing_total` 里,不参与格式与 `done > total` 的判定。
pub fn check_declared_batches(
    raw: &str,
    existing_total: Option<u32>,
) -> Result<(u32, u32), String> {
    let Some((done, total)) = parse_batches(raw) else {
        return Err(format!("批次字段要写成 `k/N`(如 `0/3`),实际收到 `{raw}`。"));
    };
    if total == 0 {
        return Err("批次总数不能为 0;不分批就别写这个字段。".into());
    }
    if done > total {
        return Err(format!("已完成 {done} 超过总数 {total},先核对再写。"));
    }
    // 基准 = 既有总数(新建按 0 算)。只有把总数抬到基准之上、又越过上限,才是新声明。
    let baseline = existing_total.unwrap_or(0);
    if total > MAX_BATCHES && total > baseline {
        let 既有 = if baseline > MAX_BATCHES {
            format!(
                "该条目既有总数是 {baseline}(历史真值),照它原样往前推(只改已完成数)\
                 或改小总数都放行,抬到 {total} 不行。"
            )
        } else {
            String::new()
        };
        return Err(format!(
            "批数上限 {MAX_BATCHES}(2026-08-10 用户定调),实际声明 {total}。\
             批数由你按工作量定,但超过 {MAX_BATCHES} 批说明这个条目本身太大:\
             把能收口的部分做完关闭,剩下的开成后续条目,不要靠加批次把一条撑到底。{既有}"
        ));
    }
    Ok((done, total))
}

impl Entry {
    pub fn refs(&self) -> Vec<String> {
        self.fields
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("refs"))
            .flat_map(|(_, v)| v.split([' ', ',']))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }
}

#[derive(Debug, Clone)]
enum TemplateLine {
    Raw(String),
    Field(String),
}

#[derive(Debug, Clone)]
struct EntryTemplate {
    id: String,
    lines: Vec<TemplateLine>,
}

#[derive(Debug, Clone)]
struct DocumentTemplate {
    preamble: Vec<String>,
    entries: Vec<EntryTemplate>,
}

#[derive(Debug, Clone)]
struct ParsedDocument {
    entries: Vec<Entry>,
    template: DocumentTemplate,
}

/// D-377:归档解析缓存,见 [`DocStore::load_archive`]。键 = 路径 → (mtime, 长度, 解析结果)。
/// 条目数上界 = 项目数 × 归档种类数,不需要淘汰策略。
type ArchiveStamp = (std::time::SystemTime, u64);
#[allow(clippy::type_complexity)]
static ARCHIVE_CACHE: Mutex<
    Option<std::collections::HashMap<PathBuf, (ArchiveStamp, ParsedDocument)>>,
> = Mutex::new(None);

fn archive_cache_get(path: &std::path::Path, stamp: ArchiveStamp) -> Option<ParsedDocument> {
    let cache = ARCHIVE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let (cached_stamp, parsed) = cache.as_ref()?.get(path)?;
    (*cached_stamp == stamp).then(|| parsed.clone())
}

fn archive_cache_put(path: &std::path::Path, stamp: ArchiveStamp, parsed: &ParsedDocument) {
    let mut cache = ARCHIVE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(path.to_path_buf(), (stamp, parsed.clone()));
}

pub struct DocStore {
    pub kind: &'static DocKind,
    pub path: PathBuf,
    preserved: Arc<Mutex<Option<DocumentTemplate>>>,
    preserved_archive: Arc<Mutex<Option<DocumentTemplate>>>,
}
impl DocStore {
    pub fn open(project_root: &Path, kind: &'static DocKind) -> Self {
        DocStore {
            kind,
            path: project_root.join(kind.rel_path),
            preserved: Arc::new(Mutex::new(None)),
            preserved_archive: Arc::new(Mutex::new(None)),
        }
    }

    /// 取本 kind 的跨进程写锁(R-138)。
    ///
    /// **一个 kind 一把锁**,键取自活动文档路径,同时罩住活动文件、归档文件与
    /// 编号账本——这三者本来就是一笔账(`next_id` 要同时扫它们),分开锁等于没锁。
    ///
    /// **读路径一律不加锁**:原子写之后读者只会看到旧全量或新全量,不存在截断态;
    /// 让读者排队只会把"文档面板刷新"变成"等 agent 写完"。
    ///
    /// **防死锁不变量:持锁期间永不获取第二把锁。** 写事务只锁自己的 kind,
    /// `check_refs` 之类跨 kind 的查询走不加锁的读路径——结构上不可能循环等待。
    /// 谁要在持锁时再去锁另一个 kind,先把这条不变量改掉并说明新的加锁序。
    pub fn lock(&self) -> std::io::Result<crate::atomic_file::FileLock> {
        crate::atomic_file::lock_exclusive(&self.path)
    }

    /// 限时取锁:拿不到返回 `Ok(None)`。给"做不成也无所谓"的幂等写用。
    pub fn try_lock(
        &self,
        budget: std::time::Duration,
    ) -> std::io::Result<Option<crate::atomic_file::FileLock>> {
        crate::atomic_file::try_lock_exclusive(&self.path, budget)
    }

    pub fn load(&self) -> std::io::Result<Vec<Entry>> {
        // D-338:load 与 save 必须互斥。save 是 tmp+rename 原子替换,但 Windows 上
        // rename 覆盖目标与读者 open 目标之间有竞态窗口——读者在替换瞬间 open 会
        // NotFound,load 对 NotFound 宽容返回 Ok(vec![]) =「读到 0 条」的假空快照
        // (D-338 压测 20 轮 1 次失败,条目数 0)。
        //
        // D-382:这里改**共享档**。原先取排他锁,于是"读一下文档"要和 bash 围栏
        // (持全部托管文档排他锁直到命令结束,上限 600s)抢同一把锁——一条线跑
        // cargo check,桌面端文档面板就按 3s 预算取锁失败,界面停在"刷新失败"。
        // 读者之间、读者与围栏之间本来就不冲突;真正要挡的只有 save。共享档下
        // D-338 的保证一字不改:save 持排他,读者在它期间照样等,永远看不到中间态。
        // 排他持有者内部调 load 走重入(见 atomic_file 的 try_lock_shared),不自锁。
        let _lock = crate::atomic_file::lock_shared(&self.path)?;
        match std::fs::read_to_string(&self.path) {
            Ok(text) => {
                let parsed = parse_document(self.kind, &text);
                *self.preserved.lock().unwrap() = Some(parsed.template.clone());
                Ok(parsed.entries)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, entries: &[Entry]) -> std::io::Result<()> {
        // 自保护:直接调 save 的入口(测试、restore、外部工具)也拿得到互斥。
        // 已在外层事务里持锁的调用方(archive_terminal / tracker 写动作)走
        // 同线程重入,不会自锁死。
        let _lock = self.lock()?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let template = self.preserved.lock().unwrap().clone();
        let text = template
            .as_ref()
            .map(|template| render_with_template(self.kind, entries, template))
            .unwrap_or_else(|| render(self.kind, entries));
        // R-138:tmp+rename 原子替换。裸 std::fs::write 是先截断再写,并发读者
        // 会看到零长度/半截文件,而 load() 对空文件宽容返回 Ok(vec![])——
        // 「成功但空」的快照就是这么穿到前端的(D-249 第①层)。
        crate::atomic_file::write_atomic(&self.path, &text)
    }

    /// ID 分配扫活跃 + 归档 + 废弃账本:归档移走或主动废弃过的编号都绝不复用。
    pub fn next_id(&self, entries: &[Entry]) -> String {
        let archived = self.load_archive().unwrap_or_default();
        let max = entries
            .iter()
            .chain(archived.iter())
            .filter_map(|e| {
                e.id.strip_prefix(self.kind.prefix)?
                    .strip_prefix('-')?
                    .parse::<u32>()
                    .ok()
            })
            .chain(self.voided_ids().keys().copied())
            .max()
            .unwrap_or(0);
        format!("{}-{:03}", self.kind.prefix, max + 1)
    }

    /// 归档文件:同目录 `<name>-archive.md`(如 requirements-archive.md)。
    pub fn archive_file(&self) -> PathBuf {
        match self.path.file_stem().and_then(|s| s.to_str()) {
            Some(stem) => self.path.with_file_name(format!("{stem}-archive.md")),
            None => self.path.with_extension("archive.md"),
        }
    }

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
        let cleaned_title = strip_status_markers(&archived[pos].title);
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
                if key.is_empty() || (value.is_empty() && key == "阻塞") {
                    return false;
                }
                seen_fields.insert((key.to_string(), value.to_string()))
            });
            out.push(entry);
        }
        out
    }

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

/// 宽容解析:`## ` 开头即条目;ID 缺失/状态缺失都不报错(手改友好)。
pub fn parse(kind: &DocKind, text: &str) -> Vec<Entry> {
    parse_document(kind, text).entries
}

fn parse_document(kind: &DocKind, text: &str) -> ParsedDocument {
    let mut entries: Vec<Entry> = Vec::new();
    let mut templates = Vec::new();
    let mut preamble = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            let entry = parse_heading(kind, rest);
            templates.push(EntryTemplate {
                id: entry.id.clone(),
                lines: Vec::new(),
            });
            entries.push(entry);
        } else if let Some(entry) = entries.last_mut() {
            let template = templates.last_mut().expect("entry template exists");
            if let Some(bullet) = trimmed.trim_start().strip_prefix("- ") {
                if let Some((key, value)) = bullet.split_once(':') {
                    let key = key.trim().to_string();
                    entry.fields.push((key.clone(), value.trim().to_string()));
                    template.lines.push(TemplateLine::Field(key));
                } else {
                    template.lines.push(TemplateLine::Raw(line.to_string()));
                }
            } else {
                template.lines.push(TemplateLine::Raw(line.to_string()));
            }
        } else {
            preamble.push(line.to_string());
        }
    }
    ParsedDocument {
        entries,
        template: DocumentTemplate {
            preamble,
            entries: templates,
        },
    }
}
fn parse_heading(kind: &DocKind, rest: &str) -> Entry {
    let mut title = rest.trim().to_string();
    let mut status = String::new();
    let mut severity = None;

    // 从尾部剥离 (severity) 和 [status],顺序宽容。
    // severity 只在命中该文档类型的合法枚举时才剥离——标题自带的括号(如
    // "桌面端(类 VSCode 布局)")必须原样保留(狗粮暴露的 bug,见 D-002)。
    for _ in 0..2 {
        let t = title.trim_end();
        if t.ends_with(')') {
            if let (Some(pos), Some(valid)) = (t.rfind('('), kind.severities) {
                let candidate = t[pos + 1..t.len() - 1].trim();
                if valid.contains(&candidate) {
                    severity = Some(candidate.to_string());
                    title = t[..pos].trim_end().to_string();
                    continue;
                }
            }
        }
        // status 剥离判据(D-332 重构):只有「方括号在尾部且 [ 前是空白」才是状态标记
        // 形态——`vec[index]` 的 [ 前是字母、`[DONE] 帧` 的 ] 不在尾部,两者都不是状态
        // 标记,原样保留(D-070 与 D-002 同族)。形态符合时**合法/非法都剥离**:非法
        // candidate(如 requirement 上的 `[open]`)保留在 status 字段里,由调度层
        // fail-closed(INVALID + integrity 报错),不再静默变空字符串被当成可执行。
        if t.ends_with(']') {
            if let Some(pos) = t.rfind('[') {
                let candidate = t[pos + 1..t.len() - 1].trim();
                let preceded_by_space = pos > 0
                    && t[..pos]
                        .chars()
                        .last()
                        .map(|c| c.is_whitespace())
                        .unwrap_or(false);
                if preceded_by_space && !candidate.is_empty() {
                    status = candidate.to_string();
                    title = t[..pos].trim_end().to_string();
                    continue;
                }
            }
        }
        break;
    }

    // 首 token 形如 X-123 视为 ID。
    let (id, title) = match title.split_once(' ') {
        Some((first, rest)) if looks_like_id(first) => (first.to_string(), rest.trim().to_string()),
        _ if looks_like_id(&title) => (title.clone(), String::new()),
        _ => (String::new(), title.clone()),
    };
    Entry {
        id,
        title,
        status,
        severity,
        fields: Vec::new(),
    }
}

fn looks_like_id(s: &str) -> bool {
    match s.split_once('-') {
        Some((prefix, num)) => {
            !prefix.is_empty()
                && prefix.chars().all(|c| c.is_ascii_uppercase())
                && !num.is_empty()
                && num.chars().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

/// 字段值写进文档前必须归一成**单行**——这是往返不变式的唯一守点。
///
/// 解析契约是「一行一个字段」(见 `parse_document`):只有 `- key: value` 那一行会
/// 成为字段,其余任何行都落进 `TemplateLine::Raw`——原样保留但**不可寻址**。于是
/// 带换行的字段值一旦写出去,第 2 行起就永久脱离字段体系:update 只改得到第一行,
/// 剩下的段落**没有任何工具能删**(tracker 直写被拒、git restore 被引擎拦、shell
/// 整文件重写被拦)。实测 D-239 因此积了 3 份重复的「验收复核」段落(M-056 记录)。
///
/// 这里把换行折成空格,保证「写进去的东西一定能原样解析回来」。段落结构会丢,但
/// 内容一字不少——比起产生删不掉的垃圾,这是明显更小的代价。四个渲染出口必须都
/// 走这里,漏一个就等于漏一条产生游离段落的路。
fn push_field(out: &mut String, key: &str, value: &str) {
    let single_line = value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    out.push_str(&format!("- {key}: {single_line}\n"));
}

pub fn render(kind: &DocKind, entries: &[Entry]) -> String {
    let mut out = format!("# {}\n", kind.heading);
    for e in entries {
        out.push('\n');
        out.push_str(&format!("## {} {}", e.id, e.title));
        if !e.status.is_empty() {
            out.push_str(&format!(" [{}]", e.status));
        }
        if let Some(sev) = &e.severity {
            out.push_str(&format!(" ({sev})"));
        }
        out.push('\n');
        for (key, value) in &e.fields {
            push_field(&mut out, key, value);
        }
    }
    out
}

fn render_with_template(kind: &DocKind, entries: &[Entry], template: &DocumentTemplate) -> String {
    let mut out = String::new();
    if template.preamble.is_empty() {
        out.push_str(&format!("# {}\n", kind.heading));
    } else {
        for line in &template.preamble {
            out.push_str(line);
            out.push('\n');
        }
    }
    for entry in entries {
        let entry_template = template
            .entries
            .iter()
            .find(|candidate| candidate.id == entry.id);
        if let Some(entry_template) = entry_template {
            render_entry_with_template(&mut out, entry, entry_template);
        } else {
            render_entry(&mut out, entry);
        }
    }
    out
}

fn render_entry_with_template(out: &mut String, entry: &Entry, template: &EntryTemplate) {
    render_heading(out, entry);
    // D-329:模板尾部的空行是条目间距的残影(间距由 ensure_blank_separator 统一
    // 负责)。原样渲染会让追加的新字段落在空行之后——每次 update/close 都多出一段
    // 不可寻址的游离空段,且随写次数累积。渲染时裁掉尾部空 Raw,新字段紧跟末字段。
    let mut line_count = template.lines.len();
    while line_count > 0 {
        match &template.lines[line_count - 1] {
            TemplateLine::Raw(raw) if raw.trim().is_empty() => line_count -= 1,
            _ => break,
        }
    }
    let mut used = vec![false; entry.fields.len()];
    for line in &template.lines[..line_count] {
        match line {
            TemplateLine::Raw(raw) => {
                // 连续空行折叠为一个(D-130):条目内部堆积的空行是引擎自己吐出来的,
                // 不是用户内容——真正的自由文本一行不丢,只压掉重复的空白。
                if raw.trim().is_empty() && (out.ends_with("\n\n") || out.is_empty()) {
                    continue;
                }
                out.push_str(raw);
                out.push('\n');
            }
            TemplateLine::Field(key) => {
                if let Some((index, (current_key, value))) =
                    entry
                        .fields
                        .iter()
                        .enumerate()
                        .find(|(index, (current_key, _))| {
                            !used[*index] && current_key.eq_ignore_ascii_case(key)
                        })
                {
                    used[index] = true;
                    push_field(out, current_key, value);
                }
            }
        }
    }
    for (index, (key, value)) in entry.fields.iter().enumerate() {
        if !used[index] {
            push_field(out, key, value);
        }
    }
}

fn render_entry(out: &mut String, entry: &Entry) {
    render_heading(out, entry);
    for (key, value) in &entry.fields {
        push_field(out, key, value);
    }
}

/// 条目之间规范为恰好一个空行。
///
/// 不这么做会无限膨胀(D-130):解析时条目间的空行被存成上一条模板的
/// `TemplateLine::Raw("")`,渲染时原样写回,而这里若再无条件 `push('\n')`,
/// 每保存一次每条就多一行。实测 defects.md 已达 94% 空行、开头连着 225 个空行,
/// 把真实内容稀释到几乎不可读,还会把数据丢失这类关键 diff 埋掉。
/// 条目内的用户自由文本不受影响(D-060 的保留承诺仍成立),这里只规范条目间距——
/// 格式本就由引擎在写入侧强制(见模块头)。
fn ensure_blank_separator(out: &mut String) {
    if out.is_empty() {
        return;
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out.push_str("\n\n");
}

fn render_heading(out: &mut String, entry: &Entry) {
    ensure_blank_separator(out);
    out.push_str(&format!("## {} {}", entry.id, entry.title));
    if !entry.status.is_empty() {
        out.push_str(&format!(" [{}]", entry.status));
    }
    if let Some(sev) = &entry.severity {
        out.push_str(&format!(" ({sev})"));
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn 批次夹具(fields: Vec<(&str, &str)>) -> Entry {
        Entry {
            id: "R-999".into(),
            title: "t".into(),
            status: "doing".into(),
            severity: None,
            fields: fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    /// D-377:归档解析缓存的唯一风险是**给旧内容**。这里钉住失效键(mtime+长度):
    /// 归档被改写后,下一次 load_archive 必须看到新内容而不是命中上一次的解析结果。
    #[test]
    fn 归档解析缓存在文件改动后失效() {
        let root = std::env::temp_dir().join(format!(
            "kz-archive-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&root, &DEFECTS);
        let archive = store.archive_file();
        std::fs::create_dir_all(archive.parent().unwrap()).unwrap();

        std::fs::write(
            &archive,
            "# Defects

## D-001 头一条 [fixed] (low)
- 优先级: P3
",
        )
        .unwrap();
        let first = store.load_archive().unwrap();
        assert_eq!(first.len(), 1, "前置:归档应解析出一条");
        // 命中缓存:同一份文件重复读,结果一致。
        assert_eq!(store.load_archive().unwrap().len(), 1);

        std::fs::write(
            &archive,
            "# Defects

## D-001 头一条 [fixed] (low)
- 优先级: P3

## D-002 又一条 [fixed] (low)
- 优先级: P3
",
        )
        .unwrap();
        assert_eq!(
            store.load_archive().unwrap().len(),
            2,
            "归档改了却还在返回旧解析:缓存失效键失灵(D-377)"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 批次进度_只认显式声明_未声明即不分批() {
        let make = 批次夹具;
        // 没写批次一律 (0,1),复杂度不再生成格数。原先中=3/大=8 的固定默认值经
        // tracker 关闭门禁(total>1 && done<total)直接把没声明批次的中/大条目
        // 拦死——批数由 agent 定之后,引擎不替他猜(D-242 影响①的回归锁)。
        assert_eq!(batch_progress(&make(vec![("复杂度", "大")])), (0, 1));
        assert_eq!(batch_progress(&make(vec![("复杂度", "中")])), (0, 1));
        assert_eq!(batch_progress(&make(vec![("复杂度", "小")])), (0, 1));
        assert_eq!(
            batch_progress(&make(vec![])),
            (0, 1),
            "没评估复杂度按一轮做完算"
        );

        // 写了就以它为准:归档里真实存在 11 批的拆解条目,读路径不得钳到上限 10。
        assert_eq!(
            batch_progress(&make(vec![("复杂度", "大"), ("批次", "3/11")])),
            (3, 11)
        );
        // 手写文档的宽容:空格与全角斜杠。
        assert_eq!(batch_progress(&make(vec![("批次", " 2 ／ 5 ")])), (2, 5));
        // 已完成不会超过总数;0/0 视为没声明,回落"不分批"而不是画 0 个格。
        assert_eq!(batch_progress(&make(vec![("批次", "9/5")])), (5, 5));
        assert_eq!(
            batch_progress(&make(vec![("复杂度", "中"), ("批次", "0/0")])),
            (0, 1)
        );
        assert_eq!(batch_progress(&make(vec![("批次", "乱写")])), (0, 1));
    }

    #[test]
    fn 声明批数上限十批_超出拒绝并给出出路() {
        assert_eq!(
            check_declared_batches("0/10", None),
            Ok((0, 10)),
            "10 是合法上界"
        );
        assert_eq!(
            check_declared_batches(" 3 ／ 7 ", None),
            Ok((3, 7)),
            "宽容解析一致"
        );

        let over = check_declared_batches("0/11", None).unwrap_err();
        assert!(over.contains("10"), "错误里要点名上限: {over}");
        assert!(
            over.contains("后续条目"),
            "只说不行不算数,必须给出可执行的出路(D-173 的教训): {over}"
        );

        assert!(
            check_declared_batches("0/0", None).is_err(),
            "总数 0 没有意义"
        );
        assert!(
            check_declared_batches("乱写", None).is_err(),
            "格式非法要挡住"
        );
        assert!(
            check_declared_batches("5/3", None).is_err(),
            "已完成不能超过总数"
        );

        // 读路径不钳制的回归锁:上限只在写入侧生效,历史条目照原样读出来。
        // 谁"顺手"把 10 也钳到读路径上,归档的 11/11 会显示成 10/10,
        // 且声明 12 批的条目做完 10 批就会被关闭门禁放行。
        assert_eq!(
            declared_batch_progress(&批次夹具(vec![("批次", "3/11")])),
            Some((3, 11))
        );
    }

    #[test]
    fn 上限只拦抬高的总数_历史超限条目照常逐批推进() {
        // 存量/归档里真实存在 11 批的条目。它们的正常推进是「改已完成数、不动总数」——
        // 门禁若对 total>10 一律拒,agent 想动这类条目就只能先篡改总数,门禁反而在
        // 逼人伪造历史。基准比较把两件事分开:抬高才是新声明。
        assert_eq!(
            check_declared_batches("4/11", Some(11)),
            Ok((4, 11)),
            "历史 3/11 推进到 4/11 是逐批推进,必须放行"
        );
        assert_eq!(
            check_declared_batches("3/11", Some(11)),
            Ok((3, 11)),
            "总数原样重写(等于既有值)也算不高于,放行"
        );
        assert_eq!(
            check_declared_batches("3/3", Some(11)),
            Ok((3, 3)),
            "把总数改小到实际批数是我们鼓励的收口路径"
        );

        let 抬高 = check_declared_batches("3/16", Some(11)).unwrap_err();
        assert!(抬高.contains("16"), "错误要点名本次声明: {抬高}");
        assert!(抬高.contains("11"), "错误要点名既有基准: {抬高}");
        assert!(抬高.contains("后续条目"), "仍要给出可执行的出路: {抬高}");

        assert!(
            check_declared_batches("0/12", Some(5)).is_err(),
            "既有值本身没超上限时,抬到 12 照旧撞门"
        );
        assert!(
            check_declared_batches("0/11", None).is_err(),
            "新建没有既有值,按 <=10 严格约束"
        );
        // 基准只放宽上限,不放宽其它判据。
        assert!(
            check_declared_batches("12/11", Some(11)).is_err(),
            "已完成超过总数,给了基准也不能放行"
        );
    }

    #[test]
    fn roundtrip() {
        let entries = vec![Entry {
            id: "R-001".into(),
            title: "支持本地模型".into(),
            status: "doing".into(),
            severity: None,
            fields: vec![
                ("验收".into(), "ollama 走通循环".into()),
                ("refs".into(), "D-003".into()),
            ],
        }];
        let text = render(&REQUIREMENTS, &entries);
        assert_eq!(parse(&REQUIREMENTS, &text), entries);
    }

    /// D-294:字段值带换行时必须折成单行,否则第 2 行起会变成永远删不掉的游离段落。
    ///
    /// 反验方式:把 `push_field` 换回 `format!("- {key}: {value}\n")`,本用例第一处
    /// 断言就会红——解析回来只剩 2 个字段(第 3、4 行成了 Raw),而且此后无论怎么
    /// update 都碰不到它们。这正是 D-239 积出 3 份重复「验收复核」段落的机制。
    #[test]
    fn 多行字段值折成单行_不产生游离段落() {
        let entries = vec![Entry {
            id: "R-001".into(),
            title: "t".into(),
            status: "doing".into(),
            severity: None,
            fields: vec![
                (
                    "进展".into(),
                    "第一行\n第二行继续\n\n第三行前面还有空行".into(),
                ),
                ("refs".into(), "D-003".into()),
            ],
        }];
        let text = render(&REQUIREMENTS, &entries);

        // 往返闭合:字段数不变,值折成单行,内容一字不少。
        let back = parse(&REQUIREMENTS, &text);
        assert_eq!(back.len(), 1);
        assert_eq!(
            back[0].fields,
            vec![
                (
                    "进展".to_string(),
                    "第一行 第二行继续 第三行前面还有空行".to_string()
                ),
                ("refs".to_string(), "D-003".to_string()),
            ],
            "多行值必须折成单行字段,否则第 2 行起不可寻址"
        );

        // 文档里不得出现游离行:条目内每一行要么是标题要么是 `- key: value`。
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            assert!(
                line.starts_with("## ") || line.starts_with("# ") || line.starts_with("- "),
                "渲染产出了不可寻址的游离行: {line:?}"
            );
        }

        // 幂等:再存一次不会继续变形(游离段落当年正是靠这一步越积越多)。
        assert_eq!(render(&REQUIREMENTS, &back), text);
    }

    #[test]
    fn 游离行列出与删除_其余内容一字不变_二次保存幂等() {
        // R-201 验收①②③:raw_lines 稳定标识、raw_delete 只删指定行、删除后幂等。
        let dir = std::env::temp_dir().join(format!(
            "kz-rawlines-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let path = dir.join(REQUIREMENTS.rel_path);
        let text = "\
# Requirements

## R-001 条目 [todo]
- 进展: 第一行
- 优先级: P1
历史手写段落一
- 验收: 有验收
历史手写段落二
";
        std::fs::write(&path, text).unwrap();

        // ①列出:条目内从 1 起的序号 + 原文,稳定可辨。
        // raw_lines 依赖最近一次 load() 保存的模板(工具路径恒先 load,此处显式)。
        store.load().unwrap();
        let raws = store.raw_lines("R-001");
        assert_eq!(raws.len(), 2, "{raws:?}");
        assert_eq!(raws[0].ordinal, 1);
        assert!(raws[0].text.contains("历史手写段落一"), "{:?}", raws[0]);
        assert_eq!(raws[1].ordinal, 2);
        assert!(raws[1].text.contains("历史手写段落二"), "{:?}", raws[1]);
        assert!(store.raw_lines("R-999").is_empty(), "未知 ID 应为空");

        // ②删除第 2 条:文件里只少那一行,其余字节不变。
        store.delete_raw_line("R-001", 2).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(!after.contains("历史手写段落二"), "{after}");
        assert!(after.contains("历史手写段落一"), "{after}");
        assert!(after.contains("## R-001 条目 [todo]"), "{after}");
        assert!(after.contains("- 进展: 第一行"), "{after}");
        assert!(after.contains("- 优先级: P1"), "{after}");
        assert!(after.contains("- 验收: 有验收"), "{after}");

        // ④字段体系完全不受影响。
        let parsed = store.load().unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed[0].fields.len(),
            3,
            "删除游离行不得吞字段: {:?}",
            parsed[0].fields
        );
        assert!(parsed[0]
            .fields
            .iter()
            .any(|(k, v)| k == "进展" && v == "第一行"));

        // ③二次保存幂等:已删行不会从模板里复活。
        store.save(&parsed).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            after,
            "再次保存不得复活已删行"
        );

        // 越界序号拒绝且不写盘。
        let before = std::fs::read_to_string(&path).unwrap();
        let err = store.delete_raw_line("R-001", 9).unwrap_err();
        assert!(err.to_string().contains("只有 1 条"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before,
            "越界删除不得写盘"
        );
        assert!(store.delete_raw_line("R-001", 0).is_err(), "序号从 1 开始");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn title_with_parens_survives_roundtrip() {
        // D-002:中文括号后缀曾被误剥为 severity。
        let entries = vec![Entry {
            id: "R-002".into(),
            title: "Tauri 桌面端(类 VSCode 布局)".into(),
            status: "todo".into(),
            severity: None,
            fields: vec![],
        }];
        let text = render(&REQUIREMENTS, &entries);
        assert_eq!(parse(&REQUIREMENTS, &text), entries);
        // defects 文档里合法 severity 照常剥离
        let text = "## D-001 标题 [open] (high)\n";
        let parsed = parse(&DEFECTS, text);
        assert_eq!(parsed[0].severity.as_deref(), Some("high"));
        assert_eq!(parsed[0].title, "标题");
    }

    /// D-070:标题自带的方括号后缀不得被当成 status 剥离(与 D-002 同族)。
    #[test]
    fn title_with_brackets_survives_roundtrip() {
        let entries = vec![
            Entry {
                id: "R-100".into(),
                title: "支持 vec[index] 语法".into(),
                status: "todo".into(),
                severity: None,
                fields: vec![],
            },
            Entry {
                id: "R-101".into(),
                title: "处理 [DONE] 帧".into(),
                status: "doing".into(),
                severity: None,
                fields: vec![],
            },
        ];
        let text = render(&REQUIREMENTS, &entries);
        assert_eq!(parse(&REQUIREMENTS, &text), entries);

        // 方括号结尾但不是合法状态:必须原样留在标题里,不能被截断成非法状态
        let parsed = parse(&REQUIREMENTS, "## R-102 支持 vec[index]\n");
        assert_eq!(parsed[0].title, "支持 vec[index]");
        assert_eq!(parsed[0].status, "");

        // 合法状态照常剥离
        let parsed = parse(&REQUIREMENTS, "## R-103 普通标题 [done]\n");
        assert_eq!(parsed[0].title, "普通标题");
        assert_eq!(parsed[0].status, "done");
    }

    /// D-332:非法状态标记(requirement 上的 [open]/[fixed])必须被识别为非法 lifecycle,
    /// 不能静默留在标题里、status 解析为空——那样调度层会把空 lifecycle 当「非终态、
    /// 未阻塞、可执行」。形态判据:方括号在尾部且 [ 前是空白;非法值也剥离进 status。
    #[test]
    fn invalid_status_marker_is_parsed_not_silently_dropped() {
        // requirement 上出现 [open](合法枚举是 todo/doing/done/dropped)
        let parsed = parse(
            &REQUIREMENTS,
            "## R-200 新建 kanzei-base 零依赖 crate [open]\n",
        );
        assert_eq!(parsed[0].id, "R-200");
        assert_eq!(parsed[0].title, "新建 kanzei-base 零依赖 crate");
        assert_eq!(
            parsed[0].status, "open",
            "非法值必须进 status,由调度层 fail-closed"
        );

        // [fixed] 同理
        let parsed = parse(&REQUIREMENTS, "## R-201 某需求 [fixed]\n");
        assert_eq!(parsed[0].status, "fixed");

        // defect 上出现 [done](合法枚举是 open/fixing/fixed/wontfix)
        let parsed = parse(&DEFECTS, "## D-201 某缺陷 [done]\n");
        assert_eq!(parsed[0].status, "done");

        // 标题自带方括号仍必须原样保留:非状态标记形态
        // (a) [ 前是字母不是空白 —— vec[index]
        let parsed = parse(&REQUIREMENTS, "## R-202 支持 vec[index]\n");
        assert_eq!(parsed[0].title, "支持 vec[index]");
        assert_eq!(parsed[0].status, "");
        // (b) ] 不在尾部 —— [DONE] 帧
        let parsed = parse(&REQUIREMENTS, "## R-203 处理 [DONE] 帧\n");
        assert_eq!(parsed[0].title, "处理 [DONE] 帧");
        assert_eq!(parsed[0].status, "");

        // roundtrip:非法状态剥离后 render 应还原原文(标题 + [非法值])
        let entries = vec![Entry {
            id: "R-200".into(),
            title: "新建 kanzei-base 零依赖 crate".into(),
            status: "open".into(),
            severity: None,
            fields: vec![],
        }];
        let text = render(&REQUIREMENTS, &entries);
        assert!(
            text.contains("## R-200 新建 kanzei-base 零依赖 crate [open]"),
            "render 必须保留非法状态标记: {text}"
        );
    }

    #[test]
    fn tolerant_parse_of_hand_edits() {
        let text = "# Whatever\n\n## R-002 没写状态\n- 备注: 手改的\n\n## 连ID都没有 [todo]\n";
        let entries = parse(&REQUIREMENTS, text);
        assert_eq!(entries[0].id, "R-002");
        assert_eq!(entries[0].status, "");
        assert_eq!(entries[1].id, "");
        assert_eq!(entries[1].title, "连ID都没有");
        assert_eq!(entries[1].status, "todo");
    }

    #[test]
    fn id_allocation_and_transitions() {
        let store = DocStore {
            kind: &DEFECTS,
            path: "x".into(),
            preserved: Arc::new(Mutex::new(None)),
            preserved_archive: Arc::new(Mutex::new(None)),
        };
        let entries = vec![
            Entry {
                id: "D-002".into(),
                title: "t".into(),
                status: "open".into(),
                severity: None,
                fields: vec![],
            },
            Entry {
                id: "D-009".into(),
                title: "t".into(),
                status: "open".into(),
                severity: None,
                fields: vec![],
            },
        ];
        assert_eq!(store.next_id(&entries), "D-010");
        assert!(store.transition_allowed("open", "fixing").is_ok());
        assert!(store.transition_allowed("open", "wontfix").is_ok());
        assert!(store.transition_allowed("fixing", "open").is_err());
        assert!(store.transition_allowed("open", "banana").is_err());
        assert!(store.transition_allowed("手改状态", "fixing").is_ok());
    }

    #[test]
    fn archive_moves_terminal_and_preserves_ids() {
        let dir = std::env::temp_dir().join(format!("kz-archive-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mk = |id: &str, status: &str| Entry {
            id: id.into(),
            title: "t".into(),
            status: status.into(),
            severity: None,
            fields: vec![],
        };
        store
            .save(&[
                mk("R-001", "done"),
                mk("R-002", "doing"),
                mk("R-003", "dropped"),
            ])
            .unwrap();

        assert_eq!(store.archive_terminal().unwrap(), vec!["R-001", "R-003"]);
        let live = store.load().unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].id, "R-002");
        let archived = store.load_archive().unwrap();
        assert_eq!(archived.len(), 2);
        // 归档后 ID 分配仍延续全局最大值,不复用 R-003。
        assert_eq!(store.next_id(&live), "R-004");
        // 幂等:再跑一次不动任何东西。
        assert!(store.archive_terminal().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-316 归档净化:归档文件里的重复条目(同 id)与重复 key 字段(历史孤儿
    /// 误切)在任意归档动作时被收敛——重复 id 保留先归档的一份、同 key 保留
    /// 第一个非空、空字段删除;净化有变化时即使无新终态条目也强制写回。
    #[test]
    fn archive_terminal_净化重复条目与孤儿字段() {
        let dir = std::env::temp_dir().join(format!(
            "kz-archive-normalize-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mk = |id: &str, fields: Vec<(&str, &str)>| Entry {
            id: id.into(),
            title: "t".into(),
            status: "done".into(),
            severity: None,
            fields: fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };
        // 直接构造脏归档:活动文件放一个 done 条目触发归档,归档文件预置
        // D-309 重复两份 + D-312 字段脏数据(逐字重复的 复现、同 key 不同内容的
        // 验证、空 阻塞、空值多行表头 实测)。D-328 口径:只删结构垃圾,不删叙事。
        store.save(&[mk("R-100", vec![])]).unwrap();
        std::fs::write(
            store.archive_file(),
            "\
# Requirements Archive

## D-309 重复甲 [fixed] (medium)
- 复现: 甲
- 影响: 甲

## D-309 重复甲 [fixed] (medium)
- 复现: 甲
- 影响: 甲

## D-312 被污染 [fixed] (medium)
- 复现: 原条目复现
- 影响: 原条目影响
- 复现: 原条目复现
- 验证(2026-08-08): v6 迁移全绿
- 验证(2026-08-08): v7 从备份恢复,workspace 269 项通过
- 实测(2026-08-11):
- 阻塞:
",
        )
        .unwrap();
        store.archive_terminal().unwrap();
        let archived = store.load_archive().unwrap();
        // D-309 只剩一份。
        let d309: Vec<&Entry> = archived.iter().filter(|e| e.id == "D-309").collect();
        assert_eq!(d309.len(), 1, "重复条目必须被收敛: {archived:?}");
        let d312 = archived.iter().find(|e| e.id == "D-312").unwrap();
        // 逐字重复的 复现 收敛为一份。
        let repro: Vec<_> = d312.fields.iter().filter(|(k, _)| k == "复现").collect();
        assert_eq!(repro.len(), 1, "逐字重复字段必须收敛: {:?}", d312.fields);
        // 同 key 不同内容的 验证 两条都必须活着(D-328:按 key 吃第二条就是删证据)。
        let proofs: Vec<_> = d312
            .fields
            .iter()
            .filter(|(k, _)| k == "验证(2026-08-08)")
            .collect();
        assert_eq!(
            proofs.len(),
            2,
            "同名不同内容是叙事,不得去重: {:?}",
            d312.fields
        );
        // 空 阻塞 删除;空值多行表头 实测 保留(值在续行里,删表头续行就成孤儿)。
        assert!(!d312
            .fields
            .iter()
            .any(|(k, v)| k == "阻塞" && v.trim().is_empty()));
        assert!(
            d312.fields.iter().any(|(k, _)| k == "实测(2026-08-11)"),
            "空值多行表头不得误杀: {:?}",
            d312.fields
        );
        // R-100 也进来了。
        assert!(archived.iter().any(|e| e.id == "R-100"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-333:归档字段去重——重复「进展」合并内容(审计不丢),其它重复字段保留首条。
    #[test]
    fn dedupe_archived_fields_merges_progress_and_keeps_first_of_others() {
        let dir = std::env::temp_dir().join(format!(
            "kz-dedupe-arch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        std::fs::write(
            store.archive_file(),
            "# Requirements Archive\n\n\
             ## R-201 某需求 [done]\n\
             - 进展: 原始进展第一段\n\
             - 优先级: P1\n\
             - 进展: [terminal-fix 2026-08-13] done → done: 审计进展第二段\n\
             - 优先级: P2\n",
        )
        .unwrap();

        let (changed, removed) = store.dedupe_archived_fields("R-201").unwrap();
        assert!(changed, "应有去重发生");
        assert_eq!(removed, 2, "两条重复(进展 + 优先级)应被去除");

        let archived = store.load_archive().unwrap();
        let r201 = archived.iter().find(|e| e.id == "R-201").unwrap();
        let progresses: Vec<_> = r201.fields.iter().filter(|(k, _)| k == "进展").collect();
        assert_eq!(progresses.len(), 1, "进展应合并为一条: {:?}", r201.fields);
        assert!(
            progresses[0].1.contains("原始进展第一段") && progresses[0].1.contains("terminal-fix"),
            "进展内容必须都保留(审计不丢): {}",
            progresses[0].1
        );
        let priorities: Vec<_> = r201.fields.iter().filter(|(k, _)| k == "优先级").collect();
        assert_eq!(priorities.len(), 1, "优先级应保留首条: {:?}", r201.fields);
        assert_eq!(priorities[0].1, "P1", "应保留首条 P1 而非 P2");

        // 幂等:再次去重无变化。
        let (changed_again, removed_again) = store.dedupe_archived_fields("R-201").unwrap();
        assert!(!changed_again && removed_again == 0, "重复去重应幂等无变化");

        // 不存在的 id 安全返回无变化。
        let (changed_missing, removed_missing) = store.dedupe_archived_fields("R-999").unwrap();
        assert!(!changed_missing && removed_missing == 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-227:归档条目占位符测试 ID 回填——恰好命中一次替换,幂等(已回填再填=0),
    /// 找不到/多次命中拒绝,写路径与 dedupe 同锁同渲染。
    #[test]
    fn fill_archived_placeholder_回填占位符且拒绝歧义() {
        let dir = std::env::temp_dir().join(format!(
            "kz-archive-fill-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        std::fs::write(
            store.archive_file(),
            "# Requirements Archive\n\n\
             ## R-198 某需求 [done]\n\
             - 进展: 全量 cargo test --workspace 全绿(T-1786565xxx,harness 118)\n",
        )
        .unwrap();

        // 恰好命中一次 → 替换成功。
        let replaced = store
            .fill_archived_placeholder("R-198", "T-1786565xxx", "T-1786565346")
            .unwrap();
        assert_eq!(replaced, 1);
        let archived = store.load_archive().unwrap();
        let r198 = archived.iter().find(|e| e.id == "R-198").unwrap();
        let progress = r198
            .fields
            .iter()
            .find(|(k, _)| k == "进展")
            .map(|(_, v)| v.as_str())
            .unwrap();
        assert!(progress.contains("T-1786565346"), "{progress}");
        assert!(!progress.contains("T-1786565xxx"), "{progress}");

        // 幂等:已回填再填同一占位符 = 0,不写。
        let again = store
            .fill_archived_placeholder("R-198", "T-1786565xxx", "T-1786565346")
            .unwrap();
        assert_eq!(again, 0, "已回填的占位符不应再命中");

        // 找不到的 id → 报错。
        let err = store
            .fill_archived_placeholder("R-999", "T-1786565xxx", "T-1786565346")
            .unwrap_err();
        assert!(err.to_string().contains("R-999"), "{err}");

        // 多次命中 → 拒绝(有歧义)。
        std::fs::write(
            store.archive_file(),
            "# Requirements Archive\n\n\
             ## D-001 某缺陷 [fixed] (medium)\n\
             - 复现: T-1786562xxx 出现两次 T-1786562xxx\n",
        )
        .unwrap();
        let err = store
            .fill_archived_placeholder("D-001", "T-1786562xxx", "T-1786562463")
            .unwrap_err();
        assert!(err.to_string().contains("ambiguous"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-329:模板尾部空行是条目间距残影,渲染时必须裁掉——否则每次 update/close
    /// 追加的新字段都落在空行之后,不可寻址的游离空段随写次数累积(D-325 实测 1→2)。
    #[test]
    fn 追加字段不产生游离空段且多轮写入稳定() {
        let dir = std::env::temp_dir().join(format!(
            "kz-append-no-stray-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let file = dir.join(".kanzei/project/requirements.md");
        std::fs::write(
            &file,
            "# Requirements\n\n## R-001 甲 [open]\n- 复现: 甲\n\n## R-002 乙 [open]\n- 复现: 乙\n",
        )
        .unwrap();
        let mut entries = store.load().unwrap();
        entries[0]
            .fields
            .push(("进展".to_string(), "第一轮".to_string()));
        store.save(&entries).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("- 复现: 甲\n- 进展: 第一轮"),
            "追加字段必须紧跟末字段,不得隔空行:\n{text}"
        );
        // 第二轮写入不得累积新的空段(幂等)。
        let mut entries = store.load().unwrap();
        let progress = entries[0]
            .fields
            .iter_mut()
            .find(|(k, _)| k == "进展")
            .unwrap();
        progress.1 = "第二轮".to_string();
        store.save(&entries).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("- 复现: 甲\n- 进展: 第二轮"),
            "多轮写入后字段仍须紧凑:\n{text}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 反复保存不会让文档膨胀出空行() {
        // D-130:每次保存给每条多插一个空行,实测把 defects.md 稀释到 94% 空行。
        // 不变量:load→save 是幂等的,连续保存后文件字节数必须稳定。
        let dir = std::env::temp_dir().join(format!(
            "kz-blank-bloat-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mk = |id: &str| Entry {
            id: id.into(),
            title: "标题".into(),
            status: "todo".into(),
            severity: None,
            fields: vec![("验收".into(), "略".into())],
        };
        store
            .save(&[mk("R-001"), mk("R-002"), mk("R-003")])
            .unwrap();

        let mut sizes = Vec::new();
        for _ in 0..6 {
            let entries = store.load().unwrap();
            store.save(&entries).unwrap();
            sizes.push(std::fs::read_to_string(&store.path).unwrap().len());
        }
        assert!(
            sizes.windows(2).all(|w| w[0] == w[1]),
            "反复保存必须字节数稳定,实测: {sizes:?}"
        );

        // 已被历史膨胀污染的文档,一次保存即被规范回来。
        std::fs::write(
            &store.path,
            "# Requirements\n\n\n\n\n\n\n\n## R-001 标题 [todo]\n- 验收: 略\n\n\n\n\n\n\n## R-002 标题 [todo]\n- 验收: 略\n",
        )
        .unwrap();
        let entries = store.load().unwrap();
        store.save(&entries).unwrap();
        let text = std::fs::read_to_string(&store.path).unwrap();
        assert!(!text.contains("\n\n\n"), "不该留下连续空行:\n{text}");
        assert_eq!(store.load().unwrap().len(), 2, "规范化不得丢条目");

        // 条目内部的空行堆积同样要压掉,但用户自由文本一行不能少(D-060 承诺)。
        std::fs::write(
            &store.path,
            "# Requirements\n\n## R-001 标题 [todo]\n- 验收: 略\n\n\n\n手写说明不能丢\n\n\n\n### 子标题\n\n\n- 备注: 保留\n",
        )
        .unwrap();
        let entries = store.load().unwrap();
        store.save(&entries).unwrap();
        let text = std::fs::read_to_string(&store.path).unwrap();
        assert!(!text.contains("\n\n\n"), "条目内也不该留连续空行:\n{text}");
        for keep in ["手写说明不能丢", "### 子标题", "- 备注: 保留", "- 验收: 略"]
        {
            assert!(text.contains(keep), "自由内容丢失: {keep}\n{text}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn integrity_detects_missing_and_duplicated_ids() {
        // D-112:缺号=数据丢失;活动+归档同现=归档半途而废。
        let dir = std::env::temp_dir().join(format!(
            "kz-integrity-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &DEFECTS);
        let mk = |id: &str, status: &str| Entry {
            id: id.into(),
            title: "t".into(),
            status: status.into(),
            severity: None,
            fields: vec![],
        };
        // 活动: D-001 D-004;归档: D-002 D-004 → 缺 D-003,重复 D-004。
        store
            .save(&[mk("D-001", "open"), mk("D-004", "open")])
            .unwrap();
        std::fs::write(
            store.archive_file(),
            "# Defects Archive\n\n## D-002 done [fixed]\n\n## D-004 dup [fixed]\n",
        )
        .unwrap();
        let issues = store.integrity_issues(&store.load().unwrap());
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(issues[0].contains("D-004"), "{issues:?}");
        assert!(issues[1].contains("D-003"), "{issues:?}");
        assert!(issues[1].contains("UNACCOUNTED"), "{issues:?}");
        // 措辞不能再断言"数据丢失",也必须给出两条结构化出路(D-173)。
        assert!(issues[1].contains("void_id"), "{issues:?}");
        assert!(issues[1].contains("repair_missing_id"), "{issues:?}");

        // 完整状态:无告警。
        store
            .save(&[
                mk("D-001", "open"),
                mk("D-003", "open"),
                mk("D-004", "open"),
            ])
            .unwrap();
        std::fs::write(
            store.archive_file(),
            "# Defects Archive\n\n## D-002 done [fixed]\n",
        )
        .unwrap();
        assert!(store.integrity_issues(&store.load().unwrap()).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    fn 并发夹具(标记: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-{标记}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        dir
    }

    fn 造条目(id: &str, status: &str) -> Entry {
        Entry {
            id: id.into(),
            title: "标题".into(),
            status: status.into(),
            severity: None,
            fields: vec![("验收".into(), "略".into())],
        }
    }

    /// D-249 验收③ / R-138 验收①:原子写落地后 tracker 文件不会被读到截断态。
    ///
    /// 旧实现 `std::fs::write` 先截断再写,并发读者能实打实读到零长度文件,
    /// 而 `load()` 对空文件宽容返回 `Ok(vec![])`——「成功但空」的快照就从这里来。
    /// 这条用例在旧实现下会观测到 0 条目而失败,是真回归锁。
    #[test]
    fn 原子写下并发读永不看到截断态() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let dir = 并发夹具("atomic-read");
        let 少 = 3usize;
        let 多 = 30usize;
        // 两种规模差距要够大:内容长度悬殊才让截断窗口足够宽、可观测。
        let 小批: Vec<Entry> = (1..=少)
            .map(|n| 造条目(&format!("R-{n:03}"), "todo"))
            .collect();
        let 大批: Vec<Entry> = (1..=多)
            .map(|n| 造条目(&format!("R-{n:03}"), "todo"))
            .collect();
        DocStore::open(&dir, &REQUIREMENTS).save(&小批).unwrap();

        let 停 = Arc::new(AtomicBool::new(false));
        let 读者: Vec<_> = (0..2)
            .map(|_| {
                let dir = dir.clone();
                let 停 = Arc::clone(&停);
                std::thread::spawn(move || {
                    let mut 观测 = Vec::new();
                    while !停.load(Ordering::Relaxed) {
                        // 每次新开 store:与"另一个进程来读"最接近的形态。
                        match DocStore::open(&dir, &REQUIREMENTS).load() {
                            Ok(entries) => 观测.push(Ok(entries.len())),
                            Err(e) => 观测.push(Err(e.to_string())),
                        }
                    }
                    观测
                })
            })
            .collect();

        for round in 0..200 {
            let batch = if round % 2 == 0 { &小批 } else { &大批 };
            DocStore::open(&dir, &REQUIREMENTS).save(batch).unwrap();
        }
        停.store(true, Ordering::Relaxed);

        let mut 总数 = 0usize;
        for handle in 读者 {
            for 观测 in handle.join().unwrap() {
                总数 += 1;
                match 观测 {
                    Ok(len) => assert!(
                        len == 少 || len == 多,
                        "读到了截断态:条目数 {len},只可能是 {少} 或 {多}"
                    ),
                    Err(e) => panic!("原子写之后读不该失败: {e}"),
                }
            }
        }
        assert!(总数 > 0, "读者一次也没跑到,这条用例没有证明力");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 归档是**跨两个文件**的两步写,原子写保证不了两者之间的原子性。
    /// 但当前写序(先写归档、再删活动)保证了任一瞬间条目至少在一处可见;
    /// 谁把 save 提到 write_atomic 前面,这条就会红。
    #[test]
    fn 归档过程中条目不会在两个文件里同时消失() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let dir = 并发夹具("archive-race");
        let 全部: Vec<String> = (1..=12).map(|n| format!("R-{n:03}")).collect();
        let entries: Vec<Entry> = 全部
            .iter()
            .enumerate()
            .map(|(i, id)| 造条目(id, if i % 2 == 0 { "done" } else { "doing" }))
            .collect();
        DocStore::open(&dir, &REQUIREMENTS).save(&entries).unwrap();

        let 停 = Arc::new(AtomicBool::new(false));
        let 读者 = {
            let dir = dir.clone();
            let 停 = Arc::clone(&停);
            std::thread::spawn(move || {
                let mut 缺失 = Vec::new();
                let mut 轮次 = 0usize;
                while !停.load(Ordering::Relaxed) {
                    let store = DocStore::open(&dir, &REQUIREMENTS);
                    let (Ok(active), Ok(archived)) = (store.load(), store.load_archive()) else {
                        continue;
                    };
                    let 可见: std::collections::BTreeSet<String> = active
                        .iter()
                        .chain(archived.iter())
                        .map(|e| e.id.clone())
                        .collect();
                    轮次 += 1;
                    缺失.extend(
                        (1..=12)
                            .map(|n| format!("R-{n:03}"))
                            .filter(|id| !可见.contains(id)),
                    );
                }
                (轮次, 缺失)
            })
        };

        DocStore::open(&dir, &REQUIREMENTS)
            .archive_terminal()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        停.store(true, Ordering::Relaxed);
        let (轮次, 缺失) = 读者.join().unwrap();
        assert!(轮次 > 0, "读者一次也没跑到,这条用例没有证明力");
        assert!(
            缺失.is_empty(),
            "归档中途条目在两个文件里同时消失: {缺失:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-138 验收③:并发写 tracker 不丢条目、不撞 ID。
    ///
    /// 关键在于锁罩住的是 **load → next_id → save** 整段。只锁 save 挡不住这条:
    /// 两次 save 本来就不重叠,丢失发生在各自的"读"与"写"之间——两个写者读到
    /// 同一份条目、算出同一个 next_id,后写的把先写的整个覆盖掉。
    #[test]
    fn 并发写不丢条目也不撞编号() {
        let dir = 并发夹具("concurrent-write");
        DocStore::open(&dir, &REQUIREMENTS).save(&[]).unwrap();

        let 写者 = 8usize;
        let handles: Vec<_> = (0..写者)
            .map(|_| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    let store = DocStore::open(&dir, &REQUIREMENTS);
                    let _lock = store.lock().unwrap();
                    let mut entries = store.load().unwrap();
                    let id = store.next_id(&entries);
                    entries.push(造条目(&id, "todo"));
                    store.save(&entries).unwrap();
                    id
                })
            })
            .collect();
        let 分配: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        let 落盘 = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        assert_eq!(落盘.len(), 写者, "有写者的条目被覆盖掉了: {落盘:?}");
        let 唯一: std::collections::BTreeSet<&String> = 分配.iter().collect();
        assert_eq!(唯一.len(), 写者, "分配出了重复 ID: {分配:?}");
        assert!(
            DocStore::open(&dir, &REQUIREMENTS)
                .integrity_issues(&落盘)
                .is_empty(),
            "并发写之后完整性必须干净"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-249 第②层的正面锁:`load()` 只把「文件不存在」当作"真的没有条目",
    /// 其余读失败必须如实上报。谁把它宽容成 `Ok(vec![])`,上层就再也分不清
    /// 「没有条目」和「读不到」——那正是这条缺陷的核心。
    #[cfg(windows)]
    #[test]
    fn load_遇到真实读失败要报错而不是空列表() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = 并发夹具("load-error");
        let store = DocStore::open(&dir, &REQUIREMENTS);
        store.save(&[造条目("R-001", "todo")]).unwrap();

        // 文件不存在 = 真的没有条目,照旧放行。
        let 空店 = DocStore::open(&dir, &FINDINGS);
        assert_eq!(空店.load().unwrap().len(), 0);

        // 独占占用 = 读不到,必须报错。
        let 占用 = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&store.path)
            .unwrap();
        let error = DocStore::open(&dir, &REQUIREMENTS).load().unwrap_err();
        assert_ne!(error.kind(), std::io::ErrorKind::NotFound);
        drop(占用);
        assert_eq!(DocStore::open(&dir, &REQUIREMENTS).load().unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refs_extraction() {
        let e = Entry {
            id: "F-001".into(),
            title: "t".into(),
            status: "draft".into(),
            severity: None,
            fields: vec![("refs".into(), "S-001, S-002".into())],
        };
        assert_eq!(e.refs(), vec!["S-001", "S-002"]);
    }

    /// R-252 验收①:IDEAS 文档线状态机 inbox→split/dropped 有测试。
    /// 前置语义:录入不过模型原样收下(inbox 是初始态)、拆解后转 split、
    /// 用户放弃转 dropped;split/dropped 是终态(不再回流)。
    #[test]
    fn ideas_state_machine_inbox_to_split_or_dropped() {
        let kind: &DocKind = &IDEAS;
        assert_eq!(kind.prefix, "I");
        assert_eq!(kind.statuses, &["inbox", "split", "dropped"]);
        assert_eq!(kind.terminal, &["split", "dropped"]);
        assert_eq!(kind.statuses[0], "inbox");
        // 不设优先级/严重度/标签:想法是原始收件箱,不参与取活与分类。
        assert!(kind.severities.is_none());
        assert!(kind.priorities.is_none());
        assert!(kind.tags.is_none());

        let dir = std::env::temp_dir().join(format!(
            "kz-ideas-state-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = DocStore::open(&dir, kind);
        // inbox → split / inbox → dropped 放行(终态可达)。
        assert!(store.transition_allowed("inbox", "split").is_ok());
        assert!(store.transition_allowed("inbox", "dropped").is_ok());
        // 终态不可回流到非终态:split/dropped 不再回到 inbox(forward-only,非双向)。
        assert!(store.transition_allowed("split", "inbox").is_err());
        assert!(store.transition_allowed("dropped", "inbox").is_err());
        // 终态→终态按关闭语义放行(close 可任意走到终态);split/dropped 互转合法。
        assert!(store.transition_allowed("dropped", "split").is_ok());
        // 未知状态拒绝;split 是合法目标。
        assert!(store.transition_allowed("inbox", "banana").is_err());
        // 实际走一遍 add → split 的落盘闭环。
        store
            .save(&[Entry {
                id: "I-001".into(),
                title: "一个原始想法".into(),
                status: "inbox".into(),
                severity: None,
                fields: vec![],
            }])
            .unwrap();
        let entries = store.load().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, "inbox");
        // ID 前缀与下一个编号正确。
        assert_eq!(store.next_id(&entries), "I-002");
        std::fs::remove_dir_all(dir).ok();
    }
}
