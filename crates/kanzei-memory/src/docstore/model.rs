//! 文档模型域(R-257 B3):DocKind 定义与 7 个文档类型常量、Entry/RawLine 结构、
//! 批次数上限与进度计算。自 docstore.rs 原样迁出,零行为变更。

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
