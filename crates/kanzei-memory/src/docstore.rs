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
//!
//! 按域切分(R-257 B3):model(文档模型/批次)/ parse(解析)/ render(渲染)/
//! repository(存储核心)/ archive(归档)/ validation(编号台账·完整性·状态机)。
//! 零外部 API 面变更——顶层再导出保持原公共面。

#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use std::sync::{Arc, Mutex};

mod archive;
mod model;
pub use model::*;
mod parse;
pub use parse::*;
mod render;
pub use render::*;
mod repository;
pub use repository::*;
mod validation;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_store_isolates_source_and_finding_files() {
        let root = std::env::temp_dir().join(format!(
            "kz-topic-store-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let alpha = DocStore::open_topic(&root, &SOURCES, "alpha-study").unwrap();
        let beta = DocStore::open_topic(&root, &SOURCES, "beta-study").unwrap();
        assert_ne!(alpha.path, beta.path);
        alpha
            .save(&[Entry {
                id: "S-001".into(),
                title: "alpha source".into(),
                status: "active".into(),
                severity: None,
                fields: vec![],
            }])
            .unwrap();
        assert_eq!(beta.load().unwrap(), Vec::<Entry>::new());
        assert!(DocStore::open_topic(&root, &SOURCES, "../escape").is_err());
        assert!(DocStore::open_topic(&root, &SOURCES, "Alpha").is_err());
        std::fs::remove_dir_all(root).ok();
    }

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
    fn legacy_defect_metadata_is_detected_without_breaking_parentheses_titles() {
        let parsed = parse(&DEFECTS, "## D-553 旧缺陷 [open] (small) [fixed]\n");
        assert_eq!(parsed[0].status, "fixed");
        assert_eq!(parsed[0].title, "旧缺陷 [open] (small)");
        assert_eq!(
            invalid_severity_marker(&DEFECTS, &parsed[0].title),
            Some("small".into())
        );
        assert_eq!(clean_tracker_title(&DEFECTS, &parsed[0].title), "旧缺陷");

        let ordinary = "普通缺陷 (small)";
        assert_eq!(invalid_severity_marker(&DEFECTS, ordinary), None);
        assert_eq!(clean_tracker_title(&DEFECTS, ordinary), ordinary);
    }

    #[test]
    fn archived_terminal_fix_cleans_legacy_tracker_metadata() {
        let dir = std::env::temp_dir().join(format!(
            "kz-d569-archive-fix-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &DEFECTS);
        std::fs::write(
            store.archive_file(),
            "# Defects Archive\n\n## D-001 旧缺陷 [open] (small) [fixed]\n- 状态: done\n",
        )
        .unwrap();

        let issues = store.integrity_issues(&[]);
        assert_eq!(issues.len(), 3, "修复前应同时暴露三种污染: {issues:?}");
        store
            .correct_archived_terminal("D-001", "fixed", "D-569 存量完整性修复")
            .unwrap();
        let repaired = store.load_archive().unwrap();
        assert_eq!(repaired[0].title, "旧缺陷");
        assert_eq!(repaired[0].status, "fixed");
        assert!(!repaired[0].fields.iter().any(|(key, _)| key == "状态"));
        assert!(store.integrity_issues(&[]).is_empty());
        std::fs::remove_dir_all(dir).ok();
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
