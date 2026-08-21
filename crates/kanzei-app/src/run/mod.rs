//! 桌面 Agent Runtime 运行主链路(R-153 从 main.rs 拆出,R-253 二次拆解)。
//!
//! 独立理由:run.rs 承载「运行编排」这一整棵 application service 树——装配、
//! 事件归约、执行流水线、落库与协调;与编排零耦合的 IPC 已迁至 `crate::commands`,
//! 输入准入迁至 `run::input`,后续批次按生命周期继续拆(assembly/persistence/
//! execution/events/coordinator)。本文件最终收敛为 mod 声明与再导出。

pub(crate) mod assembly;
pub(crate) mod coordinator;
pub(crate) mod events;
pub(crate) mod execution;
pub(crate) mod input;
pub(crate) mod persistence;

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tauri::{Emitter, Window};

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

/// R-143:自举循环轮末自动 push——本轮确有 git commit 成功才触发(检测位由
/// run_task 的 on_event 在 ToolStart(action=commit)+ ToolEnd(ok=true) 置位)。
/// push 失败只上报不阻断:自举循环不能被网络/远端状态卡住(验收②);
/// 与既有手动 git push 流程共存,自动 push 只是把轮末该推的提交推掉(验收③)。
pub(crate) async fn maybe_push_after_commit(
    committed: bool,
    cwd: &std::path::Path,
    on_stage: &(dyn Fn(&str, String) + Sync),
    on_trace: &(dyn Fn(serde_json::Value) + Sync),
) {
    if !committed {
        return;
    }
    on_stage("推送", "本轮有提交,自动 git push…".into());
    let mut command = tokio::process::Command::new("git");
    // D-369:auto_push 在桌面端(GUI 无控制台)跑 git push,不隐藏会被 Windows
    // 新建控制台窗口——每次自动提交后都弹黑窗。与 state.rs hidden_command 同纪律。
    #[cfg(windows)]
    {
        command.creation_flags(0x0800_0000);
    }
    let output = command.arg("-C").arg(cwd).arg("push").output().await;
    let entry = match output {
        Ok(out) if out.status.success() => {
            json!({ "kind": "push", "ok": true, "at": now_ms() })
        }
        Ok(out) => {
            let detail = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let detail = if detail.is_empty() {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            } else {
                detail
            };
            on_stage("推送", format!("自动 push 失败(不阻断):{detail}"));
            json!({ "kind": "push", "ok": false, "error": detail, "at": now_ms() })
        }
        Err(error) => {
            on_stage("推送", format!("自动 push 失败(不阻断):{error}"));
            json!({ "kind": "push", "ok": false, "error": error.to_string(), "at": now_ms() })
        }
    };
    on_trace(entry);
}

pub(crate) fn emit_stage(window: &Window, session_id: &str, name: &str, detail: String) {
    let _ = window.emit(
        "kz:status",
        with_session_id(json!({ "stage": name, "detail": detail }), session_id),
    );
}

pub(crate) fn report_persistence_failure(
    window: &Window,
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
            "[profile]\ndefault = \"dev\"\n",
        )
        .unwrap();
        std::fs::write(
            worktree.join(".kanzei/kanzei.toml"),
            "[profile]\ndefault = \"research\"\n",
        )
        .unwrap();
        let (config, _) =
            kanzei_harness::KanzeiConfig::load_with_warnings_at_root(&main_root).unwrap();
        assert_eq!(
            config.profile.default.as_deref(),
            Some("dev"),
            "必须读主根那份配置;读到 research 说明取了 worktree 的分支副本"
        );
        std::fs::remove_dir_all(&worktree).ok();
        std::fs::remove_dir_all(&main_root).ok();
    }
}

