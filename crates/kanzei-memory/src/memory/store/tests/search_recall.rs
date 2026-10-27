#[test]
fn search_ranks_and_records_hits_and_rebuilds_after_db_loss() {
    let (dir, store) = temp_store();
    add(
        &store,
        "fact",
        "CRLF 是 edit 未命中主因",
        "处理 edit 替换失败换行符问题必读",
        "自动容忍已落地",
    );
    add(
        &store,
        "sop",
        "发版 SOP 两条通道",
        "发版发布安装更新相关必读",
        "package.ps1 -Publish 后静默装 setup",
    );
    // D-366:排序决策在检索门面,这里经 index 取决策排序后的 top-k。
    let index = SqliteMemoryIndex::new(&dir);
    let hits = index
        .search_entries(&IndexQuery::text("发版 更新"), None, Some("active"), 5)
        .unwrap();
    assert_eq!(
        hits[0].entry.id,
        "M-002",
        "{:?}",
        hits.iter().map(|h| &h.entry.id).collect::<Vec<_>>()
    );
    assert!(
        hits[0].snippet.contains('['),
        "snippet 高亮: {}",
        hits[0].snippet
    );
    // 命中计数生效(search_entries 在决策排序后记 record_hits)
    let again = index
        .search_entries(&IndexQuery::text("发版"), None, None, 5)
        .unwrap();
    assert!(again[0].hits >= 1);
    // 删库重建:真源是文件
    drop(again);
    std::fs::remove_file(store.db_path()).unwrap();
    store.refresh_derived().unwrap();
    let rebuilt = index
        .search_entries(&IndexQuery::text("CRLF"), None, None, 5)
        .unwrap();
    assert_eq!(rebuilt[0].entry.id, "M-001");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn 召回明细可回看且采纳与否可机械判定() {
    let (dir, store) = temp_store();
    add(
        &store,
        "fact",
        "CRLF 是 edit 未命中主因",
        "处理 edit 替换失败必读",
        "自动容忍已落地",
    );
    add(
        &store,
        "sop",
        "发版 SOP 两条通道",
        "发版发布安装更新必读",
        "package.ps1 -Publish",
    );
    let index = SqliteMemoryIndex::new(&dir);
    let hits = index
        .search_entries(&IndexQuery::text("发版 更新"), None, Some("active"), 5)
        .unwrap();
    assert!(!hits.is_empty());

    let recall_id = store.record_recall("这轮要发版", &hits, 512);
    let rounds = store.recalls(10);
    assert_eq!(rounds.len(), 1, "召回明细未落库");
    assert_eq!(rounds[0].recall_id, recall_id);
    assert_eq!(
        rounds[0].injected_bytes, 512,
        "未记录注入字节数,上下文账单无从算起"
    );
    assert!(
        rounds[0].prompt_head.contains("发版"),
        "未记录触发本次召回的 prompt"
    );
    assert!(
        rounds[0].hits.iter().all(|h| h.score != 0.0),
        "未记录检索得分,看不出为什么召回这几条"
    );
    // 关键:召回但没拉正文 = 没起作用,不能默认算数。
    assert!(
        rounds[0].hits.iter().all(|h| !h.fetched),
        "刚召回还没拉正文就被判为已采纳,评估口径失真",
    );

    // 拉了正文才算采纳,而且只回填最近一次召回。
    let target = rounds[0].hits[0].id.clone();
    store.mark_recall_fetched(&target);
    let after = store.recalls(10);
    assert!(
        after[0]
            .hits
            .iter()
            .find(|h| h.id == target)
            .unwrap()
            .fetched,
        "拉取正文后未标记为已采纳",
    );
    assert!(
        after[0].hits.iter().filter(|h| h.fetched).count() == 1,
        "采纳标记不应扩散到同轮未被拉取的条目",
    );
    std::fs::remove_dir_all(dir).ok();
}

/// R-233 ②:intent_query 从用户 prompt 提取意图词——整句中文经虚词边界
/// 切段后,实词 bigram 不再被虚词错位(「批发/版出」→「发版」可命中)。
#[test]
fn intent_query_extracts_content_terms_around_boundaries() {
    // 发版:整句 bigram 会错位成 批发/版出;意图词提取后能出 发版。
    let q = intent_query("帮我把这一批发版出去");
    assert!(q.contains("发版"), "应提取出发版: {q}");
    assert!(!q.contains("帮"), "语气字不应进查询词: {q}");
    // ASCII 词原样保留(≥2 字母)。
    let q2 = intent_query("评估 harness 质量");
    assert!(q2.contains("harness"), "{q2}");
    assert!(q2.contains("质量"), "{q2}");
    // 纯虚词/语气 prompt → 空(调用方据此跳过检索)。
    let q3 = intent_query("请帮我一下好吗");
    assert!(q3.trim().is_empty(), "{q3}");
    // 长内容段拆 bigram 仍保留实词对。
    let q4 = intent_query("发版流程要走哪些步骤");
    assert!(q4.contains("发版") && q4.contains("步骤"), "{q4}");
    // 封顶 24 词。
    let long = format!("{} 结束", "甲".repeat(60));
    let q5 = intent_query(&long);
    assert!(q5.split_whitespace().count() <= 24, "{q5}");
    // 端到端召回:意图词必须真的命中含「发版」的条目,而非只满足
    // 字符串包含——「批发版」整段短语(逐字相邻)匹配不到「发版」,
    // 必须靠 3-4 字段补交的 bigram「发版」召回。
    let (dir, store) = temp_store();
    let e = add(
        &store,
        "sop",
        "发版 SOP",
        "发布流程检索钩子",
        "发布版本的操作步骤正文",
    );
    let rows = store
        .search_candidates(&intent_query("帮我把这一批发版出去"), None, Some("active"))
        .unwrap();
    assert!(
        rows.iter().any(|r| r.entry.id == e.id),
        "「批发版」整段短语匹配不到「发版」,必须靠 bigram 召回: {rows:?}"
    );
    std::fs::remove_dir_all(dir).ok();
}

/// R-166 批3(验收③):shadow 条目不注入生产检索但可被评估——默认检索
/// (无 status 或 active 过滤)不可见,显式 status=shadow 查询可见(评估器用);
/// shadow → active 需 provenance(candidate 同规则)。
#[test]
fn shadow_entry_is_evaluable_but_not_injected() {
    let (dir, store) = temp_store();
    let e = store
        .add(
            "fact",
            "影子评估条目",
            "影子钩子",
            "影子正文",
            "memory-manager",
            &[],
            None,
            false,
        )
        .unwrap();
    let AddOutcome::Added(candidate) = e else {
        panic!("expected Added")
    };
    // candidate → shadow。
    let shadowed = store.to_shadow(&candidate.id).unwrap();
    assert_eq!(shadowed.status, "shadow");
    // None is the explicit maintenance selector; production passes active.
    let hits_all = store.search_candidates("影子评估", None, None).unwrap();
    assert!(hits_all.iter().any(|h| h.entry.id == candidate.id));
    let hits_active = store
        .search_candidates("影子评估", None, Some("active"))
        .unwrap();
    assert!(
        hits_active.iter().all(|h| h.entry.id != candidate.id),
        "shadow 不得冒充 active"
    );
    // 显式查 shadow 可见(评估器通道)。
    let hits_shadow = store
        .search_candidates("影子评估", None, Some("shadow"))
        .unwrap();
    assert!(
        hits_shadow.iter().any(|h| h.entry.id == candidate.id),
        "显式查 shadow 应可见"
    );
    // shadow → active 仍需 provenance。
    let err = store.promote(&candidate.id, &[], None).unwrap_err();
    assert!(err.to_string().contains("no memory_sources evidence"));
    let eid = crate::memory::seed_episode(&dir, "ses");
    let promoted = store
        .promote(
            &candidate.id,
            &[(eid, Some(0), Some(5))],
            Some("shadow-hash"),
        )
        .unwrap();
    assert_eq!(promoted.status, "active");
    // 进 active 后默认检索可见。
    let hits_after = store.search_candidates("影子评估", None, None).unwrap();
    assert!(
        hits_after.iter().any(|h| h.entry.id == candidate.id),
        "active 后应可检索"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn find_by_marker_看得见candidate且吃老口径标记() {
    let (dir, store) = temp_store();
    store
        .append_note(
            "fixture source",
            "[fp:bash|`git merge` is blocked in bash: git mutations must use the structured `git` tool]",
            "fact",
            &[],
        )
        .unwrap();
    let entry = match store
        .add(
            "fact",
            "bash 里 git mutation 被拦",
            "git 操作被拒时必读",
            "正文\n[fp:bash|`git merge` is blocked in bash: git mutations must use the structured `git` tool]",
            "memory-manager",
            &[],
            None,
            // force:fixture 自造指纹验证 find_by_marker 归一,非真实写入口径
            // (R-216 指纹闸要求指纹先有来源 note;此处测的是 find 而非闸门)。
            true,
        )
        .unwrap()
    {
        AddOutcome::Added(e) => e,
        _ => panic!("应写入"),
    };
    assert_eq!(entry.status, "candidate");
    // 另一个子命令、同一道墙:归一后与正文里的老口径标记等价,
    // 且 candidate 不再隐形——否则 manager 会把同一个坑再记一遍。
    assert_eq!(
        store
            .find_by_marker("[fp:bash|`git restore` is blocked in bash: git mutations must use the structured `git` tool]")
            .map(|e| e.id),
        Some(entry.id.clone()),
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn 注入次数不同不能改变相关性排序() {
    let (dir, store) = temp_store();
    // 两条在 bm25 上等价的条目(标题仅一字之差,description/body 同构)。
    add(
        &store,
        "fact",
        "发版通道甲",
        "发版发布安装更新必读",
        "正文等长条目一",
    );
    add(
        &store,
        "fact",
        "发版通道乙",
        "发版发布安装更新必读",
        "正文等长条目二",
    );
    // D-366:排序决策在检索门面,经 index 取决策排序结果。
    let index = SqliteMemoryIndex::new(&dir);
    let hits = index
        .search_entries(&IndexQuery::text("发版"), None, Some("active"), 5)
        .unwrap();
    assert_eq!(hits.len(), 2);
    let (a, b) = ("M-001".to_string(), "M-002".to_string());
    // 各召回 3 轮:甲从未被拉正文,乙每轮都被拉。recall_id 以毫秒为键,轮间隔 2ms。
    for _ in 0..3 {
        record_current_recall(&store, &hits, &[&b]);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let profile = store.recall_profile();
    assert_eq!(profile.get(&a), Some(&(3, 0)), "{profile:?}");
    assert_eq!(profile.get(&b), Some(&(3, 3)), "{profile:?}");
    let ranked = index
        .search_entries(&IndexQuery::text("发版"), None, Some("active"), 5)
        .unwrap();
    assert_eq!(
        ranked
            .iter()
            .map(|hit| (&hit.entry.id, hit.score))
            .collect::<Vec<_>>(),
        hits.iter()
            .map(|hit| (&hit.entry.id, hit.score))
            .collect::<Vec<_>>()
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn 无读取证据不能降低任何分类的相关性分数() {
    // 偏好可经常驻上下文加载，读取工具并非唯一采用路径。
    // 「召回 3 采纳 0」不能在缺少实际读取证据时降低相关性分数。
    let (dir, store) = temp_store();
    add(
        &store,
        "preference",
        "发版定调甲",
        "发版发布安装更新必读",
        "正文等长条目一",
    );
    add(
        &store,
        "fact",
        "发版通道乙",
        "发版发布安装更新必读",
        "正文等长条目二",
    );
    // D-366:排序决策在检索门面,经 index 取决策排序结果。
    let index = SqliteMemoryIndex::new(&dir);
    let hits = index
        .search_entries(&IndexQuery::text("发版"), None, Some("active"), 5)
        .unwrap();
    assert_eq!(hits.len(), 2);
    for _ in 0..3 {
        record_current_recall(&store, &hits, &[]);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let ranked = index
        .search_entries(&IndexQuery::text("发版"), None, Some("active"), 5)
        .unwrap();
    assert_eq!(
        ranked
            .iter()
            .map(|hit| (&hit.entry.id, hit.score))
            .collect::<Vec<_>>(),
        hits.iter()
            .map(|hit| (&hit.entry.id, hit.score))
            .collect::<Vec<_>>()
    );
    std::fs::remove_dir_all(dir).ok();
}
