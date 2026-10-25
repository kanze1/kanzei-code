//! 桌面 Agent Runtime 运行主链路(R-153 从 main.rs 拆出,R-253 二次拆解)。
//!
//! run 模块负责运行编排：装配、事件归约、执行流水线、落库与协调。
//! IPC 准入在 crate::commands，运行输入在 run::input，生命周期实现按
//! assembly/persistence/execution/events/coordinator 分域；本文件保留模块声明、再导出与共享 helper。

use crate::app_events::AppEvents;
pub(crate) mod assembly;
pub(crate) mod baseline;
pub(crate) mod coordinator;
pub(crate) mod events;
pub(crate) mod execution;
mod facts;
pub(crate) mod input;
pub(crate) mod persistence;
pub(crate) mod work_start;

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tauri::Emitter;

use crate::with_session_id;

/// D-297 验收③:TaskProgress 入参落 run.trace 时保留的字符上限。入参可能是完整
/// 工具调用 JSON(子代理勘察可带大文件内容),截断到 4K 字符足够复核调用意图,
/// 又不让单条轨迹事件把库体积与解析成本放大。
const TRACE_INPUT_KEEP_CHARS: usize = 4096;

/// R-236 B1：轮末触发线优先采用最近一次 provider 的真实 input usage；
/// 本轮没有有效 usage（冷启动、provider 未上报或返回 0）时才回落本地估算。
fn compaction_input_tokens(
    last_input_tokens: Option<u64>,
    messages: &[kanzei_llm::Message],
) -> u64 {
    last_input_tokens
        .filter(|tokens| *tokens > 0)
        .unwrap_or_else(|| kanzei_core::estimate_conversation_tokens(messages))
}

/// D-361:一条子代理 trace 是否该把工具名计入本轮画像,是则给出名字。
///
/// 只认 `phase == "end"`——那是子代理内部一次工具调用**已完成**的信号(subagent.rs
/// 由 ToolEnd 折算)。`start` 会重复计同一次调用,`usage`/`cancelled` 根本不带工具名。
/// 名字空白的 trace 不计:空名进画像等于凭空造出一个「有进展工具」。
fn subagent_round_tool(trace: &kanzei_core::TaskTrace) -> Option<&str> {
    if trace.phase != "end" {
        return None;
    }
    let name = trace.name.trim();
    (!name.is_empty()).then_some(name)
}
#[tauri::command]
pub(crate) fn app_info() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "build": option_env!("KANZEI_BUILD_INFO").unwrap_or("dev"),
    })
}

/// 当前墙钟毫秒(R-253:多个 command 模块与事件域共用,留在 run 模块再导出)。
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

pub(crate) fn emit_stage(window: &AppEvents, session_id: &str, name: &str, detail: String) {
    let _ = window.emit(
        "kz:status",
        with_session_id(json!({ "stage": name, "detail": detail }), session_id),
    );
}

pub(crate) fn report_persistence_failure(
    window: &AppEvents,
    session_id: &str,
    operation: &str,
    error: impl std::fmt::Display,
) {
    let message = format!("运行结果已保留，但{operation}失败: {error}");
    tracing::warn!("{message}");
    let _ = window.emit(
        "kz:error",
        with_session_id(json!({ "message": message, "terminal": false }), session_id),
    );
}

pub(crate) fn append_run_notification(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    status: &str,
    summary: impl Into<String>,
    requires_action: bool,
) -> anyhow::Result<()> {
    store.append_notification_atomic(session_id, status, &summary.into(), requires_action)?;
    Ok(())
}
#[cfg(test)]
mod worktree_run_tests {
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-run-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei")).unwrap();
        dir
    }

    /// R-177 验收③:配置取**主根**那一份,worktree 里的分支副本改了也不生效。
    /// run_task 用的就是这个入口(`load_with_warnings_at_root(&project_root)`),
    /// 配套的机械判据是本文件里发现式取根的配置入口零命中。
    #[test]
    fn 配置从主根加载_worktree副本改了不生效() {
        let main_root = temp_dir("cfg-main");
        let worktree = temp_dir("cfg-tree");
        std::fs::write(
            main_root.join(".kanzei/kanzei.toml"),
            "[profile]\ndefault = \"dev-auto\"\n",
        )
        .unwrap();
        std::fs::write(
            worktree.join(".kanzei/kanzei.toml"),
            "[profile]\ndefault = \"research\"\n",
        )
        .unwrap();
        let sources = kanzei_harness::project_trust::status(&main_root).unwrap();
        kanzei_harness::project_trust::approve(&main_root, &sources.fingerprint).unwrap();
        let (config, _) =
            kanzei_harness::KanzeiConfig::load_with_warnings_at_root(&main_root).unwrap();
        assert_eq!(
            config.profile.default.as_deref(),
            Some("dev-auto"),
            "必须读主根那份配置;读到 research 说明取了 worktree 的分支副本"
        );
        std::fs::remove_dir_all(&worktree).ok();
        std::fs::remove_dir_all(&main_root).ok();
    }
}

