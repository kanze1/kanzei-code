//! 记忆准入策略(R-255 第二刀,从 store.add 提纯)。
//!
//! 独立理由:准入是「什么有资格成为记忆」的研究策略——枚举校验、必填、subject
//! 不变式、交付状态拒收、指纹一致性、精确+近似标题判重,是记忆研究里变更最频繁
//! 的一层;把它锁在 Repository 私有逻辑里等于让 research policy 与 storage
//! mechanics 强耦合。提成独立结构后,做记忆实验(调准入门槛)只改这一处(验收⑥),
//! 且可不经 add 直接构造场景测(验收②)(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):状态不变量(subject 冲突)先于标题去重与 R-216 语义闸,且
//! 不受 force 影响(状态就地覆盖,绝不并存);近似判重比例与绝对量两道都要过,
//! 少一道会误杀短标题(TITLE_DUP_MIN_COMMON)。

use std::path::PathBuf;

use super::MemoryEntry;

/// 近似重复的判定阈值:两个标题切词后的包含度。0.55 是拿实测样本卡的——
/// 2026-08-12 归档的那 8 条「tracker update 字段语义」两两在 0.57~0.75,
/// 而 M-011《repair_reused_id 修复》与 M-012《完整性门禁拒绝写操作》这种
/// 同子系统但确属两条知识的只有 0.32,阈值落在中间。
pub(crate) const TITLE_DUP_THRESHOLD: f64 = 0.55;

/// 光看比例会误杀短标题:「安装通道切换 SOP」与「安装通道改为便携版」共享
/// 「安装通道」四个字就到 0.57,但那是两条知识。所以再加一道绝对量下限——
/// 真重复的那 8 条两两共享 12~16 个词,短标题的偶然同名到不了 8。
const TITLE_DUP_MIN_COMMON: usize = 8;

/// 记忆准入策略:无状态,所有判定基于传入的 entries 与参数(纯函数,独立可测,
/// 不经 add 也能构造场景)。
pub(crate) struct MemoryAdmission;

impl MemoryAdmission {
    /// 基础校验:category 枚举、title/description/body 必填。
    pub(crate) fn validate_basic(
        category: &str,
        title: &str,
        description: &str,
        body: &str,
    ) -> anyhow::Result<()> {
        if !super::CATEGORIES.contains(&category) {
            anyhow::bail!(
                "invalid category `{category}`; valid: {}",
                super::CATEGORIES.join(" | ")
            );
        }
        let title = title.trim();
        let description = description.trim();
        if title.is_empty() {
            anyhow::bail!("title must not be empty");
        }
        if description.is_empty() {
            anyhow::bail!("description must not be empty — it is the retrieval hook");
        }
        // 空正文条目是纯噪声:占编号、进 FTS、召回出来啥也没有。2026-08-12 清理时
        // 库里躺着 3 条只有 frontmatter 的条目(M-039/M-048/M-049),都是 manager
        // 在权限受限的自动轮里批量写记忆时产的。写入侧直接拒。
        if body.trim().is_empty() {
            anyhow::bail!(
                "body must not be empty — an entry with only frontmatter is unusable: \
                 put the actual finding (what happened, what to do instead) in the body"
            );
        }
        Ok(())
    }

