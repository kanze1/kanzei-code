/// R-308 B1:同一失败指纹即使改标题、改 category 并 force 也不能重复落盘。
#[test]
fn add_rejects_existing_fingerprint_even_with_force() {
    let (dir, store) = temp_store();
    store
        .append_note(
            "edit 未命中来源",
            "失败指纹 [fp:edit|not found]",
            "fact",
            &[],
        )
        .unwrap();
    let first = store
        .add(
            "fact",
            "首次记忆",
            "编辑失败时必读",
            "正文 [fp:edit|not found]",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    let AddOutcome::Added(first) = first else {
        panic!("首条指纹记忆应成功写入");
    };
    let duplicate = store
        .add(
            "sop",
            "改标题的重复记忆",
            "编辑失败的新钩子",
            "修复建议 [fp:edit|not found]",
            "user",
            &[],
            None,
            true,
        )
        .unwrap();
    assert!(matches!(duplicate, AddOutcome::Duplicate(entry) if entry.id == first.id));
    assert_eq!(store.load_all().len(), 1);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn exact_duplicate_title_is_rejected_unless_forced() {
    let (dir, store) = temp_store();
    add(
        &store,
        "habit",
        "gh 要走本地代理",
        "gh 网络失败时必读",
        "HTTPS_PROXY=127.0.0.1:12000",
    );
    let outcome = store
        .add(
            "habit",
            "gh 要走本地代理!",
            "重复",
            "x",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    assert!(matches!(outcome, AddOutcome::Duplicate(ref e) if e.id == "U-001" || e.id == "M-001"));
    let forced = store
        .add(
            "habit",
            "gh 要走本地代理!",
            "强制新增",
            "x",
            "user",
            &[],
            None,
            true,
        )
        .unwrap();
    assert!(matches!(forced, AddOutcome::Duplicate(_)));
    std::fs::remove_dir_all(dir).ok();
}

/// R-165 批2(验收④):novelty gate 三档分流——明显重复 NOOP、
/// 明显新 PROPOSE、不确定才留 LLM,计数落遥测表。
#[test]
fn novelty_gate_three_tiers_with_telemetry() {
    let (dir, store) = temp_store();
    store
        .add(
            "fact",
            "gh 网络代理",
            "push 前必读:设置 HTTPS_PROXY",
            "HTTPS_PROXY=http://127.0.0.1:12000",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    // 明显重复:规范化标题精确命中 active 记忆。
    let (dup, _) = store
        .classify_novelty("GH 网络代理", "push 前必读", "")
        .unwrap();
    assert_eq!(dup, Novelty::Duplicate, "规范化标题应命中 active 记忆");
    // 明显新:无重叠词。
    let (fresh, _) = store
        .classify_novelty("diff 树渲染优化", "R-133 diff 渲染", "")
        .unwrap();
    assert_eq!(fresh, Novelty::New, "无关主题应判明显新");
    // 不确定:有语义命中且标题高度重合——先建一条长标题条目,再 classify 一个
    // 共享 ≥8 token 的改写标题(R-216 口径:共享 token ≥8 且比例 ≥0.55)。
    let base = "secure configuring github network proxy connection and verification";
    store
        .add(
            "fact",
            base,
            "HTTPS_PROXY 与代理地址",
            "HTTPS_PROXY=http://127.0.0.1:12000",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    // 英文改写(调序+换词,共享全部 8+ token):R-216 验收①「英文改写 M-044 被拦」。
    let (uncertain, candidates) = store
        .classify_novelty(
            "verification and secure connection of github network proxy configuring",
            "HTTPS_PROXY 与代理地址",
            "",
        )
        .unwrap();
    assert_eq!(
        uncertain,
        Novelty::Uncertain,
        "高度重合英文改写应判不确定(留 add 硬闸拦截,验收①)"
    );
    assert!(!candidates.is_empty(), "Uncertain 应返回候选条目");
    assert!(
        candidates.iter().any(|c| c.title == base),
        "候选应含被改写的基础条目"
    );
    // 计数遥测落库。
    store.record_novelty(&dup, "", "GH 网络代理");
    store.record_novelty(&fresh, "", "diff 树渲染优化");
    store.record_novelty(&uncertain, "", base);
    let conn = store.open_db().unwrap();
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM novelty_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 3, "三档分流都要有计数遥测记录");
    let verdicts: Vec<String> = conn
        .prepare("SELECT verdict FROM novelty_events ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(verdicts, vec!["duplicate", "new", "uncertain"]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn subject_状态不变量_同主题至多一条_active_且_force_不可绕() {
    let (dir, store) = temp_store();
    let first = match store
        .add(
            "fact",
            "安装通道:NSIS 安装版",
            "查安装/更新通道时必读",
            "AppData 下",
            "user",
            &[],
            Some("安装通道"),
            false,
        )
        .unwrap()
    {
        AddOutcome::Added(e) => e,
        _ => panic!("expected add"),
    };
    // subject 写进 frontmatter,重读还原。
    let (path, _) = store
        .load_all()
        .into_iter()
        .find(|(_, e)| e.id == first.id)
        .unwrap();
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("subject: 安装通道"));

    // 标题不同、subject 相同 → 冲突,返回既有条目。
    let conflict = store
        .add(
            "fact",
            "安装通道改为便携版",
            "查安装通道必读",
            "新状态",
            "user",
            &[],
            Some("安装通道"),
            false,
        )
        .unwrap();
    assert!(matches!(conflict, AddOutcome::SubjectConflict(ref e) if e.id == first.id));
    // force 不可绕:状态不变量不是风格偏好。
    let forced = store
        .add(
            "fact",
            "安装通道改为便携版",
            "查安装通道必读",
            "新状态",
            "user",
            &[],
            Some("安装通道"),
            true,
        )
        .unwrap();
    assert!(matches!(forced, AddOutcome::SubjectConflict(ref e) if e.id == first.id));
    // 不同 category 同 subject 不冲突(键含 category)。
    assert!(matches!(
        store
            .add(
                "sop",
                "安装通道切换 SOP",
                "切换安装通道时必读",
                "步骤",
                "user",
                &[],
                Some("安装通道"),
                false
            )
            .unwrap(),
        AddOutcome::Added(_)
    ));
    // 旧状态 stale 后,同 subject 可重新建立(active 才占键)。
    store
        .update(&first.id, None, None, None, Some("stale"), None, false)
        .unwrap();
    assert!(matches!(
        store
            .add(
                "fact",
                "安装通道:便携版",
                "查安装通道必读",
                "新状态",
                "user",
                &[],
                Some("安装通道"),
                false
            )
            .unwrap(),
        AddOutcome::Added(_)
    ));
    let candidate = store
        .add(
            "sop",
            "安装通道候选状态",
            "安装通道候选验证时必读",
            "候选正文",
            "memory-manager",
            &[],
            Some("候选安装通道"),
            false,
        )
        .unwrap();
    assert!(matches!(candidate, AddOutcome::Added(ref e) if e.status == "candidate"));
    let candidate_duplicate = store
        .add(
            "sop",
            "安装通道候选状态更新",
            "安装通道候选验证时必读",
            "候选正文更新",
            "memory-manager",
            &[],
            Some("候选安装通道"),
            true,
        )
        .unwrap();
    assert!(matches!(
        candidate_duplicate,
        AddOutcome::SubjectConflict(_)
    ));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn add_拒绝空正文条目() {
    // 2026-08-12 清理时库里躺着 3 条只有 frontmatter 的条目(M-039/048/049),
    // 占编号、进 FTS、召回出来什么也没有。写入侧直接拒。
    let (dir, store) = temp_store();
    let err = match store.add(
        "fact",
        "标题在",
        "描述在",
        "   \n  ",
        "user",
        &[],
        None,
        false,
    ) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("空正文必须被拒"),
    };
    assert!(err.contains("body must not be empty"), "{err}");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn add_近似标题跨状态跨类目判重() {
    let (dir, store) = temp_store();
    // source != "user" → 落 candidate:未晋升条目隐形正是重复的生产线。
    let first = match store
        .add(
            "sop",
            "defect update 字段键名与多字段处理 SOP 防英文 key 追加与旧内容丢弃的脏数据陷阱",
            "更新字段时必读",
            "正文",
            "memory-manager",
            &[],
            None,
            false,
        )
        .unwrap()
    {
        AddOutcome::Added(e) => e,
        _ => panic!("首条应写入"),
    };
    assert_eq!(first.status, "candidate");
    // 换个说法、换个 category 再记一遍 —— 旧闸门(标题一字不差 + 同 category)
    // 一条都拦不住,这正是那 8 条重复的由来。
    match store
        .add(
            "fact",
            "defect/req update 字段键名与值处理 SOP 防脏数据",
            "缺陷更新时必读",
            "正文",
            "user",
            &[],
            None,
            false,
        )
        .unwrap()
    {
        AddOutcome::Duplicate(existing) => assert_eq!(existing.id, first.id),
        _ => panic!("近似重复没被拦住"),
    }
    // force 只绕过语义不确定闸，标题判重仍然生效。
    assert!(matches!(
        store
            .add(
                "fact",
                "defect/req update 字段键名与值处理 SOP 防脏数据",
                "缺陷更新时必读",
                "正文",
                "user",
                &[],
                None,
                true,
            )
            .unwrap(),
        AddOutcome::Duplicate(_)
    ));
    // 同子系统但确属两条知识的不能误杀(实测包含度 0.32,远低于阈值)。
    assert!(matches!(
        store
            .add(
                "sop",
                "活动归档同 ID 语义不同时用 repair_reused_id 修复勿直接编辑托管文档",
                "完整性门禁报同号时必读",
                "正文",
                "user",
                &[],
                None,
                false,
            )
            .unwrap(),
        AddOutcome::Added(_)
    ));
    std::fs::remove_dir_all(dir).ok();
}

/// R-216 验收②④:伪造指纹的 add 被拒——指纹必须来自 inbox 来源 note 或既有条目。
#[test]
fn 自造指纹的add被拒_来源note指纹放行() {
    let (dir, store) = temp_store();
    // 自造指纹(无来源)→ 拒绝。
    let err = store
        .add(
            "fact",
            "伪造指纹条目",
            "钩子",
            "正文 [fp:madeup|something]",
            "user",
            &[],
            None,
            false,
        )
        .unwrap_err();
    assert!(err.to_string().contains("禁止自造"), "{err}");
    // 来源 note 注入同指纹 → 放行。
    store
        .append_note("来源 note", "[fp:madeup|something]", "fact", &[])
        .unwrap();
    let ok = store
        .add(
            "fact",
            "合法指纹条目",
            "钩子",
            "正文 [fp:madeup|something]",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    assert!(matches!(ok, AddOutcome::Added(_)), "来源指纹应放行");
    std::fs::remove_dir_all(dir).ok();
}

/// R-216 验收③④:标题命中交付状态形态(R-/D- 编号 + 已交付/勿重复/验收边界)→ 拒,
/// 指路 tracker。交付状态是 tracker 的事,记忆记「怎么做」不记「哪个交付了」。
#[test]
fn 交付状态内容被拒并指路tracker() {
    let (dir, store) = temp_store();
    for bad_title in [
        "R-012 已交付,勿重复",
        "关于 D-044 的验收边界已达成",
        "R-055 delivered, do not repeat",
    ] {
        let err = store
            .add("fact", bad_title, "钩子", "正文", "user", &[], None, false)
            .unwrap_err();
        assert!(
            err.to_string().contains("tracker") && err.to_string().contains("交付"),
            "应指路 tracker: {err}"
        );
    }
    // 正常知识标题(含 R-/D- 引用但不含交付状态词)不受影响。
    let ok = store
        .add(
            "fact",
            "R-012 实现里 bash 前缀匹配的判定",
            "钩子",
            "正文",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    assert!(
        matches!(ok, AddOutcome::Added(_)),
        "含 R- 引用但非交付状态应放行"
    );
    std::fs::remove_dir_all(dir).ok();
}

/// R-216 验收①④:英文改写 M-044 场景被 add 硬闸拦截并返回候选,指路 memory_update。
#[test]
fn 英文改写被add硬闸拦截返回候选() {
    let (dir, store) = temp_store();
    let base = "secure configuring github network proxy connection and verification";
    store
        .add(
            "fact",
            base,
            "HTTPS_PROXY 与代理地址",
            "HTTPS_PROXY=http://127.0.0.1:12000",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    // 英文改写(共享 ≥8 token):Uncertain → add 硬闸拒并返回候选。
    let out = store
        .add(
            "fact",
            "verification and secure connection of github network proxy configuring",
            "HTTPS_PROXY 与代理地址",
            "HTTPS_PROXY=http://127.0.0.1:12000",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    match out {
        AddOutcome::Uncertain(candidates) => {
            assert!(
                candidates.iter().any(|c| c.title == base),
                "候选应含被改写的基础条目: {:?}",
                candidates
            );
        }
        other => panic!("英文改写应被 Uncertain 硬闸拦截: {other:?}"),
    }
    std::fs::remove_dir_all(dir).ok();
}