#[cfg(test)]
mod assembly_tests {
    use kanzei_harness::{ConfigComponent, Harness, KanzeiConfig, ProfileKind, ResolveCtx};
    use kanzei_tools::{BaseComponent, DevelopmentProfile, ResearchProfile};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// D-361:只有「子代理内部一次工具调用已完成」的 trace 才计入本轮画像。
    #[test]
    fn 子代理画像上卷只认已完成的工具调用() {
        let trace = |phase: &str, name: &str| kanzei_core::TaskTrace {
            child_id: "child-1".into(),
            phase: phase.into(),
            name: name.into(),
            ..Default::default()
        };
        assert_eq!(
            super::subagent_round_tool(&trace("end", "edit")),
            Some("edit"),
            "子代理调完 edit 必须上卷进画像"
        );
        assert_eq!(
            super::subagent_round_tool(&trace("start", "edit")),
            None,
            "start 会与 end 重复计同一次调用,不计"
        );
        assert_eq!(
            super::subagent_round_tool(&trace("usage", "")),
            None,
            "usage trace 不带工具名,不计"
        );
        assert_eq!(
            super::subagent_round_tool(&trace("cancelled", "")),
            None,
            "取消 trace 不带工具名,不计"
        );
        // UI-0926 #8:meta trace 只报人格与模型,不是一次工具调用。
        let meta = kanzei_core::TaskTrace {
            agent: Some("explore".into()),
            model: Some("qwen3:8b".into()),
            ..trace("meta", "")
        };
        assert_eq!(
            super::subagent_round_tool(&meta),
            None,
            "meta trace 不是工具调用,不计"
        );
        assert_eq!(
            super::subagent_round_tool(&trace("end", "   ")),
            None,
            "空名不得凭空造出一个「有进展工具」"
        );
    }

    #[test]
    fn 轮末压缩触发优先使用provider真实usage_无usage才估算() {
        let messages = vec![kanzei_llm::Message::user_text("本地估算内容")];
        let summary = kanzei_core::RunSummary {
            text: String::new(),
            usage: kanzei_llm::Usage::default(),
            last_input_tokens: Some(321),
            steps: 1,
            halted_by_user: false,
            step_limit_reached: false,
            finish_reason: Some(kanzei_llm::FinishReason::EndTurn),
            messages: messages.clone(),
            context_report: vec![],
            overflow_traces: vec![],
            round_messages: messages.clone(),
        };
        assert_eq!(
            super::compaction_input_tokens(summary.last_input_tokens, &messages),
            321
        );

        let cold = kanzei_core::RunSummary {
            last_input_tokens: None,
            ..summary
        };
        assert_eq!(
            super::compaction_input_tokens(cold.last_input_tokens, &messages),
            kanzei_core::estimate_conversation_tokens(&messages)
        );
    }

