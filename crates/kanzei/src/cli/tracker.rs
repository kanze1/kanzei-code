//! `kz req|defect|source|finding|idea|decision` 人用直通(R-256 批3,纯搬迁自 main.rs)。
//!
//! 独立理由:tracker CLI 是「不经 LLM 直接调 tracker 工具」的直通层,`parse_tracker_flags`
//! 负责登记开关解析(add/update 共用),与 run/replay-eval 正交;拆出后 tracker 工具的
//! 动作面变更不必读懂 run 的装配(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):R-191 B3 起登记是硬约束,CLI 必须支持 --severity/--priority/
//! --complexity/--tag/--field,否则 `kz defect add` 一律被自己的门禁拒掉;D-359
//! `--reason` 是公共 flag(reopen/void_id/fix_terminal 强制必填);取根走显式主根
//! (D-267 教训:worktree 里发现式取根会命中分支副本)。

use kanzei_harness::{Tool, ToolCtx};

use super::{explicit_main_root, main_project_root};

/// 人用直通:不经 LLM,直接调 tracker 工具。
/// tracker 子命令的字段开关解析(add / update 共用),返回剩下的位置参数。
///
/// R-191 B3 起 req/defect 登记是硬约束(缺 severity/priority/复杂度/标签即拒),
/// 而这条 CLI 入口原先只会拼标题——`kz defect add` 一律被自己的门禁拒掉。
/// 支持:`--severity/-s`、`--priority/-p`、`--complexity`、`--tag`、
/// `--field 键=值`(可重复,写 复现/根因/影响/期望/验收/进展 等任意字段)。
/// 位置参数语义不变:add 拼成标题,update 取第一个作 id、第二个作 status。
pub(crate) fn parse_tracker_flags(args: &[String], input: &mut serde_json::Value) -> Vec<String> {
    let mut positional: Vec<String> = Vec::new();
    let mut fields = serde_json::Map::new();
    let mut rest = args.iter();
    while let Some(word) = rest.next() {
        match word.as_str() {
            "--severity" | "-s" => {
                if let Some(v) = rest.next() {
                    input["severity"] = serde_json::json!(v);
                }
            }
            "--priority" | "-p" => {
                if let Some(v) = rest.next() {
                    input["priority"] = serde_json::json!(v);
                }
            }
            // D-359:`--reason` 下沉为公共 flag。原先只有 fix_terminal 分支单独解析它,
            // 于是 reopen / void_id 这些**强制必填 reason** 的动作在命令行侧永远给不出
            // reason——`kz req reopen R-183 --reason "..."` 只会回一句 "`reason` is
            // required"。reopen 是「doing/fixing 推不动时退回初始态」的合法退路,退路
            // 在 CLI 不可用,僵尸 doing 就只能靠往阻塞字段里塞理由来挪出 WIP 槽(D-359 现场)。
            "--reason" => {
                if let Some(v) = rest.next() {
                    input["reason"] = serde_json::json!(v);
                }
            }
            "--complexity" => {
                if let Some(v) = rest.next() {
                    fields.insert("复杂度".into(), serde_json::json!(v));
                }
            }
            "--tag" => {
                if let Some(v) = rest.next() {
                    fields.insert("标签".into(), serde_json::json!(v));
                }
            }
            "--field" | "-f" => {
                if let Some(v) = rest.next() {
                    if let Some((key, value)) = v.split_once('=') {
                        fields.insert(key.trim().into(), serde_json::json!(value));
                    }
                }
            }
            other => positional.push(other.to_string()),
        }
    }
    if !fields.is_empty() {
        input["fields"] = serde_json::Value::Object(fields);
    }
    positional
}