    /// 状态不变量:同 scope+category+subject 至多一条 active/candidate——同 subject 的 add
    /// 必须先报 SubjectConflict,不能先被语义闸拦成 Uncertain(测试锚点)。
    /// 先于标题去重与 R-216 语义闸,且不受 force 影响:状态就地覆盖,绝不并存。
    pub(crate) fn find_subject_conflict<'a>(
        entries: &'a [(PathBuf, MemoryEntry)],
        category: &str,
        subject: Option<&str>,
    ) -> Option<&'a MemoryEntry> {
        let subject = subject.map(str::trim).filter(|s| !s.is_empty())?;
        entries
            .iter()
            .find(|(_, e)| {
                (e.status == "active" || e.status == "candidate")
                    && e.category == category
                    && e.extras.iter().any(|(k, v)| k == "subject" && v == subject)
            })
            .map(|(_, e)| e)
    }

    /// R-216 闸①:交付状态拒收——标题/subject 命中「R-/D- 编号 + 已交付/勿重复/
    /// 验收边界」形态时,这是 tracker 的状态,不是记忆;force 也不能绕过。
    pub(crate) fn check_delivery_state(title: &str, subject: Option<&str>) -> anyhow::Result<()> {
        let title_lc = title.to_lowercase();
        let subject_lc = subject.map(str::to_lowercase).unwrap_or_default();
        let is_delivery_state = ["已交付", "勿重复", "验收边界", "delivered", "do not repeat"]
            .iter()
            .any(|kw| {
                title_lc.contains(&kw.to_lowercase()) || subject_lc.contains(&kw.to_lowercase())
            });
        if is_delivery_state && has_tracker_id(title) {
            anyhow::bail!(
                "标题/subject 命中交付状态形态(R-/D- 编号 + 已交付/勿重复/验收边界)——\
                 这是 tracker 条目的状态,不是记忆。\
                 记忆记「怎么做」的约束,不记「哪个条目交付了」;交付状态请写在 requirements/defects 里,refs 引用即可。"
            );
        }
        Ok(())
    }

    /// R-216 闸②:指纹一致性——标题/description/body 携带 [fp:] 必须与来源 note 中
    /// 引擎生成的指纹逐字一致。force 也不能伪造或绕过来源证据。
    pub(crate) fn check_fingerprint(
        text: &str,
        inbox_text: &str,
        existing_fps: impl Iterator<Item = String>,
    ) -> anyhow::Result<()> {
        let fp_markers = super::fp_markers(text);
        if !fp_markers.is_empty() {
            let existing: Vec<String> = existing_fps.collect();
            for fp in &fp_markers {
                let legit = inbox_text.contains(fp.as_str()) || existing.contains(fp);
                if !legit {
                    anyhow::bail!(
                        "标题/description/body 携带指纹 {fp} 但该指纹不存在于 inbox 来源 note 或任何既有条目——\
                         指纹是引擎从失败信号生成的,禁止自造(实证 M-055/M-056)。\
                         去掉自造指纹,或先用 memory_note 记录真实来源。"
                    );
                }
            }
        }
        Ok(())
    }

    /// 同指纹判重:同 scope 内 active/candidate 共享同一失败指纹时,无论标题/category
    /// 是否变化都返回既有条目。指纹是失败信号的主键,允许重复落盘会把一次复发扩成
    /// 多条候选并破坏后续 recurrence 计数；deprecated/invalid 已不在 load_all 范围。
    pub(crate) fn find_fingerprint_conflict<'a>(
        entries: &'a [(PathBuf, MemoryEntry)],
        fingerprints: &[String],
    ) -> Option<&'a MemoryEntry> {
        entries
            .iter()
            .filter(|(_, entry)| entry.status == "active" || entry.status == "candidate")
            .find(|(_, entry)| {
                entry
                    .fingerprint()
                    .is_some_and(|existing| fingerprints.iter().any(|fp| fp == &existing))
            })
            .map(|(_, entry)| entry)
    }

    /// 精确 + 近似标题判重(不可被 force 绕过):
    /// - 一字不差(normalize 后)即 Duplicate;
    /// - 切词包含度 ≥ TITLE_DUP_THRESHOLD 且绝对量 ≥ TITLE_DUP_MIN_COMMON 也算重复;
    /// - CJK 短标题使用「较短标题全部被包含」的保守规则,不再被绝对量 8 拒绝。
    pub(crate) fn find_duplicate<'a>(
        entries: &'a [(PathBuf, MemoryEntry)],
        category: &str,
        title: &str,
    ) -> Option<&'a MemoryEntry> {
        let normalized = normalize_title(title);
        if let Some((_, existing)) = entries.iter().find(|(_, e)| {
            (e.status == "active" || e.status == "candidate")
                && e.category == category
                && normalize_title(&e.title) == normalized
        }) {
            return Some(existing);
        }
        // 近似去重:跨 category 也查。
        entries
            .iter()
            .map(|(_, e)| e)
            .filter(|e| e.status == "active" || e.status == "candidate")
            .filter(|e| title_containment(title, &e.title) >= TITLE_DUP_THRESHOLD)
            .max_by(|a, b| {
                title_containment(title, &a.title)
                    .partial_cmp(&title_containment(title, &b.title))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }
}

