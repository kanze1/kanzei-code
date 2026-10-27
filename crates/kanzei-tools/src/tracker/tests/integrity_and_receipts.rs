#[tokio::test]
async fn tracker_write_and_repair_gates_refuse_unreadable_identity_sources() {
    for ledger in [false, true] {
        let (dir, store, tool, ctx) = m6_tracker_fixture("read-errors");
        if !ledger {
            std::fs::write(store.ledger_file(), "# Ledger\n").unwrap();
        }
        let source = if ledger {
            store.ledger_file()
        } else {
            store.archive_file()
        };
        std::fs::write(&source, b"old identity proof\xff").unwrap();
        let mut active = std::fs::read_to_string(&store.path).unwrap();
        active.push_str("\nOriginal user narrative to preserve.\n");
        std::fs::write(&store.path, active).unwrap();
        let paths = [
            store.path.clone(),
            store.archive_file(),
            store.ledger_file(),
        ];
        let before: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect();
        for action in [
            "raw_delete",
            "add",
            "update",
            "repair_reused_id",
            "repair_missing_id",
            "void_id",
            "fix_terminal",
            "normalize",
        ] {
            let input = if action == "raw_delete" {
                json!({"action":action,"id":"R-001","ordinal":1})
            } else {
                json!({"action":action,"id":"R-002","title":"Recovered real item",
                    "status":"todo","reason":"withdrawn with evidence","apply":true})
            };
            let output = tool.execute(input, &ctx).await;
            assert!(
                output.is_error,
                "{action} must not ignore unreadable identities: {}",
                output.content
            );
            assert!(
                output.content.contains("cannot verify tracker integrity"),
                "{action}: {}",
                output.content
            );
            assert!(output
                .content
                .contains(source.file_name().unwrap().to_str().unwrap()));
            for (path, expected) in paths.iter().zip(&before) {
                assert_eq!(
                    std::fs::read(path).unwrap(),
                    *expected,
                    "{action} changed {}",
                    path.display()
                );
            }
        }
        let error =
            super::scheduling::append_progress(&dir, &REQUIREMENTS, "R-001", "must not write")
                .unwrap_err();
        assert!(error.contains("cannot verify"));
        assert_eq!(std::fs::read(&store.path).unwrap(), before[0]);
        assert!(crate::write_log::entries_after(&dir, 0).unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn post_write_io_failure_keeps_matching_write_receipt_and_does_not_rewrite() {
    let (dir, store, tool, ctx) = m6_tracker_fixture("post-write-read-error");
    let _lock = store.lock().unwrap();
    let old = std::fs::read(&store.path).unwrap();
    let mut entries = store.load().unwrap();
    entries[0].title = "Actual successful write before verification failure".into();
    store.save(&entries).unwrap();
    let written = std::fs::read(&store.path).unwrap();
    assert_ne!(old, written);
    let modified = std::fs::metadata(&store.path).unwrap().modified().unwrap();
    // Deterministic boundary injection: the production action is complete,
    // then its identity source fails before the real production finalizer.
    std::fs::write(store.ledger_file(), b"old proof\xff").unwrap();
    let output = tool.finalize_action_result(
        &store,
        "update",
        &ctx,
        kanzei_harness::ToolOutput::ok("updated R-001"),
    );
    assert!(output.is_error);
    assert!(output.content.contains("already wrote its result"));
    assert!(output
        .content
        .contains("post-write integrity verification failed"));
    assert!(output.content.contains("requirements-ids.md"));
    assert_eq!(std::fs::read(&store.path).unwrap(), written);
    assert_eq!(
        std::fs::metadata(&store.path).unwrap().modified().unwrap(),
        modified,
        "the finalizer must not repeat the document write"
    );
    let logs = crate::write_log::entries_after(&dir, 0).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].path, REQUIREMENTS.rel_path);
    assert_eq!(logs[0].fingerprint, crate::content_hash(&written));
    assert!(logs[0].matches_content(Some(&written)));
    assert_eq!(logs[0].run_id.as_deref(), Some("m6-run"));
    drop(_lock);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn post_write_content_issue_stays_warning_with_valid_receipt() {
    let (dir, store, tool, ctx) = m6_tracker_fixture("post-write-content-warning");
    let _lock = store.lock().unwrap();
    store.save(&[entry("R-002")]).unwrap();
    let written = std::fs::read(&store.path).unwrap();
    let output = tool.finalize_action_result(
        &store,
        "update",
        &ctx,
        kanzei_harness::ToolOutput::ok("updated R-002"),
    );
    assert!(
        !output.is_error,
        "content issues retain the warning contract: {}",
        output.content
    );
    assert!(output.content.contains("tracker integrity"));
    assert!(output.content.contains("UNACCOUNTED"));
    let logs = crate::write_log::entries_after(&dir, 0).unwrap();
    assert_eq!(logs.len(), 1);
    assert!(logs[0].matches_content(Some(&written)));
    drop(_lock);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn archive_internal_verification_failure_keeps_actual_write_receipts() {
    for unreadable in [true, false] {
        let (dir, store, tool, ctx) = m6_tracker_fixture("archive-internal-failure");
        let _lock = store.lock().unwrap();
        let mut terminal = entry("R-001");
        terminal.status = "done".into();
        store.save(&[terminal]).unwrap();
        let moved = store.archive_terminal().unwrap();
        assert_eq!(moved, ["R-001"]);
        assert!(store.load().unwrap().is_empty());
        assert_eq!(store.load_archive().unwrap()[0].id, "R-001");
        // The same private verifier is called by production maintenance::archive
        // after this real lower write. No public hook or timing race is needed.
        if unreadable {
            let mut bytes = std::fs::read(store.archive_file()).unwrap();
            bytes.push(0xff);
            std::fs::write(store.archive_file(), bytes).unwrap();
        } else {
            std::fs::write(store.archive_file(), "# Requirements Archive\n").unwrap();
        }
        let active = std::fs::read(&store.path).unwrap();
        let archived = std::fs::read(store.archive_file()).unwrap();
        let active_stamp = std::fs::metadata(&store.path).unwrap().modified().unwrap();
        let archive_stamp = std::fs::metadata(store.archive_file())
            .unwrap()
            .modified()
            .unwrap();
        let action_output = tool.finalize_archive_move(&store, &ctx, moved);
        let output = tool.finalize_action_result(&store, "archive", &ctx, action_output);
        assert!(output.is_error);
        assert!(output.content.contains("already wrote its result"));
        assert!(output.content.contains("do not blindly retry"));
        assert!(output.content.contains("requirements-archive.md"));
        if unreadable {
            assert!(output.content.contains("cannot verify archived entries"));
        } else {
            assert!(
                output.content.contains("R-001 missing"),
                "{}",
                output.content
            );
        }
        assert_eq!(std::fs::read(&store.path).unwrap(), active);
        assert_eq!(std::fs::read(store.archive_file()).unwrap(), archived);
        assert_eq!(
            std::fs::metadata(&store.path).unwrap().modified().unwrap(),
            active_stamp
        );
        assert_eq!(
            std::fs::metadata(store.archive_file())
                .unwrap()
                .modified()
                .unwrap(),
            archive_stamp
        );
        let logs = crate::write_log::entries_after(&dir, 0).unwrap();
        assert_eq!(
            logs.len(),
            2,
            "actual archive writes need both receipts exactly once"
        );
        for (path, bytes) in [
            (REQUIREMENTS.rel_path, &active),
            (".kanzei/project/requirements-archive.md", &archived),
        ] {
            let matching: Vec<_> = logs.iter().filter(|log| log.path == path).collect();
            assert_eq!(matching.len(), 1);
            assert!(matching[0].matches_content(Some(bytes)));
            assert_eq!(matching[0].fingerprint, crate::content_hash(bytes));
        }
        drop(_lock);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn write_boundary_rejects_engine_and_unstructured_blocker() {
    let dir = std::env::temp_dir().join(format!(
        "kz-r356-write-boundary-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    store.save(&[entry("R-001")]).unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());

    let engine = tool
        .execute(
            json!({
                "action": "update",
                "id": "R-001",
                "fields": {"observed_head": "deadbeef"}
            }),
            &ctx,
        )
        .await;
    assert!(engine.is_error, "引擎字段写入必须拒绝: {}", engine.content);
    assert!(engine.content.contains("observed_head"));

    let blocker = tool
        .execute(
            json!({
                "action": "update",
                "id": "R-001",
                "fields": {"阻塞": "等待用户回复"}
            }),
            &ctx,
        )
        .await;
    assert!(
        blocker.is_error,
        "无解除条件的阻塞必须拒绝: {}",
        blocker.content
    );
    assert!(blocker.content.contains("解除条件"));
    assert!(
        store.load().unwrap()[0].fields.is_empty(),
        "拒绝写入不得改文档"
    );
}

/// R-184 批5:完整性破损时 append_progress 必须拒绝(与 TrackerTool 同一门禁)。
#[test]
fn append_progress_refuses_when_integrity_broken() {
    let dir = std::env::temp_dir().join(format!(
        "kz-append-progress-integrity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    // 编号断层(R-001 缺 R-002,直接上 R-003)→ 完整性门禁必须拦。
    let mut broken = entry("R-001");
    broken.fields.push(("进展".into(), "x".into()));
    store.save(&[broken, entry("R-003")]).unwrap();
    let err = super::append_progress(&dir, &REQUIREMENTS, "R-001", "x").unwrap_err();
    assert!(
        err.contains("tracker integrity is broken"),
        "完整性破损必须拒绝回写: {err}"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn 完整性破损时拒绝写操作但读操作放行() {
    // D-140:告警是 warn-only 时无人处理,实测缺号连响 5 个提交。
    let dir = std::env::temp_dir().join(format!(
        "kz-integrity-gate-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    // 制造缺号:R-001 与 R-003 存在,R-002 缺失。
    store.save(&[entry("R-001"), entry("R-003")]).unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());

    // 读操作必须仍然可用——否则模型无法诊断,就成了死锁。
    for action in ["list", "get"] {
        let mut input = json!({"action": action, "id": "R-001"});
        if action == "list" {
            input["reason"] = json!("deduplicate_registration");
        }
        let out = tool.execute(input, &ctx).await;
        assert!(!out.is_error, "读操作不该被拦: {action} -> {}", out.content);
    }
    // 写操作全部被拒,且错误里给出可执行的恢复路径。
    for input in [
        json!({"action": "add", "title": "新条目"}),
        json!({"action": "update", "id": "R-001", "status": "doing"}),
        json!({"action": "close", "id": "R-001"}),
        json!({"action": "archive"}),
        json!({"action": "reorder", "order": ["R-003", "R-001"]}),
    ] {
        let action = input["action"].as_str().unwrap().to_string();
        let out = tool.execute(input, &ctx).await;
        assert!(out.is_error, "写操作必须被拒: {action}");
        assert!(
            out.content.contains("R-002"),
            "要指名缺失的 id: {}",
            out.content
        );
        assert!(
            out.content.contains("git log -S"),
            "要给恢复路径: {}",
            out.content
        );
    }
    // 补齐缺号后写操作恢复正常。
    store
        .save(&[entry("R-001"), entry("R-002"), entry("R-003")])
        .unwrap();
    let out = tool
        .execute(json!({"action": "add", "title": "恢复后可写", "priority": "P2", "fields": {"复杂度": "中", "标签": "后端", "来源": "用户原话「测试」", "发现记录": "{\"Intent\":\"测试意图\",\"Explicit\":\"用户原话\",\"Assumptions\":\"无\",\"Ambiguities\":\"无\",\"领域对象\":\"条目\",\"最小成功闭环\":\"登记\",\"延后决策\":\"无\"}"}}), &ctx)
        .await;
    assert!(!out.is_error, "完整性恢复后应放行: {}", out.content);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn 复用历史_id_可改号修复且不会丢归档自由内容() {
    let dir = std::env::temp_dir().join(format!(
        "kz-reused-id-repair-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let project = dir.join(".kanzei/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("ideas.md"),
        "# Ideas\n\n## I-001 原始想法 [inbox]\n- 进展: 旧\n\n## I-002 新想法 [inbox]\n- 来源: R-093\n",
    )
    .unwrap();
    std::fs::write(
        project.join("ideas-archive.md"),
        "# Ideas Archive\n\n## I-002 旧想法 [split]\n- 验收: 拆解即 `idea update I-002 split`\n\n手写归档说明 I-002 不能丢\n\n## I-003 另一想法 [split]\n",
    )
    .unwrap();
    let tool = TrackerTool {
        tool_name: "idea",
        noun: "idea",
        kind: &IDEAS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());

    let repaired = tool
        .execute(json!({"action": "repair_reused_id", "id": "I-002"}), &ctx)
        .await;
    assert!(!repaired.is_error, "{}", repaired.content);
    assert!(repaired.content.contains("I-004"), "{}", repaired.content);
    let active = std::fs::read_to_string(project.join("ideas.md")).unwrap();
    let archive = std::fs::read_to_string(project.join("ideas-archive.md")).unwrap();
    assert!(active.contains("## I-002 新想法 [inbox]"));
    assert!(archive.contains("## I-004 旧想法 [split]"));
    assert!(archive.contains("idea update I-004 split"));
    assert!(archive.contains("手写归档说明 I-004 不能丢"));

    let updated = tool
        .execute(
            json!({"action": "update", "id": "I-001", "fields": {"进展": "新"}}),
            &ctx,
        )
        .await;
    assert!(
        !updated.is_error,
        "修复后普通写操作应恢复: {}",
        updated.content
    );
    let store = DocStore::open(&dir, &IDEAS);
    let entries = store.load().unwrap();
    assert!(store.integrity_issues(&entries).unwrap().is_empty());
    std::fs::remove_dir_all(dir).ok();
}

/// D-173:缺号有两条**结构化**出路,不必再靠伪造 `[wontfix]` 墓碑骗过门禁。
#[tokio::test]
async fn 缺号可注销或补回_两条修复通道都在门禁关闭时可用() {
    let dir = std::env::temp_dir().join(format!(
        "kz-id-ledger-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    // 两个空洞:R-002 其实是撤销的分配,R-003 是真丢了。
    store.save(&[entry("R-001"), entry("R-004")]).unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());

    // 门禁关闭:普通写被拒,且措辞点名三条修复通道、并禁止伪造墓碑。
    let out = tool
        .execute(json!({"action": "add", "title": "x"}), &ctx)
        .await;
    assert!(out.is_error);
    assert!(out.content.contains("void_id"), "{}", out.content);
    assert!(out.content.contains("repair_missing_id"), "{}", out.content);
    assert!(out.content.contains("fabricate"), "{}", out.content);

    // 注销必须给理由。
    let out = tool
        .execute(json!({"action": "void_id", "id": "R-002"}), &ctx)
        .await;
    assert!(out.is_error);
    assert!(out.content.contains("reason"), "{}", out.content);
    // 不能拿它注销一个还活着的条目。
    let out = tool
        .execute(
            json!({"action": "void_id", "id": "R-001", "reason": "想清掉它"}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "{}", out.content);

    let out = tool
        .execute(
            json!({"action": "void_id", "id": "R-002", "reason": "分配后当场撤销,git log -S 全历史无此条目"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    // 补回:必须给真实标题,并插回原编号位置。
    let out = tool
        .execute(
            json!({"action": "repair_missing_id", "id": "R-003", "title": "从 git 捞回的条目", "status": "doing"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let ids: Vec<String> = store.load().unwrap().iter().map(|e| e.id.clone()).collect();
    assert_eq!(ids, vec!["R-001", "R-003", "R-004"], "必须插回原位");

    // 两个空洞都交代完 → 完整性恢复,普通写放行,且注销过的号不再被复用。
    assert!(store
        .integrity_issues(&store.load().unwrap())
        .unwrap()
        .is_empty());
    let out = tool
        .execute(json!({"action": "add", "title": "恢复后可写", "priority": "P2", "fields": {"复杂度": "中", "标签": "后端", "来源": "用户原话「测试」", "发现记录": "{\"Intent\":\"测试意图\",\"Explicit\":\"用户原话\",\"Assumptions\":\"无\",\"Ambiguities\":\"无\",\"领域对象\":\"条目\",\"最小成功闭环\":\"登记\",\"延后决策\":\"无\"}"}}), &ctx)
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("R-005"), "{}", out.content);
    std::fs::remove_dir_all(&dir).ok();
}

/// 已注销的编号后来又冒出条目 = 账实不符的另一半,同样要报。
#[test]
fn 注销后又出现条目会被报为账实不符() {
    let dir = std::env::temp_dir().join(format!(
        "kz-ledger-conflict-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    store.save(&[entry("R-001")]).unwrap();
    store.void_id("R-002", "撤销的分配,有据可查").unwrap();
    store.save(&[entry("R-001"), entry("R-002")]).unwrap();
    let issues = store.integrity_issues(&store.load().unwrap()).unwrap();
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(issues[0].contains("voided"), "{issues:?}");
    assert!(issues[0].contains("R-002"), "{issues:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn integrity_warning_surfaces_in_tool_output() {
    // D-112:缺号(R-002)必须出现在每次成功调用的输出里。
    let dir = std::env::temp_dir().join(format!(
        "kz-tracker-integrity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    store.save(&[entry("R-001"), entry("R-003")]).unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let out = tool
        .execute(
            json!({"action": "list", "reason": "deduplicate_registration"}),
            &ToolCtx::new(dir.clone(), dir.clone()),
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("tracker integrity"), "{}", out.content);
    assert!(out.content.contains("R-002"), "{}", out.content);
    std::fs::remove_dir_all(&dir).ok();
}

/// R-268 批2:写动作(add)成功后必须产出写日志——围栏收口对账的归因凭据,
/// 「专用工具写文档」与「bash 越界写」由此可区分。
#[tokio::test]
async fn 写动作产出写日志_路径指纹与身份齐备() {
    let dir = std::env::temp_dir().join(format!(
        "kz-writelog-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone()).with_identity(
        "wt".into(),
        "key".into(),
        "run-1".into(),
        "proc-1".into(),
    );
    let out = tool
        .execute(
            json!({"action": "add", "title": "写日志验证条目", "priority": "P3", "tag": "后端", "complexity": "小"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);

    // 写日志落盘,且路径/指纹/身份与本次写入一致。
    let logs = crate::write_log::entries_after(&dir, 0).unwrap();
    assert_eq!(logs.len(), 1, "一次 add 应产出一条写日志");
    let log = &logs[0];
    assert_eq!(
        log.path, ".kanzei/project/requirements.md",
        "日志路径必须指向被写文档"
    );
    let on_disk = std::fs::read(&store.path).unwrap();
    assert_eq!(
        log.fingerprint,
        crate::content_hash(&on_disk),
        "日志指纹必须等于写后内容指纹"
    );
    assert_eq!(log.run_id.as_deref(), Some("run-1"));
    assert_eq!(log.process_id.as_deref(), Some("proc-1"));
    std::fs::remove_dir_all(dir).ok();
}
