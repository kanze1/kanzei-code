#[test]
fn load_archived_reads_archive_dir() {
    let (dir, store) = temp_store();
    let live = add(&store, "fact", "活动条目", "图谱冒烟钩子", "正文");
    let archive = store.root.join("archive");
    std::fs::create_dir_all(&archive).unwrap();
    let mut old = live.clone();
    old.id = "M-900".into();
    old.title = "归档条目".into();
    old.status = "deprecated".into();
    std::fs::write(archive.join("M-900-old.md"), render_entry(&old)).unwrap();
    std::fs::write(archive.join("notes.txt"), "不是条目").unwrap();
    let archived = store.load_archived();
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].1.id, "M-900");
    assert_eq!(archived[0].1.status, "deprecated");
    assert!(
        store.load_all().iter().all(|(_, e)| e.id != "M-900"),
        "load_all 不含归档"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn set_area_writes_dedups_and_empty_clears() {
    let (dir, store) = temp_store();
    let entry = add(&store, "fact", "区域条目", "图谱区域钩子", "正文");
    let updated = store
        .set_area(
            &entry.id,
            &[
                "kanzei-tools/edit".into(),
                "kanzei-tools/edit".into(),
                " scripts ".into(),
            ],
        )
        .unwrap();
    assert_eq!(updated.areas(), vec!["kanzei-tools/edit", "scripts"]);
    let text = std::fs::read_to_string(
        store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == entry.id)
            .unwrap()
            .0,
    )
    .unwrap();
    assert!(
        text.contains(
            "area: kanzei-tools/edit scripts
"
        ),
        "{text}"
    );
    let cleared = store.set_area(&entry.id, &[]).unwrap();
    assert!(cleared.areas().is_empty());
    assert!(cleared.extras.iter().all(|(k, _)| k != "area"));
    assert!(store.set_area("M-404", &["scripts".into()]).is_err());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn update_evolves_and_stale_downranks() {
    let (dir, store) = temp_store();
    let e = add(&store, "fact", "旧结论", "某场景必读", "V1");
    let updated = store
        .update(
            &e.id,
            None,
            None,
            Some("V2 修订"),
            Some("stale"),
            None,
            false,
        )
        .unwrap();
    assert_eq!(updated.status, "deprecated"); // R-165:stale 兼容映射 deprecated
    assert_eq!(updated.body, "V2 修订");
    assert_eq!(updated.created, e.created);
    // stale 默认不出现在 active 过滤下(存储侧候选集过滤语义)
    let active_only = store
        .search_candidates("结论", None, Some("active"))
        .unwrap();
    assert!(active_only.is_empty());
    std::fs::remove_dir_all(dir).ok();
}

/// R-165 批1:编译产物(candidate)必须带 episode 证据才能 promote 进 active——
/// 无 provenance 的记忆永不可注入检索,引擎强制不靠 manager 自觉。
#[test]
fn promote_requires_provenance_hard_gate() {
    let (dir, store) = temp_store();
    // source != user → candidate(manager 编译产物)
    let e = store
        .add(
            "fact",
            "编译事实",
            "编译钩子",
            "body",
            "memory-manager",
            &[],
            None,
            false,
        )
        .unwrap();
    let AddOutcome::Added(candidate) = e else {
        panic!("expected Added");
    };
    assert_eq!(candidate.status, "candidate");
    // 无证据 → 拒绝
    let err = store.promote(&candidate.id, &[], None).unwrap_err();
    assert!(err.to_string().contains("no memory_sources evidence"));
    // 状态仍是 candidate
    let (_, after) = store
        .load_all()
        .into_iter()
        .find(|(_, e)| e.id == candidate.id)
        .unwrap();
    assert_eq!(after.status, "candidate");
    // 有证据 → 晋升 active(R-213:episode 必须真实存在,先 seed 真实轮次)
    let eid = crate::memory::seed_episode(&dir, "ses");
    let promoted = store
        .promote(
            &candidate.id,
            &[(eid, Some(0), Some(10))],
            Some("test-hash"),
        )
        .unwrap();
    assert_eq!(promoted.status, "active");
    // 晋升后再次 promote 拒绝(candidate|shadow 才可晋升,active 不行)
    let err2 = store
        .promote(&candidate.id, &[(2, None, None)], None)
        .unwrap_err();
    assert!(err2
        .to_string()
        .contains("only candidate|shadow can be promoted"));
    std::fs::remove_dir_all(dir).ok();
}

/// R-165 批1:source=="user" 用户直写是最高权证据,直接 active(不走编译门禁)。
#[test]
fn user_written_entry_is_active_directly() {
    let (dir, store) = temp_store();
    let e = store
        .add(
            "preference",
            "开发重心",
            "取活必读",
            "先清缺陷",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    let AddOutcome::Added(entry) = e else {
        panic!("expected Added");
    };
    assert_eq!(entry.status, "active");
    assert!(store
        .load_all()
        .iter()
        .any(|(_, e)| e.id == entry.id && e.status == "active"));
    std::fs::remove_dir_all(dir).ok();
}

/// R-165 批2(验收②):recurrence 三段晋升——跨轮计数递增,
/// 第 1 次=1、第 2 次=2、第 3 次=3,harvest_failures 按档位写笔记。
#[test]
fn recurrence_three_stage_promotion_counts() {
    let (dir, store) = temp_store();
    let fp = format!(
        "[fp:edit|old string not found #{}-{}]",
        std::process::id(),
        "rec"
    );
    assert_eq!(store.recurrence_count(&fp), 0, "未出现前计数为 0");
    assert_eq!(store.bump_recurrence(&fp), 1, "第 1 次出现");
    assert_eq!(
        store.bump_recurrence(&fp),
        2,
        "第 2 次出现 → candidate 档位"
    );
    assert_eq!(store.bump_recurrence(&fp), 3, "第 3 次出现 → promote 档位");
    assert_eq!(store.recurrence_count(&fp), 3, "只读查询不递增");
    assert_eq!(store.bump_recurrence(&fp), 4);
    // 计数持久化跨 store 实例(同一 index.db)。
    let reopened = MemoryStore::open(store.scope, store.root.clone());
    assert_eq!(reopened.recurrence_count(&fp), 4, "持久计数跨实例可见");
    std::fs::remove_dir_all(dir).ok();
}

/// R-165 批4(验收⑤):evidence(memory_sources)无自治写路径——
/// 只有 promote() 写它,且每条证据落在 state.db 可 join。
/// (代码审计:record_memory_source 生产调用方唯一 = store.rs promote 内。)
#[test]
fn promote_is_sole_evidence_writer_and_rows_land() {
    let (dir, store) = temp_store();
    let e = store
        .add(
            "fact",
            "证据编译条目",
            "证据钩子",
            "正文",
            "memory-manager",
            &[],
            None,
            false,
        )
        .unwrap();
    let AddOutcome::Added(candidate) = e else {
        panic!("expected Added")
    };
    // 先初始化 state.db schema(promote 内 open 依赖 schema;测试环境无现成库)。
    let path = dir.join(".kanzei").join("state.db");
    kanzei_core::SessionStore::open(&path).unwrap();
    // episode 7 必须先存在:memory_sources 有外键 REFERENCES episodes。
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO episodes(episode_id, session_id, created_at, prompt_head, outcome, steps,
                                      input_tokens, output_tokens, tools_json, context_json)
                 VALUES (7, 'test-session', 0, '测试轮', 'ok', 1, 10, 10, '[]', '{}')",
            [],
        )
        .unwrap();
    }
    store
        .promote(
            &candidate.id,
            &[(7, Some(100), Some(200))],
            Some("audit-hash"),
        )
        .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (memory_id, episode_id, source_hash): (String, i64, String) = conn
        .query_row(
            "SELECT memory_id, episode_id, source_hash FROM memory_sources WHERE memory_id = ?1",
            params![candidate.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(memory_id, candidate.id);
    assert_eq!(episode_id, 7);
    assert_eq!(source_hash, "audit-hash");
    std::fs::remove_dir_all(dir).ok();
}

/// R-213 验收①:伪造 episode_id 的 promote 被拒——「无来源不入 active」必须是
/// 「来源指向真实轮次」,编造一个 episodes 表里不存在的 id 不得蒙混晋升。
#[test]
fn promote_rejects_fabricated_episode_id() {
    let (dir, store) = temp_store();
    let e = store
        .add(
            "fact",
            "伪造证据条目",
            "伪造钩子",
            "正文",
            "memory-manager",
            &[],
            None,
            false,
        )
        .unwrap();
    let AddOutcome::Added(candidate) = e else {
        panic!("expected Added")
    };
    // state.db 里没有任何 episode,999_999 必然不存在。
    let err = store
        .promote(&candidate.id, &[(999_999, None, None)], None)
        .unwrap_err();
    assert!(
        err.to_string().contains("does not exist"),
        "伪造 episode_id 应被拒: {err}"
    );
    // 状态仍 candidate,未晋升。
    let (_, after) = store
        .load_all()
        .into_iter()
        .find(|(_, e)| e.id == candidate.id)
        .unwrap();
    assert_eq!(after.status, "candidate");
    std::fs::remove_dir_all(dir).ok();
}

/// R-213 验收②:写证据失败不产生 active 条目——episode 校验通过后,evidence
/// 落库若失败(如 memory_sources 表被破坏),promote 必须整体失败且条目保持原状态。
#[test]
fn promote_write_evidence_failure_does_not_activate() {
    let (dir, store) = temp_store();
    let e = store
        .add(
            "fact",
            "证据写失败条目",
            "写失败钩子",
            "正文",
            "memory-manager",
            &[],
            None,
            false,
        )
        .unwrap();
    let AddOutcome::Added(candidate) = e else {
        panic!("expected Added")
    };
    // seed 真实 episode,让 episode_exists 校验通过。
    let eid = crate::memory::seed_episode(&dir, "ses");
    // 人为制造证据写失败:drop memory_sources 表(migrate 版本已对齐不会再重建)。
    let db = dir.join(".kanzei").join("state.db");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("DROP TABLE memory_sources").unwrap();
    }
    let err = store
        .promote(&candidate.id, &[(eid, Some(0), Some(5))], Some("test"))
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("failed to record memory_source evidence"),
        "写证据失败应整体失败: {err}"
    );
    // 未晋升:仍是 candidate。
    let (_, after) = store
        .load_all()
        .into_iter()
        .find(|(_, e)| e.id == candidate.id)
        .unwrap();
    assert_eq!(after.status, "candidate");
    std::fs::remove_dir_all(dir).ok();
}