/// R-216:标题是否含 tracker 条目编号(R-xxx / D-xxx)。
pub(crate) fn has_tracker_id(title: &str) -> bool {
    let upper = title.to_uppercase();
    for prefix in ["R-", "D-"] {
        if let Some(rest) = upper.strip_prefix(prefix) {
            if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return true;
            }
        }
        // 也可能编号在中间:「关于 R-012 的交付状态」。
        for (idx, _) in upper.match_indices(prefix) {
            let after = &upper[idx + 2..];
            if after.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

/// 标题规范化:只留字母数字,小写(判重用,与显示无关)。
pub(crate) fn normalize_title(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// 标题切词:CJK 按单字、ASCII 按词(小写),标点丢弃。CJK 不分词就没法比,
/// 单字粒度对中文标题足够——记忆标题短,长词的顺序信息不重要。
fn title_tokens(title: &str) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let mut word = String::new();
    for ch in title.chars() {
        if is_cjk(ch) {
            if !word.is_empty() {
                out.insert(std::mem::take(&mut word));
            }
            out.insert(ch.to_string());
        } else if ch.is_alphanumeric() {
            word.extend(ch.to_lowercase());
        } else if !word.is_empty() {
            out.insert(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        out.insert(word);
    }
    out
}

/// 包含度 = 交集 / 较短一侧。用包含度而不是 Jaccard:同一个坑的重复条目
/// 常常一条写得长、一条写得短,Jaccard 会被长的那条稀释掉。
/// 普通标题要求比例与绝对量;CJK 短标题则要求较短标题的全部字符被包含,
/// 以覆盖「缓存」/「缓存清理」这类短标题，同时避免单个通用字误判。
pub(crate) fn title_containment(a: &str, b: &str) -> f64 {
    let (ta, tb) = (title_tokens(a), title_tokens(b));
    let shorter = ta.len().min(tb.len());
    let common = ta.intersection(&tb).count();
    if is_short_cjk_title(a) && is_short_cjk_title(b) {
        if shorter >= 2 && common == shorter {
            return 1.0;
        }
        return 0.0;
    }
    if shorter < 6 || common < TITLE_DUP_MIN_COMMON {
        return 0.0;
    }
    common as f64 / shorter as f64
}

fn is_short_cjk_title(title: &str) -> bool {
    let alnum: Vec<char> = title.chars().filter(|ch| ch.is_alphanumeric()).collect();
    !alnum.is_empty() && alnum.iter().all(|ch| is_cjk(*ch)) && alnum.len() <= 8
}

/// CJK 单字判定(标题切词与 CJK 切分共用)。
pub(crate) fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}
/// D-282:主题 token 交集计数。英文按词(小写)、CJK 按单字,去掉高频虚词
/// (的/了/是/在/与…),避免「新 description 全是通用字」被误判为同主题。
/// 返回 0 表示两段文本没有共同主题词——description 与条目主题漂移的判据。
pub(crate) fn topic_overlap(a: &str, b: &str) -> usize {
    fn tokens(text: &str) -> std::collections::HashSet<String> {
        let mut set = std::collections::HashSet::new();
        let mut word = String::new();
        for ch in text.chars() {
            if ch.is_ascii_alphanumeric() {
                word.push(ch.to_ascii_lowercase());
            } else {
                if word.len() >= 2 {
                    set.insert(word.clone());
                }
                word.clear();
                if is_cjk(ch) && !STOP_CHARS.contains(&ch) {
                    set.insert(ch.to_string());
                }
            }
        }
        if word.len() >= 2 {
            set.insert(word);
        }
        set
    }
    let ta = tokens(a);
    let tb = tokens(b);
    ta.intersection(&tb).count()
}

/// 主题判据里忽略的 CJK 虚词/通用字(单字交集噪音源)。
const STOP_CHARS: &[char] = &[
    '的', '了', '是', '在', '与', '和', '或', '及', '对', '为', '时', '要', '不', '会', '能', '可',
    '等', '这', '那', '就', '也', '被', '把', '从', '到', '以', '于', '之', '其', '它', '他', '她',
    '个', '条', '种', '次', '项', '处', '点', '段', '行', '列', '张', '件', '份', '页', '步', '轮',
    '批', '并', '且', '但', '而', '若', '则', '即', '如', '虽', '还', '又', '再', '更', '最', '很',
    '太', '仅', '只', '已', '未', '无', '有', '没', '别', '自', '各', '每', '某', '几', '两', '多',
    '少', '一', '二', '三', '四', '五', '六', '七', '八', '九', '十', '百', '千', '万', '零',
];

#[cfg(test)]
mod tests {
    // R-255 验收②:准入策略的独立可测入口——不经 store.add,直接构造 MemoryEntry
    // 场景调 MemoryAdmission 方法,证明 research policy 与 storage mechanics 已解耦。
    use super::*;
    use std::path::PathBuf;

    fn entry(status: &str, category: &str, title: &str, subject: Option<&str>) -> MemoryEntry {
        let mut extras = Vec::new();
        if let Some(subject) = subject {
            extras.push(("subject".to_string(), subject.to_string()));
        }
        MemoryEntry {
            id: "M-001".into(),
            scope: "project".into(),
            category: category.into(),
            title: title.into(),
            description: "desc".into(),
            status: status.into(),
            created: "2026-01-01".into(),
            updated: "2026-01-01".into(),
            source: "user".into(),
            extras,
            body: "body".into(),
        }
    }

    fn pack(entries: Vec<MemoryEntry>) -> Vec<(PathBuf, MemoryEntry)> {
        entries.into_iter().map(|e| (PathBuf::new(), e)).collect()
    }

    #[test]
    fn validate_basic_拒空与非法类别() {
        assert!(MemoryAdmission::validate_basic("nope", "t", "d", "b").is_err());
        assert!(MemoryAdmission::validate_basic("fact", "", "d", "b").is_err());
        assert!(MemoryAdmission::validate_basic("fact", "t", "", "b").is_err());
        assert!(MemoryAdmission::validate_basic("fact", "t", "d", "  ").is_err());
        assert!(MemoryAdmission::validate_basic("fact", "t", "d", "b").is_ok());
    }

    #[test]
    fn subject冲突_只命中同category同subject的active() {
        let entries = pack(vec![
            entry("active", "fact", "A", Some("s1")),
            entry("candidate", "fact", "B", Some("s1")),
            entry("active", "sop", "C", Some("s1")),
        ]);
        // 同 category+subject 的 active 命中(即使还有 candidate 同 subject)。
        let hit = MemoryAdmission::find_subject_conflict(&entries, "fact", Some("s1"));
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().title, "A");
        // 不同 category 不冲突(该 category 无此 subject);不同 subject 不冲突;None subject 不冲突。
        assert!(MemoryAdmission::find_subject_conflict(&entries, "sop", Some("s2")).is_none());
        assert!(MemoryAdmission::find_subject_conflict(&entries, "fact", Some("s2")).is_none());
        assert!(MemoryAdmission::find_subject_conflict(&entries, "fact", None).is_none());
    }

    #[test]
    fn 交付状态拒收_force也不可跳过() {
        assert!(MemoryAdmission::check_delivery_state("R-123 已交付", None).is_err());
        assert!(MemoryAdmission::check_delivery_state("本地代理已配置", None).is_ok());
    }

    #[test]
    fn 指纹一致性_合法与自造() {
        let body = "踩坑 [fp:abc123] 记录";
        // 指纹出现在 inbox → 合法。
        assert!(
            MemoryAdmission::check_fingerprint(body, "note [fp:abc123]", std::iter::empty())
                .is_ok()
        );
        // 指纹不在 inbox 也不在既有条目 → 拒绝(自造),force 也不能跳过。
        assert!(MemoryAdmission::check_fingerprint(body, "", std::iter::empty()).is_err());
        assert!(MemoryAdmission::check_fingerprint(
            "description [fp:abc123]",
            "",
            std::iter::empty()
        )
        .is_err());
        // 无指纹体不查。
        assert!(MemoryAdmission::check_fingerprint("无指纹", "", std::iter::empty()).is_ok());
    }

    #[test]
    fn 同指纹判重只命中_active_candidate() {
        let mut active = entry("active", "fact", "主条目", None);
        active.body = "正文 [fp:edit|not found]".into();
        let mut candidate = entry("candidate", "fact", "候选条目", None);
        candidate.body = "候选 [fp:other]".into();
        let mut deprecated = entry("deprecated", "fact", "旧条目", None);
        deprecated.body = "旧正文 [fp:edit|not found]".into();
        let entries = pack(vec![active, candidate, deprecated]);
        let hit =
            MemoryAdmission::find_fingerprint_conflict(&entries, &["[fp:edit|not found]".into()]);
        assert_eq!(hit.map(|entry| entry.title.as_str()), Some("主条目"));
        assert!(
            MemoryAdmission::find_fingerprint_conflict(&entries, &["[fp:missing]".into()])
                .is_none()
        );
    }

    #[test]
    fn 标题判重_精确近似与_cjk_短标题且_force_不可绕() {
        let entries = pack(vec![entry(
            "active",
            "fact",
            "gh 要走本地代理且要配置 http 代理地址",
            None,
        )]);
        let exact = MemoryAdmission::find_duplicate(
            &entries,
            "fact",
            "gh 要走本地代理且要配置 http 代理地址",
        );
        assert!(exact.is_some());
        let near = MemoryAdmission::find_duplicate(
            &entries,
            "fact",
            "gh 必须走本地代理且配置 http 代理地址",
        );
        assert!(near.is_some());
        let other = MemoryAdmission::find_duplicate(&entries, "fact", "完整性门禁拒绝写操作");
        assert!(other.is_none());
        // 标题重复不属于可 force 绕过的语义闸。
        assert!(MemoryAdmission::find_duplicate(
            &entries,
            "fact",
            "gh 要走本地代理且要配置 http 代理地址",
        )
        .is_some());

        let short = pack(vec![entry("candidate", "fact", "缓存清理", None)]);
        assert!(MemoryAdmission::find_duplicate(&short, "fact", "缓存").is_some());
    }
}