#[cfg(test)]
mod auto_push_tests {
    use super::maybe_push_after_commit;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn temp_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-push-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "test@example.invalid"]);
        git(&repo, &["config", "user.name", "Kanzei Test"]);
        dir
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} 失败: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// 有提交 + 有 remote → push 成功,origin 收到该提交,轨迹记录 ok:true。
    #[tokio::test]
    async fn 本轮有提交_推送成功_远端收到() {
        let dir = temp_repo("ok");
        let repo = dir.join("repo");
        let remote = dir.join("remote.git");
        Command::new("git")
            .args(["init", "-q", "--bare"])
            .arg(&remote)
            .output()
            .unwrap();
        // bare 仓库默认 HEAD 分支名随 git 版本/config 漂移(master 或 main),
        // 钉死 refs/heads/main 让 rev-parse 断言与本地仓库分支名一致。
        git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        std::fs::write(repo.join("a.txt"), "hello\n").unwrap();
        git(&repo, &["add", "a.txt"]);
        git(&repo, &["commit", "-q", "-m", "第一条"]);
        // simple 模式无 upstream 的 `git push` 会拒绝;先建立 upstream(等价于
        // 手动 push 流程已跑过),再验证「轮末自动 push 把后续提交推上去」。
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        std::fs::write(repo.join("b.txt"), "second\n").unwrap();
        git(&repo, &["add", "b.txt"]);
        git(&repo, &["commit", "-q", "-m", "第二条"]);
        let local_head = git(&repo, &["rev-parse", "HEAD"]);

        let stages = std::sync::Mutex::new(Vec::new());
        let traces = std::sync::Mutex::new(Vec::new());
        maybe_push_after_commit(
            true,
            &repo,
            &|name, detail| stages.lock().unwrap().push(format!("{name}:{detail}")),
            &|entry| traces.lock().unwrap().push(entry),
        )
        .await;

        let stages = stages.into_inner().unwrap();
        let traces = traces.into_inner().unwrap();
        let remote_head = git(&remote, &["rev-parse", "main"]);
        assert_eq!(remote_head, local_head, "远端必须收到本轮提交");
        assert!(
            traces.iter().any(|e| e["ok"] == true),
            "轨迹应记 push 成功: {traces:?}"
        );
        assert!(
            !stages.iter().any(|s| s.contains("失败")),
            "成功路径不该报失败: {stages:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 本轮没有 commit(检测位 false)→ 根本不触发 push,零 stage/零 trace。
    #[tokio::test]
    async fn 本轮无提交_不触发push() {
        let dir = temp_repo("none");
        let repo = dir.join("repo");
        let stages = std::sync::Mutex::new(Vec::new());
        let traces = std::sync::Mutex::new(Vec::new());
        maybe_push_after_commit(
            false,
            &repo,
            &|name, detail| stages.lock().unwrap().push(format!("{name}:{detail}")),
            &|entry| traces.lock().unwrap().push(entry),
        )
        .await;
        assert!(
            traces.into_inner().unwrap().is_empty(),
            "无提交不应产生 push 轨迹"
        );
        assert!(
            stages.into_inner().unwrap().is_empty(),
            "无提交不应产生任何 stage 输出"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 有提交但没有 remote → push 失败,但函数不 panic、不阻断,失败经 stage 可见。
    #[tokio::test]
    async fn 有提交无remote_失败可见不panic() {
        let dir = temp_repo("noremote");
        let repo = dir.join("repo");
        std::fs::write(repo.join("a.txt"), "hello\n").unwrap();
        git(&repo, &["add", "a.txt"]);
        git(&repo, &["commit", "-q", "-m", "第一条"]);

        let stages = std::sync::Mutex::new(Vec::new());
        let traces = std::sync::Mutex::new(Vec::new());
        maybe_push_after_commit(
            true,
            &repo,
            &|name, detail| stages.lock().unwrap().push(format!("{name}:{detail}")),
            &|entry| traces.lock().unwrap().push(entry),
        )
        .await;

        let stages = stages.into_inner().unwrap();
        let traces = traces.into_inner().unwrap();
        assert!(
            stages.iter().any(|s| s.contains("失败")),
            "失败必须经 stage 可见: {stages:?}"
        );
        assert!(
            traces.iter().any(|e| e["ok"] == false),
            "轨迹应记 push 失败: {traces:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod assembly_tests {
    use kanzei_harness::{ConfigComponent, Harness, KanzeiConfig, ProfileKind, ResolveCtx};
    use kanzei_tools::{BaseComponent, DevProfile, ResearchProfile};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// D-361:只有「子代理内部一次工具调用已完成」的 trace 才计入本轮画像。
    #[test]
    fn 子代理画像上卷只认已完成的工具调用() {
        let trace = |phase: &str, name: &str| kanzei_core::TaskTrace {
            child_id: "child-1".into(),
            phase: phase.into(),
            name: name.into(),
            summary: None,
            ok: None,
            outcome: None,
            code: None,
            preview: None,
            artifact: None,
            display: None,
            input: None,
            usage: None,
            text: None,
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

    /// D-195:运行装配线必须注册前端自查段点名的每个工具。
    #[test]
    fn 桌面装配线必须注册前端自查段点名的每个工具() {
        let root = PathBuf::from("C:/kanzei-d195-app-test");
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness
            .add(BaseComponent)
            .add(DevProfile)
            .add(ResearchProfile)
            .add(crate::harness_ext::FrontendToolsComponent)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let tools: Vec<String> = snapshot
            .materialize_tools()
            .iter()
            .map(|tool| tool.name().to_string())
            .collect();
        let mentioned =
            kanzei_tools::prompt_tool_mentions(kanzei_tools::frontend_inspection_guidance());
        assert_eq!(mentioned.len(), 5);
        for tool in mentioned {
            assert!(
                tools.contains(&tool),
                "缺少前端自查工具 `{tool}`;已注册: {tools:?}"
            );
        }
    }

    // ══ R-240:按需求类型/复杂度聚合运行指标 ══

    #[test]
    fn extract_ticket_id_识别r与d条目() {
        use crate::commands::run::extract_ticket_id;
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
        use crate::commands::run::ticket_complexity;
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
        use crate::commands::run::aggregate_run_metrics;
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