pub(crate) async fn tracker_cli(args: &[String]) -> anyhow::Result<()> {
    use kanzei_tools::docstore::{DECISIONS, DEFECTS, FINDINGS, IDEAS, REQUIREMENTS, SOURCES};
    use kanzei_tools::tracker::TrackerTool;

    let tool = match args[0].as_str() {
        "idea" => TrackerTool {
            tool_name: "idea",
            noun: "idea",
            kind: &IDEAS,
            requires_refs: None,
        },
        "decision" => TrackerTool {
            tool_name: "decision",
            noun: "decision",
            kind: &DECISIONS,
            requires_refs: None,
        },
        "req" => TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        },
        "defect" => TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &DEFECTS,
            requires_refs: None,
        },
        "source" => TrackerTool {
            tool_name: "source",
            noun: "source",
            kind: &SOURCES,
            requires_refs: None,
        },
        "finding" => TrackerTool {
            tool_name: "finding",
            noun: "finding",
            kind: &FINDINGS,
            requires_refs: Some(&SOURCES),
        },
        _ => unreachable!(),
    };
    let action = args.get(1).map(String::as_str).unwrap_or("list");
    let mut input = serde_json::json!({ "action": action });
    if action == "list" {
        // 人在 CLI 主动查看仍允许；agent 运行期完整双队列由 tracker 守护拒绝。
        input["reason"] = serde_json::json!("human_cli");
    }
    match action {
        "get" | "close" | "update" | "repair_reused_id" => {
            // D-284:update 也要能写字段与进展。只收 id/status 的话 CLI 走不到关闭——
            // §1.25 要求验收证据必须在 close 前写进进展字段,close 后条目归档就改不动。
            let positional = parse_tracker_flags(&args[2..], &mut input);
            if let Some(id) = positional.first() {
                input["id"] = serde_json::json!(id);
            }
            if let Some(status) = positional.get(1) {
                input["status"] = serde_json::json!(status);
            }
        }
        // D-329:这些动作原先落在 `_ => {}`,位置参数 id 根本没接——CLI 一律报
        // "id is required",工具自己指路的清理通道(raw_lines/raw_delete)在命令行侧
        // 不可用。raw_delete 的第二个位置参数是序号。
        "raw_lines" | "reopen" | "archive" | "void_id" | "repair_missing_id" => {
            let positional = parse_tracker_flags(&args[2..], &mut input);
            if let Some(id) = positional.first() {
                input["id"] = serde_json::json!(id);
            }
        }
        // D-331:fix_terminal 是归档纠错动作,CLI 也要能直接调用(id + status + --reason)。
        // D-359:--reason 已由 parse_tracker_flags 统一解析,这里不再单独扫一遍 args。
        "fix_terminal" => {
            let positional = parse_tracker_flags(&args[2..], &mut input);
            if let Some(id) = positional.first() {
                input["id"] = serde_json::json!(id);
            }
            if let Some(status) = positional.get(1) {
                input["status"] = serde_json::json!(status);
            }
        }
        // D-332:normalize 是统一 repair surface(dry-run 默认,apply 落盘)。
        // CLI 形态:`kz req normalize` / `kz req normalize --apply` / `--status <合法值>`。
        "normalize" => {
            let _ = parse_tracker_flags(&args[2..], &mut input);
            if args.iter().any(|a| a == "--apply") {
                input["apply"] = serde_json::json!(true);
            }
            if let Some(pos) = args.iter().position(|a| a == "--status") {
                if let Some(v) = args.get(pos + 1) {
                    input["status"] = serde_json::json!(v);
                }
            }
        }
        // R-227:archive_fill 回填归档条目里的占位符测试 ID。
        // CLI 形态:`kz req archive_fill <id> <old> <new>`。
        "archive_fill" => {
            let positional = parse_tracker_flags(&args[2..], &mut input);
            if let Some(id) = positional.first() {
                input["id"] = serde_json::json!(id);
            }
            if let Some(old) = positional.get(1) {
                input["old"] = serde_json::json!(old);
            }
            if let Some(new) = positional.get(2) {
                input["new"] = serde_json::json!(new);
            }
        }
        "raw_delete" => {
            let positional = parse_tracker_flags(&args[2..], &mut input);
            if let Some(id) = positional.first() {
                input["id"] = serde_json::json!(id);
            }
            if let Some(ordinal) = positional.get(1).and_then(|raw| raw.parse::<u64>().ok()) {
                input["ordinal"] = serde_json::json!(ordinal);
            }
        }
        "add" => {
            let positional = parse_tracker_flags(&args[2..], &mut input);
            input["title"] = serde_json::json!(positional.join(" "));
        }
        _ => {}
    }
    // 追踪类子命令写的正是 .kanzei/project/*.md,和 run 一样不能落进 HOME(D-194)。
    // R-182 / D-267:这条入口原先是发现式取根,于是在 worktree 里第一层就命中被
    // checkout 出来的 `.kanzei` **分支副本**——两棵树相隔 10 秒各跑 `kz defect add`,
    // 各自在自己的副本上算 next_id,**都拿到 D-267**。改走显式主根:
    // `KANZEI_PROJECT_ROOT` 指哪写哪,没设时行为与今天逐字节相同。
    // (tracker 的位置参数会把 `add` 后面的词全部拼成标题,所以这条入口只认环境变量,
    //  不认 `--project-root` 开关。)
    let cwd = std::env::current_dir()?;
    let project_root = main_project_root(explicit_main_root(None).as_deref(), &cwd)?;
    let ctx = ToolCtx::new(cwd, project_root);
    let output = tool.execute(input, &ctx).await;
    if output.is_error {
        eprintln!("{}", output.content);
        std::process::exit(1);
    }
    println!("{}", output.content);
    Ok(())
}