    #[test]
    fn 桌面工具schema字符账单增量() {
        let root = PathBuf::from("C:/kanzei-r364-b1-desktop-schema-bill");
        let collaboration_probe = crate::collaboration::CollaborationProbe::new(
            std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            root.clone(),
            "r364-budget-probe".into(),
        );
        let ctx = ResolveCtx {
            profile: ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness
            .add(BaseComponent)
            .add(DevelopmentProfile)
            .add(ResearchProfile)
            .add(crate::harness_ext::FrontendToolsComponent)
            .add(crate::collaboration::CollaborationComponent {
                probe: collaboration_probe,
            })
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let resident_names: Vec<&str> = snapshot
            .resident_tools()
            .iter()
            .map(|tool| tool.name())
            .collect();
        assert!(resident_names.contains(&"tool_search"));
        assert!(resident_names.contains(&"collaboration_status"));
        assert_eq!(
            resident_names.len() + 1,
            21,
            "桌面 resident 另含 core task_spec"
        );
        let deferred = snapshot.deferred_tools();
        assert_eq!(deferred.len(), 21);
        assert!(deferred.iter().any(|tool| tool.name() == "desktop"));
        assert!(!resident_names.contains(&"desktop"));
        let all_specs: Vec<kanzei_llm::ToolSpec> = snapshot
            .materialize_tools()
            .iter()
            .map(|tool| kanzei_llm::ToolSpec {
                name: tool.name().to_string(),
                description: tool.description(),
                input_schema: tool.input_schema(),
            })
            .collect();
        let ui_tools = crate::harness_ext::DESKTOP_DEFERRED_TOOLS;
        let specs: Vec<_> = all_specs
            .into_iter()
            .filter(|spec| ui_tools.contains(&spec.name.as_str()))
            .collect();
        for &expected in ui_tools {
            assert!(
                specs.iter().any(|spec| spec.name.as_str() == expected),
                "桌面增量工具缺失: {expected}"
            );
        }

        let mut rows: Vec<(&str, usize, usize)> = specs
            .iter()
            .map(|spec| {
                let bytes =
                    spec.name.len() + spec.description.len() + spec.input_schema.to_string().len();
                (spec.name.as_str(), spec.char_len(), bytes / 4)
            })
            .collect();
        rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
        eprintln!("Desktop Dev UI 增量 schema 字符账单:工具\t字符\tbytes/4 估算 tokens");
        for (name, chars, token_estimate) in &rows {
            eprintln!("{name}\t{chars}\t{token_estimate}");
        }
        let row_sum: usize = rows.iter().map(|row| row.1).sum();
        let spec_sum: usize = specs.iter().map(kanzei_llm::ToolSpec::char_len).sum();
        assert_eq!(row_sum, spec_sum, "桌面增量账单总数必须等于逐项之和");
        eprintln!("Desktop UI increment total={spec_sum}; collaboration_status 单独 probe");
    }

    // ══ R-240:按需求类型/复杂度聚合运行指标 ══

    #[test]
    fn extract_ticket_id_识别r与d条目() {
        use crate::commands::run::metrics::extract_ticket_id;
        assert_eq!(
            extract_ticket_id("R-202 run_task 拆分"),
            Some("R-202".into())
        );
        assert_eq!(extract_ticket_id("D-321 修复"), Some("D-321".into()));
        assert_eq!(extract_ticket_id("继续推进，规则按系统提示执行"), None);
        assert_eq!(extract_ticket_id(""), None);
        // 非需求编号的 R- 不误认(后面无数字)。
        assert_eq!(extract_ticket_id("README 说明"), None);
        // 多个编号取第一个。
        assert_eq!(
            extract_ticket_id("R-183 与 R-186 联动"),
            Some("R-183".into())
        );
    }

    #[test]
    fn ticket_complexity_从文档段落解析() {
        use crate::commands::run::metrics::ticket_complexity;
        let dir = std::env::temp_dir().join(format!(
            "kz-ticket-meta-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::write(
            dir.join(".kanzei/project/requirements.md"),
            "# Requirements\n\n## R-101 示例 [doing]\n- 优先级: P0\n- 复杂度: 中\n- 标签: 核心\n\n## R-102 无复杂度 [doing]\n- 优先级: P1\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(".kanzei/project/defects.md"),
            "# Defects\n\n## D-201 缺陷 [fixing]\n- 复杂度: 小\n",
        )
        .unwrap();
        assert_eq!(ticket_complexity(&dir, "R-101").as_deref(), Some("中"));
        assert_eq!(ticket_complexity(&dir, "D-201").as_deref(), Some("小"));
        assert_eq!(
            ticket_complexity(&dir, "R-102"),
            None,
            "无复杂度字段 → None"
        );
        assert_eq!(
            ticket_complexity(&dir, "R-999"),
            None,
            "不存在的条目 → None"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn aggregate_run_metrics_按类型与复杂度分组() {
        use crate::commands::run::metrics::aggregate_run_metrics;
        use std::collections::HashMap;
        let rows: Vec<(String, String, u32, u64, u64)> = vec![
            (
                "R-101 中复杂度任务".into(),
                "completed".into(),
                5,
                1000,
                200,
            ),
            (
                "R-101 中复杂度任务(二次)".into(),
                "completed".into(),
                3,
                800,
                100,
            ),
            ("R-102 小复杂度任务".into(), "completed".into(), 2, 300, 50),
            ("D-201 缺陷修复".into(), "completed".into(), 1, 150, 30),
            ("继续推进".into(), "completed".into(), 4, 500, 80),
        ];
        let mut metas = HashMap::new();
        metas.insert("R-101".into(), "中".into());
        metas.insert("R-102".into(), "小".into());
        metas.insert("D-201".into(), "小".into());
        let out = aggregate_run_metrics(&rows, &metas);
        let groups = out["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 3, "三个分类:R-中/R-小/D-小");
        let r101 = groups
            .iter()
            .find(|g| g["kind"] == "R" && g["complexity"] == "中")
            .unwrap();
        assert_eq!(r101["count"], 2);
        assert_eq!(r101["sumInput"], 1800);
        assert_eq!(r101["sumOutput"], 300);
        assert_eq!(r101["sumSteps"], 8);
        let d201 = groups
            .iter()
            .find(|g| g["kind"] == "D" && g["complexity"] == "小")
            .unwrap();
        assert_eq!(d201["count"], 1);
        // 无 ID 轮进 uncategorized。
        assert_eq!(out["uncategorized"]["count"], 1);
        assert_eq!(out["uncategorized"]["sumInput"], 500);
    }
}

#[cfg(test)]
mod persistence_boundary_tests {
    use super::append_run_notification;
    use kanzei_core::SessionStore;

    #[test]
    fn 轮末通知经真实存储边界可回放() {
        let store = SessionStore::open_in_memory().unwrap();
        let session_id = "session-run-boundary";
        store
            .create_session(session_id, "test-project", None)
            .unwrap();

        append_run_notification(&store, session_id, "running", "任务已开始", false).unwrap();
        append_run_notification(&store, session_id, "succeeded", "任务完成", false).unwrap();

        let notifications = store.replay_notifications(session_id, 0, 10).unwrap();
        assert_eq!(notifications.len(), 2);
        assert_eq!(notifications[0].sequence, 1);
        assert_eq!(notifications[0].status, "running");
        assert_eq!(notifications[1].sequence, 2);
        assert_eq!(notifications[1].status, "succeeded");
        assert_eq!(notifications[1].summary, "任务完成");
    }
}