/// R-165 批3(验收③):deprecated/invalid 移入 archive/ 且默认检索不可见(D-231)。
#[test]
fn deprecated_moves_to_archive_and_hidden_from_search() {
    let (dir, _) = temp_store();
    let store = MemoryStore::project(&dir);
    let e = store
        .add(
            "fact",
            "将被推翻的结论",
            "旧钩子",
            "旧内容",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    let AddOutcome::Added(entry) = e else {
        panic!("expected Added")
    };
    // 设置 deprecated → refresh_derived 自动归档。
    store
        .update(&entry.id, None, None, None, Some("deprecated"), None, false)
        .unwrap();
    // 主目录已无该文件,archive/ 有墓碑。
    assert!(!store
        .root
        .join(format!("{}.md", entry.file_stem()))
        .exists());
    let logs = kanzei_base::write_log::entries_after(&dir, 0).unwrap();
    let source_path = format!(".kanzei/memory/{}.md", entry.file_stem());
    let archive_path = format!(".kanzei/memory/archive/{}.md", entry.file_stem());
    assert!(
        logs.iter().any(|log| {
            log.path == source_path
                && log.content == kanzei_base::write_log::LoggedContent::Deleted
                && log.fingerprint == kanzei_base::content_hash(&[])
        }),
        "归档必须为源文件删除留下写日志: {logs:?}"
    );
    let archived_bytes = std::fs::read(
        store
            .archive_dir()
            .join(format!("{}.md", entry.file_stem())),
    )
    .unwrap();
    assert!(
        logs.iter().any(|log| {
            log.path == archive_path
                && log.fingerprint == kanzei_base::content_hash(&archived_bytes)
        }),
        "归档必须为 archive 目标留下写日志: {logs:?}"
    );
    // load_all 不含它(默认检索范围),load_archived_ids 保留 ID 防复用。
    assert!(store
        .archive_dir()
        .join(format!("{}.md", entry.file_stem()))
        .exists());
    // load_all 不含它(默认检索范围),load_archived_ids 保留 ID 防复用。
    assert!(store.load_all().iter().all(|(_, e)| e.id != entry.id));
    assert!(store.load_archived_ids().contains(&entry.id));
    // 默认检索(search 无 status 过滤或 active 过滤)都不可见。
    let hits = store.search_candidates("将被推翻", None, None).unwrap();
    assert!(
        hits.iter().all(|h| h.entry.id != entry.id),
        "归档条目不得被检索"
    );
    // invalid 同样归档。
    let i = store
        .add(
            "fact",
            "证伪的假设",
            "证伪钩子",
            "证伪内容",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    let AddOutcome::Added(invalid) = i else {
        panic!("expected Added")
    };
    store
        .update(&invalid.id, None, None, None, Some("invalid"), None, false)
        .unwrap();
    assert!(!store.load_all().iter().any(|(_, e)| e.id == invalid.id));
    assert!(store
        .archive_dir()
        .join(format!("{}.md", invalid.file_stem()))
        .exists());
    std::fs::remove_dir_all(dir).ok();
}

/// R-166 批3:非 candidate 不能进 shadow;shadow 不落入 archive_dead(它是
/// 中间态,不是失效态)——refresh_derived 后主目录仍保留 shadow 文件。
#[test]
fn shadow_rejects_non_candidate_and_survives_refresh() {
    let (dir, store) = temp_store();
    let e = store
        .add(
            "fact",
            "直写活跃条目",
            "活跃钩子",
            "正文",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    let AddOutcome::Added(active) = e else {
        panic!("expected Added")
    };
    assert_eq!(active.status, "active");
    let err = store.to_shadow(&active.id).unwrap_err();
    assert!(err.to_string().contains("only candidate can enter shadow"));
    // candidate 进 shadow 后 refresh_derived(任意写触发)不归档它。
    let c = store
        .add(
            "fact",
            "影子常驻",
            "常驻钩子",
            "正文",
            "memory-manager",
            &[],
            None,
            false,
        )
        .unwrap();
    let AddOutcome::Added(candidate) = c else {
        panic!("expected Added")
    };
    store.to_shadow(&candidate.id).unwrap();
    // 用一次无关 update 触发 refresh_derived。
    store
        .update(
            &active.id,
            Some("直写活跃条目改名"),
            None,
            None,
            None,
            None,
            false,
        )
        .unwrap();
    assert!(
        store.load_all().iter().any(|(_, e)| e.id == candidate.id),
        "shadow 是中间态,不得被归档"
    );
    assert_eq!(
        store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == candidate.id)
            .unwrap()
            .1
            .status,
        "shadow"
    );
    std::fs::remove_dir_all(dir).ok();
}

/// D-282 ①:description 主题一致性。新 description 与条目现有主题无任何共同词
/// (title/正文)时拒绝——防 manager 把 tracker 字段语义条目的钩子换成 edit 主题
/// 内容(实测 M-044 被覆盖)。同主题演化放行。
#[test]
fn update拒绝主题漂移的description() {
    let (dir, store) = temp_store();
    let e = add(
        &store,
        "sop",
        "tracker update 字段语义",
        "处理 req/defect update 写字段时必读",
        "中文键精确匹配;英文键会追加;多行会产生游离段落",
    );
    // 主题漂移:edit 替换主题与 tracker 字段语义交集 <2 → 拒绝(D-282 真实形态)。
    let drifted = store
        .update(
            &e.id,
            None,
            Some("edit 替换时确认 allow_deletion 防止误删"),
            None,
            None,
            None,
            true,
        )
        .unwrap_err();
    assert!(
        drifted.to_string().contains("共同词过少"),
        "漂移 description 必须被拒: {drifted}"
    );
    // 条目内容未被改写:description 仍是旧钩子(漂移被拒后原样保留)。
    let after = store
        .load_all()
        .into_iter()
        .find(|(_, x)| x.id == e.id)
        .unwrap()
        .1;
    assert_eq!(
        after.description, "处理 req/defect update 写字段时必读",
        "漂移被拒后 description 不得被改写: {}",
        after.description
    );
    // 同主题演化(保留旧主题词)放行。
    store
        .update(
            &e.id,
            None,
            Some("处理 req/defect update 写字段时必读(含中文键/英文键/游离段落)"),
            None,
            None,
            None,
            true,
        )
        .unwrap();
    std::fs::remove_dir_all(dir).ok();
}

/// D-282 ②:CAS——传 expected_hash 且期间有并发写时拒绝,防止人工维护与
/// 轮末 manager 互相覆盖。
#[test]
fn update_cas拒绝过期expected_hash() {
    let (dir, store) = temp_store();
    let e = add(&store, "fact", "主题甲", "钩子甲", "正文甲");
    let render = |id: &str| {
        let entry = store
            .load_all()
            .into_iter()
            .find(|(_, x)| x.id == id)
            .unwrap()
            .1;
        render_entry(&entry)
    };
    let stale_hash = kanzei_base::content_hash(render(&e.id).as_bytes());
    // 期间别人改了条目(改 title)——模拟非 manager 写,豁免主题校验。
    store
        .update(&e.id, Some("主题甲改"), None, None, None, None, false)
        .unwrap();
    // 拿着旧 hash 再写 → 拒绝。
    let cas_err = store
        .update(
            &e.id,
            None,
            Some("钩子甲修订"),
            None,
            None,
            Some(&stale_hash),
            true,
        )
        .unwrap_err();
    assert!(
        cas_err.to_string().contains("已被并发修改"),
        "过期 hash 必须拒绝: {cas_err}"
    );
    // 用新 hash 写 → 放行。
    let fresh_hash = kanzei_base::content_hash(render(&e.id).as_bytes());
    store
        .update(
            &e.id,
            None,
            Some("钩子甲修订"),
            None,
            None,
            Some(&fresh_hash),
            true,
        )
        .unwrap();
    std::fs::remove_dir_all(dir).ok();
}
