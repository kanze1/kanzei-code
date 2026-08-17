//! test_record 工具:测试记录 `.kanzei/project/tests.md` 的专用写通道。
//!
//! R-080 的根因是「权限严了却没有配套工具」的另一个实例:`.kanzei/project/*`
//! 对 write/edit 硬 deny、shell 对托管目录回滚,而测试记录没有任何专用写入
//! 通道——test_run_record 是 Tauri 命令,agent 侧没有对应工具,于是 tests.md
//! 永远不存在,左侧栏永远显示"暂无测试记录",归档分支永不执行。
//!
//! 本工具把解析/快照/自动归档/追加记录逻辑下沉到 kanzei-tools:
//! - app 的 `test_runs_snapshot` / `test_run_record` 改为薄封装调用本模块,
//!   避免两套格式解析与归档逻辑漂移;
//! - agent 侧获得 `test_record` 工具:跑完测试后记录一条,状态
//!   running/passed/failed/skipped,终态(passed/failed/skipped)由快照自动
//!   归档进 tests-archive.md,左侧栏展示 active + archived。

use std::path::Path;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

/// 测试记录(相对项目根)。
pub const TEST_RUNS_REL: &str = ".kanzei/project/tests.md";
/// 测试记录归档(相对项目根)。
pub const TEST_RUNS_ARCHIVE_REL: &str = ".kanzei/project/tests-archive.md";

const VALID_STATUS: &[&str] = &["running", "passed", "failed", "skipped"];

/// 工具文本的渲染上限(口径照 grep.rs 的 DEFAULT_LIMIT / MAX_LINE_CHARS)。
///
/// 存在的理由:render_snapshot 原先把**整份归档索引**逐条无上限拼进返回文本。
/// 实测 2026-08-15:归档 683 条 / 241KB,渲染出的 `○` 行合计约 5 万字符,而活动
/// 记录 0 条——每次调用回灌的内容 ≥99% 是与本次写入无关的历史清单。更糟的是它
/// 单调增长:每记一条终态测试,此后**所有**调用永久加约 73 字符,同一会话里后面
/// 的调用比前面更贵。这不是某次输出偏大,是随项目寿命线性膨胀的上下文税
/// (实测占全部工具结果体量的 38%,是 read 的 8 倍)。
const RENDER_TITLE_CHARS: usize = 120;

/// 快照顺手做那次幂等归档的取锁预算。写事务本身是毫秒级的,等到 200ms 还没轮到
/// 说明对面正忙;归档是幂等的,跳过一轮下次刷新就补上,而面板卡住用户立刻能感觉到
/// ——`atomic_file::try_lock_exclusive` 的定位就是这条"做不成也无所谓"的路径。
const ARCHIVE_LOCK_BUDGET: std::time::Duration = std::time::Duration::from_millis(200);

/// 取测试记录的写事务锁(D-261,原语来自 `crate::atomic_file`)。
///
/// **一把锁罩两个文件**:键取活动文件 `tests.md`,归档 `tests-archive.md` 与它是
/// 一笔账(`allocate_test_id` / `ensure_id_unused` 同时扫两边),分开锁等于没锁
/// ——与 docstore「一个 kind 一把锁,键取自活动文档路径」同源。
///
/// **纪律:毫秒级持有,绝不跨 await。** `FileLock` 是 `!Send`,而本模块所有持锁的
/// 函数都是同步的(`TestRecordTool::execute` 只是同步调用它们),锁不会进入
/// async 状态机——谁要把持锁的代码挪到 `.await` 两侧,编译器会先拦下来。
///
/// **防死锁不变量:持锁期间永不获取第二把锁。** 本模块只有这一把锁,内层重入
/// (`record_test_run` → `append_test_run` / `test_runs_snapshot`)走同线程重入计数。
fn lock_test_runs(root: &Path) -> Result<crate::atomic_file::FileLock, String> {
    crate::atomic_file::lock_exclusive(&root.join(TEST_RUNS_REL)).map_err(|e| e.to_string())
}

#[derive(Deserialize, JsonSchema)]
struct TestRecordInput {
    /// 测试标题(如 "cargo test -p kanzei-llm")
    title: String,
    /// running | passed | failed | skipped
    status: String,
    /// 实际执行的命令(可选)
    #[serde(default)]
    command: Option<String>,
    /// 结果摘要(可选)
    #[serde(default)]
    summary: Option<String>,
    /// 测试实际耗时秒数(可选,R-210:门禁最慢环节可量化);写入「时长」字段
    #[serde(default)]
    duration_secs: Option<f64>,
    /// 关联条目 ID 列表(如 ["D-201", "R-153"]);写入「关联」字段,
    /// 建立测试→缺陷/需求的映射,供按条目反查测试记录
    #[serde(default)]
    refs: Option<Vec<String>>,
    /// 要收尾的既有记录 id(如 "T-1786254656");省略时按标题自动认领同名 running 记录
    #[serde(default)]
    id: Option<String>,
    /// 显式修复动作:传入要修复的重复编号(如 "T-1786297655"),把归档里该 id 下
    /// 除第一条外的重复记录逐条改成未占用编号并打印结果;此时其余字段被忽略。
    /// 仅用于清理 D-227 之前的历史同号存量,绝不静默自动触发。
    #[serde(default)]
    repair_reused_archived_id: Option<String>,
}

/// running 记录多久没收尾就算悬空。自举一轮的定向测试通常几分钟内出结果,
/// 半小时还挂着基本等于"跑完忘了登记"或"根本没跑"。
pub const STALE_RUNNING_SECS: u64 = 1800;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 记录 id 里带着创建时刻(T-<epoch>),据此算悬空时长。
///
/// 同一秒内连发多条时分配器会把编号推到墙钟之前(见 `allocate_test_id`),
/// 此时 age 取 0 而不是让 age_secs/stale 两个字段整个消失——几秒后自愈。
fn running_age_secs(id: &str) -> Option<u64> {
    let stamp: u64 = id.strip_prefix("T-")?.parse().ok()?;
    Some(now_secs().saturating_sub(stamp))
}

/// 分配下一个测试记录 id:`max(现在, 已占用最大编号 + 1)`。
///
/// D-227:秒级时间戳单独用会撞。四条 UI 冒烟记录共用了 `T-1786297655`,事后
/// 无法按 id 逐条引用/收尾。它**不是**并发缺陷——`ToolConcurrency::write_worktree`
/// 切 wave、R-171 的写租约都已经把写入串行化了(四条记录全部落盘存活即为证据),
/// 撞车照样发生:串行保证的是「同一时刻只有一个写者」,唯一性要的是「分配前看过
/// 已发出的编号」,两件事无交集。所以这里必须扫 active + archive 已占用的编号。
///
/// 保持纯 u64 而不是加 `-2` 后缀:`running_age_secs` 与 `last_passed_at` 都按
/// `T-<epoch>` 做 `parse::<u64>()`,后缀会让悬空检测静默失效,且 1400+ 条历史
/// 记录要跟着迁移。单调推进的代价只是编号在突发时领先墙钟几秒。
fn allocate_test_id(root: &Path) -> String {
    let used_max = [TEST_RUNS_REL, TEST_RUNS_ARCHIVE_REL]
        .into_iter()
        .flat_map(|rel| read_test_records(&root.join(rel)))
        .filter_map(|(_, record)| {
            record["id"]
                .as_str()?
                .strip_prefix("T-")?
                .parse::<u64>()
                .ok()
        })
        .max();
    let next = match used_max {
        Some(max) => now_secs().max(max + 1),
        None => now_secs(),
    };
    format!("T-{next}")
}

/// 新登记落盘前的编号占用兜底:该 id 已被 active/archive 任何一条记录占用就报错。
///
/// `allocate_test_id` 正常情况下已经保证编号未被占用,这里挡的是分配器被绕过的
/// 场景(手改文件、外部进程写入、将来新增的写路径)。发现冲突一律报错、不自动改号:
/// 参照 `docstore::repair_reused_archived_id` 的保守立场——静默改号会把编号复用
/// 伪装成一次正常写入,证据链就此不可信(D-004:拒绝的理由必须说出来,绝不静默)。
fn ensure_id_unused(root: &Path, id: &str, incoming_title: &str) -> Result<(), String> {
    for rel in [TEST_RUNS_REL, TEST_RUNS_ARCHIVE_REL] {
        for (_, record) in read_test_records(&root.join(rel)) {
            if record["id"].as_str() != Some(id) {
                continue;
            }
            let existing_title = record["title"].as_str().unwrap_or_default().trim();
            return Err(format!(
                "测试记录 {id} 已被占用(现有标题「{existing_title}」,见 {rel}),\
                 拒绝再登记一条同号记录(本次标题「{}」)。未写入任何内容。\
                 同一编号只能对应一次测试,否则按 id 收尾或反查证据时无法区分是哪一条。\
                 下一步:省略 id 重新调用会自动分配未占用编号;若本意是收尾已有记录,\
                 请带上那条记录自己的 id。",
                incoming_title.trim()
            ));
        }
    }
    Ok(())
}

pub struct TestRecordTool;

#[async_trait]
impl Tool for TestRecordTool {
    fn name(&self) -> &'static str {
        "test_record"
    }

    fn description(&self) -> String {
        format!(
            "Record a test run into `{TEST_RUNS_REL}` (the ONLY write channel for it — \
             write/edit are denied there). Call it after running tests: title (what was run), \
             status (running/passed/failed/skipped), optional command and summary. Terminal \
             statuses (passed/failed/skipped) are auto-archived into `{TEST_RUNS_ARCHIVE_REL}` \
             on snapshot; the sidebar lists active + archived runs."
        )
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(TestRecordInput)).unwrap()
    }

    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        // 写 tests.md,属工作树写操作,不能与其他写工具并行。
        ToolConcurrency::write_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: TestRecordInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let root = ctx.project_root.clone();
        if let Some(repair_id) = &input.repair_reused_archived_id {
            // D-259:显式一次性修复入口。只清历史同号存量,绝不自动触发;
            // 参照 docstore::repair_reused_archived_id 的保守立场,结果必须说出来。
            return match repair_reused_archived_id(&root, repair_id) {
                Ok(report) => ToolOutput::ok(report),
                Err(err) => ToolOutput::error(err),
            };
        }
        // D-332 验收④:收尾(status != running)时记录暂存源码指纹,提交门禁优先比
        // 指纹背书测试。拿不到 git 状态(非 git 目录/无暂存)时指纹为空,门禁退回 mtime。
        let fingerprint = if input.status != "running" {
            crate::git::staged_source_fingerprint(&ctx.cwd).unwrap_or_default()
        } else {
            String::new()
        };
        match record_test_run_with_duration(
            &root,
            input.id.as_deref(),
            &input.title,
            &input.status,
            input.command.as_deref(),
            input.summary.as_deref(),
            input.refs.as_deref(),
            input.duration_secs,
            Some(&fingerprint),
        ) {
            Ok(snapshot) => ToolOutput::ok(render_snapshot(&snapshot)),
            Err(err) => ToolOutput::error(err),
        }
    }
}

/// 解析 tests.md / tests-archive.md 的 `## T-xxx 标题 [status]` 块。
pub fn parse_test_blocks(text: &str) -> Vec<(String, serde_json::Value)> {
    text.split("\n## ")
        .filter_map(|raw| {
            let block = if raw.starts_with("## ") {
                raw.to_string()
            } else {
                format!("## {raw}")
            };
            let header = block.lines().next()?.trim_start_matches("## ").trim();
            let status_start = header.rfind('[')?;
            let status_end = header[status_start..].find(']')? + status_start;
            let status = header[status_start + 1..status_end].trim();
            let before = header[..status_start].trim();
            let (id, title) = before
                .split_once(' ')
                .map(|(id, title)| (id.to_string(), title.to_string()))
                .unwrap_or_else(|| (before.to_string(), String::new()));
            let fields = block
                .lines()
                .skip(1)
                .filter_map(|line| line.trim().strip_prefix("- "))
                .filter_map(|line| line.split_once(':'))
                .map(|(key, value)| json!({ "key": key.trim(), "value": value.trim() }))
                .collect::<Vec<_>>();
            // R-130:结构化「关联」字段——测试→缺陷/需求映射。旧记录没有此字段时
            // refs 为空,由 initialize_refs 从标题回填。
            let refs = fields
                .iter()
                .find(|f| f["key"] == "关联")
                .and_then(|f| f["value"].as_str())
                .map(|v| {
                    v.split_whitespace()
                        .filter(|part| is_entry_id(part))
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Some((
                block.trim_end().to_string(),
                json!({
                    "id": id, "title": title, "status": status,
                    "fields": fields, "refs": refs
                }),
            ))
        })
        .collect()
}

fn read_test_records(path: &Path) -> Vec<(String, serde_json::Value)> {
    std::fs::read_to_string(path)
        .map(|text| parse_test_blocks(&text))
        .unwrap_or_default()
}

/// 单个 `## T-xxx 标题 [status]` 块的记录 id(取标题行第一个 token)。
fn block_id(block: &str) -> String {
    block
        .lines()
        .next()
        .unwrap_or_default()
        .trim_start_matches("## ")
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

/// 活动记录按状态分成 (仍在跑的, 终态待归档的) 两组,元素是原文块。
fn partition_active(active_path: &Path) -> (Vec<String>, Vec<String>) {
    let mut live = Vec::new();
    let mut terminal = Vec::new();
    for (block, record) in read_test_records(active_path) {
        let status = record["status"].as_str().unwrap_or_default();
        if matches!(status, "passed" | "failed" | "skipped") {
            terminal.push(block);
        } else {
            live.push(block);
        }
    }
    (live, terminal)
}

/// 把 active 里的终态记录搬进归档文件(幂等)。
///
/// **D-261:整段在锁内。** 读、算、写三步之间被另一个进程插入一次写入,归档就会
/// 出现重复条目、活动文件里新写的记录也会被这次整文件回写抹掉——与
/// `docstore::archive_terminal`「事务锁必须罩住 load」同一形态、同一把原语。
///
/// **拿不到锁就跳过。** 这是只读快照顺手做的幂等维护(面板每次刷新、每次
/// `test_record` 返回前都会走一遍),做不成下次刷新就补上;让文档面板为一次归档
/// 卡住是拿更重的问题换更轻的(`atomic_file::try_lock_exclusive` 的定位即此)。
/// 只有"别人正在写"才跳过——编号复用、IO 故障这类真失败照常抛给调用方。
fn archive_terminal_records(active_path: &Path, archive_path: &Path) -> Result<(), String> {
    // 先不加锁探一眼:绝大多数快照根本没有可归档记录,不该为此去碰锁文件,更不该
    // 让面板刷新与 agent 的写入互相排队。这一眼只用于"要不要进事务",写入依据一律
    // 以锁内那次重读为准。
    if partition_active(active_path).1.is_empty() {
        return Ok(());
    }
    let Some(_lock) = crate::atomic_file::try_lock_exclusive(active_path, ARCHIVE_LOCK_BUDGET)
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    // 锁内重读:等锁期间别人可能已经把这批记录归档完了。
    let (live_blocks, terminal_blocks) = partition_active(active_path);
    if terminal_blocks.is_empty() {
        return Ok(());
    }
    let mut archived_text =
        std::fs::read_to_string(archive_path).unwrap_or_else(|_| "# Test Runs Archive\n".into());
    // 归档里同一编号只能有一条。内容完全相同视为重复归档,幂等跳过;
    // 内容不同说明编号被复用,直接报错——静默追加正是 D-227 那批同号记录
    // (T-1786297655 ×4)在归档里彼此无法区分的成因。
    let already = parse_test_blocks(&archived_text)
        .into_iter()
        .filter_map(|(block, record)| Some((record["id"].as_str()?.to_string(), block)))
        .collect::<std::collections::BTreeMap<_, _>>();
    for block in terminal_blocks {
        let id = block_id(&block);
        if let Some(existing) = already.get(&id) {
            if existing.trim() == block.trim() {
                continue;
            }
            return Err(format!(
                "归档 {} 里已有测试记录 {id} 且内容不同,拒绝追加第二条同号记录。\
                 未写入任何内容。\n现有归档:{}\n本次待归档:{}\n\
                 同号记录无法按 id 区分,需人工核对后处理;自动改号会掩盖编号复用。",
                archive_path.display(),
                existing.lines().next().unwrap_or_default(),
                block.lines().next().unwrap_or_default(),
            ));
        }
        archived_text.push_str("\n\n");
        archived_text.push_str(&block);
    }
    if let Some(parent) = archive_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // 写序不可调换:**先写归档、再删活动**。原子写只保证单个文件不被读成半截,
    // 保证不了两个文件之间的原子性——两步之间崩溃时,当前顺序留下的是"记录同时在
    // 两处"(重复归档会被上面那段幂等跳过),反过来留下的是"两处都没有"= 真丢
    // 数据(与 docstore::archive_terminal 同一取舍)。
    crate::atomic_file::write_atomic(archive_path, &archived_text).map_err(|e| e.to_string())?;
    let active_text = if live_blocks.is_empty() {
        "# Test Runs\n".to_string()
    } else {
        format!("# Test Runs\n\n{}\n", live_blocks.join("\n\n"))
    };
    crate::atomic_file::write_atomic(active_path, &active_text).map_err(|e| e.to_string())
}

/// 快照:读取 active + archived,并把 active 中的终态记录自动归档。
/// 返回 { active, archived, path, archive_path }。
pub fn test_runs_snapshot(root: &Path) -> Result<serde_json::Value, String> {
    let active_path = root.join(TEST_RUNS_REL);
    let archive_path = root.join(TEST_RUNS_ARCHIVE_REL);
    archive_terminal_records(&active_path, &archive_path)?;
    let live = read_test_records(&active_path)
        .into_iter()
        .map(|(_, mut record)| {
            // 悬空标记:running 挂太久要在快照里看得见,否则"没跑"和"跑完忘了记"
            // 在界面和 agent 眼里长得一模一样。
            if record["status"].as_str() == Some("running") {
                if let Some(age) = running_age_secs(record["id"].as_str().unwrap_or_default()) {
                    record["age_secs"] = json!(age);
                    record["stale"] = json!(age >= STALE_RUNNING_SECS);
                }
            }
            record
        })
        .collect::<Vec<_>>();
    let archived = read_test_records(&archive_path)
        .into_iter()
        .map(|(_, record)| record)
        .collect::<Vec<_>>();
    Ok(json!({
        "active": live,
        "archived": archived,
        "path": active_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
    }))
}

/// 登记一条测试记录。终态优先**就地收尾**已有的同名 running 记录,而不是再追加一条。
///
/// 此前工具只有 append 一条路径:agent 先记 running、跑完再记 passed,于是每跑一次测试
/// 就留下一条永远关不掉的 running——实测一天累积 41 条。悬空的 running 与"根本没跑过"
/// 在数据上完全一样,A-009/R-152 那条证据链就是被这个洞掏空的。
pub fn record_test_run(
    root: &Path,
    id: Option<&str>,
    title: &str,
    status: &str,
    command: Option<&str>,
    summary: Option<&str>,
    refs: Option<&[String]>,
) -> Result<serde_json::Value, String> {
    record_test_run_with_duration(root, id, title, status, command, summary, refs, None, None)
}

/// 同 [`record_test_run`],额外携带测试耗时秒数(R-210)写入「时长」字段;
/// `source_fingerprint`(D-332)为收尾时暂存源码的指纹,写入「源码指纹」字段,
/// 提交门禁优先用它判定测试背书,不再纯靠 mtime。
#[allow(clippy::too_many_arguments)] // 参数与 tests.md 记录字段一一对应,对象化会同时破坏全部调用方。
pub fn record_test_run_with_duration(
    root: &Path,
    id: Option<&str>,
    title: &str,
    status: &str,
    command: Option<&str>,
    summary: Option<&str>,
    refs: Option<&[String]>,
    duration_secs: Option<f64>,
    source_fingerprint: Option<&str>,
) -> Result<serde_json::Value, String> {
    if !VALID_STATUS.contains(&status) {
        return Err(format!("测试状态必须是 {} 之一", VALID_STATUS.join("、")));
    }
    let path = root.join(TEST_RUNS_REL);
    // D-261:「读 → 认领 → 定点替换 → 写」是一笔事务,锁必须罩住整段。只锁写那一下
    // 挡不住:两个进程各自读到同一份原文、各自算出替换结果、再各自整文件写回,后写的
    // 那个把前一个的收尾连同新记录一起覆盖掉——丢失发生在它们各自的读与写**之间**。
    // 内层 append_test_run / test_runs_snapshot 会再取一次同一把锁,走同线程重入。
    let _lock = lock_test_runs(root)?;
    let existing = read_test_records(&path);
    // 认领目标:显式 id 优先;否则终态自动认领同标题的 running 记录(最新的一条)。
    let target = existing.iter().rev().find(|(_, record)| {
        let record_id = record["id"].as_str().unwrap_or_default();
        let record_status = record["status"].as_str().unwrap_or_default();
        match id {
            Some(wanted) => record_id == wanted,
            None => {
                record_status == "running"
                    && record["title"].as_str().unwrap_or_default().trim() == title.trim()
                    && status != "running"
            }
        }
    });
    let Some((old_block, record)) = target else {
        if let Some(wanted) = id {
            return Err(format!(
                "找不到测试记录 {wanted};省略 id 可新登记一条,或先用 test_record 列表核对 id"
            ));
        }
        return append_test_run_with_duration(
            root,
            title,
            status,
            command,
            summary,
            refs,
            duration_secs,
            source_fingerprint,
        );
    };
    let record_id = record["id"].as_str().unwrap_or_default().to_string();
    let mut block = format!("## {record_id} {} [{status}]\n", title.trim());
    // 命令/摘要:本次没给就沿用原记录的,不要把已有信息覆盖成空。
    let inherited = |key: &str| -> Option<String> {
        record["fields"]
            .as_array()?
            .iter()
            .find(|f| f["key"].as_str() == Some(key))
            .and_then(|f| f["value"].as_str())
            .map(|v| v.to_string())
    };
    let final_command = command
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
        .or_else(|| inherited("命令"));
    if let Some(command) = final_command.as_deref() {
        block.push_str(&format!("- 命令: {command}\n"));
        // D-371:声称前端冒烟全过时,命令必须覆盖 verify.ps1 六条冒烟(差集非空判红)。
        check_frontend_smoke_claim(title, Some(command), status)?;
    }
    if let Some(secs) = duration_secs {
        block.push_str(&format!("- 时长: {secs:.1}s\n"));
    }
    if let Some(summary) = summary
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
        .or_else(|| inherited("摘要"))
    {
        block.push_str(&format!("- 摘要: {summary}\n"));
    }
    if let Some(refs) = refs
        .filter(|list| !list.is_empty())
        .map(|list| list.join(" "))
        .or_else(|| inherited("关联"))
    {
        block.push_str(&format!("- 关联: {refs}\n"));
    }
    if status != "running" {
        // 收尾时刻:记录 id 是**开始**时间,而提交门禁要问的是"测试跑完在改完代码之后吗"。
        // 必须单独落一个终点时间,否则先起 running 再改代码就能骗过门禁。
        block.push_str(&format!("- 收尾: {}\n", now_secs()));
        // D-332 验收④:测试背书的源码指纹——收尾时记录暂存源码 hash,提交门禁
        // 优先比指纹(而不是纯 mtime),fmt 后源码 diff 变 → 指纹变 → 要求重测。
        if let Some(fp) = source_fingerprint.filter(|v| !v.trim().is_empty()) {
            block.push_str(&format!("- 源码指纹: {}\n", fp.trim()));
        }
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    // 定点替换而不是 str::replace:后者会把**所有**字节相同的块一起换掉。
    // 文件里出现两条内容相同的记录时(编号复用的历史遗留),一次收尾会连坐
    // 干掉另一条;摘要里恰好嵌了另一条记录的原文时同理。
    let Some(at) = text.find(old_block.as_str()) else {
        return Err(format!(
            "测试记录 {record_id} 的原文块已不在 {} 中(可能刚被改写);未写入任何内容,请重新读取列表后重试",
            path.display()
        ));
    };
    let mut updated = String::with_capacity(text.len());
    updated.push_str(&text[..at]);
    updated.push_str(block.trim_end());
    updated.push_str(&text[at + old_block.len()..]);
    crate::atomic_file::write_atomic(&path, &updated).map_err(|e| e.to_string())?;
    let mut snapshot = test_runs_snapshot(root)?;
    snapshot["recorded_id"] = json!(record_id);
    Ok(snapshot)
}

/// 追加一条测试记录并返回最新快照(等价于 app 侧 test_run_record)。
pub fn append_test_run(
    root: &Path,
    title: &str,
    status: &str,
    command: Option<&str>,
    summary: Option<&str>,
    refs: Option<&[String]>,
) -> Result<serde_json::Value, String> {
    append_test_run_with_duration(root, title, status, command, summary, refs, None, None)
}

/// 同 [`append_test_run`],额外携带测试耗时秒数(R-210)写入「时长」字段;
/// `source_fingerprint`(D-332)收尾时写入「源码指纹」,提交门禁优先比指纹。
#[allow(clippy::too_many_arguments)] // 参数与 tests.md 记录字段一一对应,对象化会同时破坏全部调用方。
pub fn append_test_run_with_duration(
    root: &Path,
    title: &str,
    status: &str,
    command: Option<&str>,
    summary: Option<&str>,
    refs: Option<&[String]>,
    duration_secs: Option<f64>,
    source_fingerprint: Option<&str>,
) -> Result<serde_json::Value, String> {
    if !VALID_STATUS.contains(&status) {
        return Err(format!("测试状态必须是 {} 之一", VALID_STATUS.join("、")));
    }
    // D-371:声称前端冒烟全过时,命令必须覆盖 verify.ps1 六条冒烟(差集非空判红)。
    check_frontend_smoke_claim(title, command, status)?;
    let path = root.join(TEST_RUNS_REL);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // D-261:锁必须罩住「读 → 分配 id → 写」整段。分配器扫的是"此刻已占用的编号",
    // 扫完到落盘之间只要有别人写入,两个 OS 进程就会算出同一个 id 并互相覆盖——
    // D-227 修的是同秒时间戳(单进程内的单调推进),跨进程竞态要靠这把锁。
    let _lock = lock_test_runs(root)?;
    // D-227:编号由分配器给,不能直接取墙钟秒——同一秒内的多次登记(即使已被
    // wave/写租约串行化)会拿到同一个 id。分配后再做一次占用兜底。
    let id = allocate_test_id(root);
    ensure_id_unused(root, &id, title)?;
    let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| "# Test Runs\n".into());
    text.push_str(&format!("\n\n## {id} {} [{status}]\n", title.trim()));
    if let Some(command) = command.filter(|value| !value.trim().is_empty()) {
        text.push_str(&format!("- 命令: {}\n", command.trim()));
    }
    if let Some(secs) = duration_secs {
        text.push_str(&format!("- 时长: {secs:.1}s\n"));
    }
    if let Some(summary) = summary.filter(|value| !value.trim().is_empty()) {
        text.push_str(&format!("- 摘要: {}\n", summary.trim()));
    }
    if let Some(refs) = refs.filter(|list| !list.is_empty()) {
        text.push_str(&format!("- 关联: {}\n", refs.join(" ")));
    }
    if status != "running" {
        text.push_str(&format!("- 收尾: {}\n", now_secs()));
        // D-332:测试背书的源码指纹,提交门禁优先比指纹。
        if let Some(fp) = source_fingerprint.filter(|v| !v.trim().is_empty()) {
            text.push_str(&format!("- 源码指纹: {}\n", fp.trim()));
        }
    }
    crate::atomic_file::write_atomic(&path, &text).map_err(|e| e.to_string())?;
    let mut snapshot = test_runs_snapshot(root)?;
    snapshot["recorded_id"] = json!(id);
    Ok(snapshot)
}

/// R-212:一条 passed 测试记录背书的代码范围(覆盖面)。
#[derive(Debug, Clone, PartialEq)]
pub enum TestCoverage {
    /// 覆盖全部 workspace crate(`cargo test --workspace`,或仓库根裸 `cargo test`)。
    Workspace,
    /// 覆盖指定 crate 列表(`cargo test -p X -p Y`)。
    Crates(Vec<String>),
    /// 非 Rust 测试(前端冒烟/流程脚本),不覆盖任何 crate。
    NonRust,
}

impl TestCoverage {
    /// 该覆盖面是否背书 crate_name 的改动。
    pub fn covers(&self, crate_name: &str) -> bool {
        match self {
            TestCoverage::Workspace => true,
            TestCoverage::Crates(list) => list.iter().any(|c| c == crate_name),
            TestCoverage::NonRust => false,
        }
    }

    /// 人类可读描述(门禁拦截文案用)。
    pub fn describe(&self) -> String {
        match self {
            TestCoverage::Workspace => "workspace 全量".to_string(),
            TestCoverage::Crates(list) => format!("crate {}", list.join(", ")),
            TestCoverage::NonRust => "非 Rust(前端冒烟/流程脚本)".to_string(),
        }
    }

    fn union(self, other: TestCoverage) -> TestCoverage {
        match (self, other) {
            (TestCoverage::Workspace, _) | (_, TestCoverage::Workspace) => TestCoverage::Workspace,
            (TestCoverage::NonRust, coverage) | (coverage, TestCoverage::NonRust) => coverage,
            (TestCoverage::Crates(mut left), TestCoverage::Crates(right)) => {
                left.extend(right);
                left.sort();
                left.dedup();
                TestCoverage::Crates(left)
            }
        }
    }
}

/// 从测试命令提取覆盖面(R-212)。
///
/// 只认 cargo test 的 `-p/--package` 与 `--workspace`;其余命令(node 冒烟、
/// verify.ps1、cargo build 等)一律 NonRust——它们编译不了测试目标,背不了
/// 源码提交(R-158 教训同源:跑了 cargo check 就提交,reasoning effort 被顶掉)。
pub fn coverage_from_command(command: &str) -> TestCoverage {
    let normalized = command
        .replace("&&", "\n")
        .replace("||", "\n")
        .replace(';', "\n");
    normalized
        .lines()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(coverage_from_single_command)
        .fold(TestCoverage::NonRust, TestCoverage::union)
}

fn coverage_from_single_command(command: &str) -> TestCoverage {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let is_cargo_test = tokens
        .first()
        .map(|t| t.trim_end_matches(".exe") == "cargo")
        .unwrap_or(false)
        && tokens.get(1).map(|t| *t == "test").unwrap_or(false);
    if !is_cargo_test {
        return TestCoverage::NonRust;
    }
    if tokens.contains(&"--workspace") {
        return TestCoverage::Workspace;
    }
    let mut crates: Vec<String> = Vec::new();
    let mut i = 2;
    while i < tokens.len() {
        let tok = tokens[i];
        if tok == "-p" || tok == "--package" {
            if let Some(name) = tokens.get(i + 1) {
                crates.push((*name).to_string());
                i += 2;
                continue;
            }
        } else if let Some(name) = tok.strip_prefix("-p") {
            if !name.is_empty() && !name.starts_with('-') {
                crates.push(name.to_string());
            }
        }
        i += 1;
    }
    if crates.is_empty() {
        // 裸 `cargo test`(无 -p 无 --workspace):仓库根跑 = workspace 全量。
        TestCoverage::Workspace
    } else {
        crates.sort();
        crates.dedup();
        TestCoverage::Crates(crates)
    }
}

/// 记录的命令文本:优先「命令」字段,缺失(老记录)时用标题兜底——标题通常
/// 就是命令的复述("cargo test -p kanzei-llm (R-xxx …)")。
fn record_command_text(record: &serde_json::Value) -> String {
    record["fields"]
        .as_array()
        .and_then(|fields| {
            fields
                .iter()
                .find(|f| f["key"].as_str() == Some("命令"))
                .and_then(|f| f["value"].as_str())
                .map(|s| s.to_string())
        })
        .or_else(|| record["title"].as_str().map(|s| s.to_string()))
        .unwrap_or_default()
}

/// Return a test record's completion time in epoch seconds.
///
/// Current records carry an explicit second-based `收尾` field. Historical records
/// may not have it, so their millisecond-based `T-...` allocation id is the fallback;
/// normalize that legacy value before comparing it with current records.
fn record_finished_at(record: &serde_json::Value) -> Option<u64> {
    record["fields"]
        .as_array()
        .and_then(|fields| {
            fields
                .iter()
                .find(|f| f["key"].as_str() == Some("收尾"))
                .and_then(|f| f["value"].as_str())
                .and_then(|v| v.trim().parse::<u64>().ok())
        })
        .or_else(|| {
            record["id"]
                .as_str()
                .and_then(|id| id.strip_prefix("T-"))
                .and_then(|s| s.parse::<u64>().ok())
                .map(|id| {
                    if id >= 100_000_000_000 {
                        id / 1_000
                    } else {
                        id
                    }
                })
        })
}

type PassedRecord = (u64, TestCoverage, String, String);

fn passed_records(root: &Path) -> Vec<PassedRecord> {
    let mut passed = Vec::new();
    for rel in [TEST_RUNS_REL, TEST_RUNS_ARCHIVE_REL] {
        for (_, record) in read_test_records(&root.join(rel)) {
            if record["status"].as_str() != Some("passed") {
                continue;
            }
            let Some(at) = record_finished_at(&record) else {
                continue;
            };
            let command = record_command_text(&record);
            let fingerprint = record["fields"]
                .as_array()
                .and_then(|fields| {
                    fields
                        .iter()
                        .find(|f| f["key"].as_str() == Some("源码指纹"))
                        .and_then(|f| f["value"].as_str())
                        .map(str::to_string)
                })
                .unwrap_or_default();
            passed.push((at, coverage_from_command(&command), command, fingerprint));
        }
    }
    passed
}

fn select_passed_group(
    passed: Vec<PassedRecord>,
    fingerprint_filter: Option<&str>,
) -> Option<PassedRecord> {
    let newest = passed
        .iter()
        .filter(|record| fingerprint_filter.is_none_or(|expected| record.3 == expected))
        .max_by_key(|record| record.0)?
        .clone();
    if newest.3.is_empty() {
        return Some(newest);
    }

    // 同一份暂存源码可以由多条定向测试共同背书。只取最后一条会把前一条覆盖面
    // 丢掉，导致「tools + core 都通过」仍被误判为只测了最后那个 crate。
    let mut coverage = TestCoverage::NonRust;
    let mut commands: Vec<String> = Vec::new();
    let mut at = 0;
    for (finished, item_coverage, command, fingerprint) in passed {
        if fingerprint != newest.3 {
            continue;
        }
        at = at.max(finished);
        coverage = coverage.union(item_coverage);
        if !commands.contains(&command) {
            commands.push(command);
        }
    }
    Some((at, coverage, commands.join(" && "), newest.3))
}

/// 最近一条「通过」测试记录:(收尾时刻, 覆盖面, 命令文本, 源码指纹)。
/// active + archive 一起看。
///
/// 取收尾时刻而不是记录 id:id 是测试**开始**的时间,先起 running 再改代码就能骗过门禁。
/// R-212:覆盖面随记录一起回——门禁既要「改完重跑过」,又要「跑的是覆盖这份源码的测试」。
/// D-332:源码指纹随记录一起回——门禁优先比指纹而非纯 mtime,test_record 自己写
/// tests.md 不会改变源码指纹,不再触发「自己让自己失效」的重测。
pub fn last_passed(root: &Path) -> Option<(u64, TestCoverage, String, String)> {
    let passed = passed_records(root);
    let latest_fingerprint = passed
        .iter()
        .filter(|record| !record.3.is_empty())
        .max_by_key(|record| record.0)
        .map(|record| record.3.clone());
    match latest_fingerprint.as_deref() {
        Some(fingerprint) => select_passed_group(passed, Some(fingerprint)),
        None => select_passed_group(passed, None),
    }
}

/// Return the newest passed test group for a specific staged-source fingerprint.
///
/// A newer historical record without a fingerprint (for example a frontend smoke
/// record) must not hide a newer Rust record that does carry the current fingerprint.
pub fn last_passed_for_fingerprint(
    root: &Path,
    expected_fingerprint: &str,
) -> Option<(u64, TestCoverage, String, String)> {
    select_passed_group(passed_records(root), Some(expected_fingerprint))
}

/// 最近一次「通过」的测试是什么时候收尾的(epoch 秒)。R-212 门禁改走
/// [`last_passed`] 拿覆盖面,本函数保留为纯时间戳视图(兼容既有调用方)。
pub fn last_passed_at(root: &Path) -> Option<u64> {
    last_passed(root).map(|(at, _, _, _)| at)
}

/// 某条目(R-xxx/D-xxx)名下仍未收尾的 running 测试记录。
///
/// 判据是标题里是否出现该 id:测试记录本身没有结构化的 refs 字段,而实践中标题一律
/// 以条目号开头("R-153 批6 …")。宁可用这个朴素判据,也好过关闭时对未收尾的验证一无所知。
pub fn unclosed_running_for(root: &Path, entry_id: &str) -> Vec<(String, String)> {
    read_test_records(&root.join(TEST_RUNS_REL))
        .into_iter()
        .filter_map(|(_, record)| {
            if record["status"].as_str() != Some("running") {
                return None;
            }
            let title = record["title"].as_str().unwrap_or_default();
            title.contains(entry_id).then(|| {
                (
                    record["id"].as_str().unwrap_or_default().to_string(),
                    title.to_string(),
                )
            })
        })
        .collect()
}

/// R-130:按条目(R-xxx/D-xxx)反查关联的测试记录(active + archived)。
///
/// 判据**优先结构化 refs**(「关联」字段),标题命中作为兜底——旧记录没有 refs 时
/// 靠标题里出现的条目号照样能查到,保证初始化前后的查询口径一致。
pub fn records_for_entry(root: &Path, entry_id: &str) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for rel in [TEST_RUNS_REL, TEST_RUNS_ARCHIVE_REL] {
        for (_, mut record) in read_test_records(&root.join(rel)) {
            let refs_hit = record["refs"]
                .as_array()
                .map(|refs| refs.iter().any(|r| r.as_str() == Some(entry_id)))
                .unwrap_or(false);
            let title_hit = record["title"]
                .as_str()
                .map(|title| title.contains(entry_id))
                .unwrap_or(false);
            if refs_hit || title_hit {
                record["archived"] = json!(rel == TEST_RUNS_ARCHIVE_REL);
                out.push(record);
            }
        }
    }
    out
}

/// R-228:最近一条「通过」的**前端冒烟**测试记录(收尾时刻, 标题)。
///
/// 前端冒烟识别:命令或标题命中 `node scripts/ui-*.mjs` 的运行型冒烟
/// (ui-runtime / ui-i18n / ui-lint / ui-a11y / ui-markdown)。`node --check`
/// 只做语法检查不跑行为,不算冒烟(验收②:smoke 断言过时带病过关不可复现)。
///
/// 关闭门禁用:带「前端」标签的条目关闭前,必须已有前端冒烟 passed 记录
/// (R-228 验收①:未跑 ui smoke 会被拒)。active + archive 一起看,取最新收尾。
pub fn frontend_smoke_passed(root: &Path) -> Option<(u64, String)> {
    let mut newest: Option<(u64, String)> = None;
    for rel in [TEST_RUNS_REL, TEST_RUNS_ARCHIVE_REL] {
        for (_, record) in read_test_records(&root.join(rel)) {
            if record["status"].as_str() != Some("passed") {
                continue;
            }
            let command = record_command_text(&record);
            if !is_frontend_smoke(&command) {
                continue;
            }
            let finished = record["fields"]
                .as_array()
                .and_then(|fields| {
                    fields
                        .iter()
                        .find(|f| f["key"].as_str() == Some("收尾"))
                        .and_then(|f| f["value"].as_str())
                        .and_then(|v| v.trim().parse::<u64>().ok())
                })
                .or_else(|| {
                    record["id"]
                        .as_str()
                        .and_then(|id| id.strip_prefix("T-"))
                        .and_then(|s| s.parse::<u64>().ok())
                });
            if let Some(at) = finished {
                let title = record["title"].as_str().unwrap_or_default().to_string();
                newest = Some(match newest {
                    Some(cur) if cur.0 >= at => cur,
                    _ => (at, title),
                });
            }
        }
    }
    newest
}

/// 是否前端运行型冒烟(`node scripts/ui-*.mjs`,不含 `--check` 语法检查)。
fn is_frontend_smoke(command: &str) -> bool {
    if !command.contains("node") || !command.contains("scripts/ui-") {
        return false;
    }
    !command.contains("--check")
}

/// verify.ps1 十步门禁中的六条前端冒烟(与 scripts/verify.ps1 逐条对应)。
/// D-371:声称「前端冒烟全过/冒烟集/冒烟四连」时,必须覆盖这六条——差集非空即判红,
/// 与 D-264 同一族(「跑了子集、报了全称」),机械判据补上「声称不可核」这一侧。
const FRONTEND_SMOKE_LIST: &[&str] = &[
    "ui-runtime-smoke.mjs",
    "ui-lint-smoke.mjs",
    "parallel-lines-regression.mjs",
    "ui-a11y-smoke.mjs",
    "ui-i18n-smoke.mjs",
    "ui-markdown-smoke.mjs",
];

/// D-371 机械判据:title 声称「冒烟」且 status=passed 时,command 必须覆盖
/// verify.ps1 的六条前端冒烟,差集非空即拒绝写入——「全绿」的定义是 verify.ps1
/// 十步,不是任意子集。未提供命令时同样拒绝(无法核验 = 判红)。
fn check_frontend_smoke_claim(
    title: &str,
    command: Option<&str>,
    status: &str,
) -> Result<(), String> {
    if status != "passed" || !title.contains("冒烟") {
        return Ok(());
    }
    let Some(command) = command.map(str::trim).filter(|c| !c.is_empty()) else {
        return Err(format!(
            "声称「{title}」是前端冒烟全过,但未提供命令,无法核验覆盖。\
             verify.ps1 十步含六条冒烟({}),差集非空即判红(D-371)",
            FRONTEND_SMOKE_LIST.join(" ")
        ));
    };
    let missing: Vec<&str> = FRONTEND_SMOKE_LIST
        .iter()
        .copied()
        .filter(|script| !command.contains(script))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "声称「{title}」是前端冒烟,但命令只覆盖 {}/{} 条,缺:{}。\
         verify.ps1 十步含六条冒烟,差集非空即判红(D-371)。请补跑:{}",
        FRONTEND_SMOKE_LIST.len() - missing.len(),
        FRONTEND_SMOKE_LIST.len(),
        missing.join("、"),
        FRONTEND_SMOKE_LIST.join(" ")
    ))
}

/// R-130:批量初始化/回填测试→条目映射。
///
/// 旧测试记录没有「关联」字段,查询只能靠标题命中。这里扫描 tests.md 全部记录,
/// 从标题里提取 `R-xxx` / `D-xxx` 条目号,补写「关联」字段,一次落盘。
/// 返回回填了多少条,方便调用方反馈(0 表示已全部结构化,无旧记录)。
pub fn initialize_refs(root: &Path) -> Result<serde_json::Value, String> {
    let path = root.join(TEST_RUNS_REL);
    // D-261:整读整写 tests.md,读到写之间必须没有别人写入——否则这次回填会把期间
    // 新登记的记录连同它的编号一起覆盖掉。判据("哪些块缺关联字段")与落盘是一笔事务。
    let _lock = lock_test_runs(root)?;
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut backfilled = 0usize;
    let updated = {
        let mut parts = text.split("\n## ").collect::<Vec<_>>();
        // 第一部分是文件头("# Test Runs"),不拆。
        let head = parts.remove(0);
        let mut blocks = Vec::with_capacity(parts.len() + 1);
        blocks.push(head.to_string());
        for raw in parts {
            let block = format!("## {raw}");
            // 已有「关联」字段就不动(结构化在前,标题回填只补缺失)。
            let has_refs = block.lines().any(|line| {
                line.trim()
                    .strip_prefix("- ")
                    .and_then(|l| l.split_once(':'))
                    .map(|(key, _)| key.trim() == "关联")
                    .unwrap_or(false)
            });
            if has_refs {
                blocks.push(block);
                continue;
            }
            // 从标题行提取条目号:## T-xxx 标题 [status]
            let header = block.lines().next().unwrap_or_default();
            let title = header.trim_start_matches("## ").trim();
            let title_only = title
                .split('[')
                .next()
                .unwrap_or(title)
                .trim()
                .trim_start_matches(|c: char| !c.is_ascii_digit() && c != ' ')
                .trim();
            let ids = extract_entry_ids(title_only);
            if ids.is_empty() {
                blocks.push(block);
                continue;
            }
            // 插到「收尾」行之前,保持字段块连贯。
            let mut lines = block.lines().collect::<Vec<_>>();
            let insert_at = lines
                .iter()
                .position(|line| line.trim_start().starts_with("- 收尾:"))
                .unwrap_or(lines.len());
            let ref_line = format!("- 关联: {}", ids.join(" "));
            lines.insert(insert_at, &ref_line);
            blocks.push(lines.join("\n"));
            backfilled += 1;
        }
        blocks.join("\n## ")
    };
    if updated == text {
        // 幂等:没有任何需要回填的记录时不动文件,避免每次打开测试页都触发一次写盘。
        return Ok(json!({ "backfilled": 0 }));
    }
    crate::atomic_file::write_atomic(&path, &updated).map_err(|e| e.to_string())?;
    Ok(json!({ "backfilled": backfilled }))
}

/// D-259:显式一次性修复——清理 tests-archive.md 里 D-227 之前的历史同号记录。
///
/// 参照 `docstore::repair_reused_archived_id` 的保守立场:绝不静默批量改号,必须
/// 显式指定要修复的编号,逐条改成未占用编号并保留原标题/状态/字段,结果打印出来。
/// 修复后的编号与 active + archive 双侧现存编号都不冲突(`ensure_id_unused` 的判据)。
/// 只有该编号确实存在 ≥2 条时才动手;单条/不存在直接报错,不做任何写。
pub fn repair_reused_archived_id(root: &Path, old_id: &str) -> Result<String, String> {
    if !old_id.starts_with("T-") {
        return Err(format!(
            "要修复的必须是测试记录编号(形如 T-xxx),收到「{old_id}」"
        ));
    }
    let archive_path = root.join(TEST_RUNS_ARCHIVE_REL);
    // D-261:读 → 分配新号 → 写回,整段在锁内;否则两个进程同时修可能撞号或互相覆盖。
    let _lock = lock_test_runs(root)?;
    let text = std::fs::read_to_string(&archive_path).map_err(|e| e.to_string())?;
    let records = read_test_records(&archive_path);
    let dup_positions: Vec<usize> = records
        .iter()
        .enumerate()
        .filter(|(_, (_, record))| record["id"].as_str() == Some(old_id))
        .map(|(i, _)| i)
        .collect();
    if dup_positions.len() < 2 {
        return Err(format!(
            "{old_id} 在 {} 中只有 {} 条,没有需要修复的重复(需 ≥2 条同号记录)",
            TEST_RUNS_ARCHIVE_REL,
            dup_positions.len()
        ));
    }
    // 收集 active + archive 双侧全部已占用编号,新编号必须避开(ensure_id_unused 判据)。
    let mut used: std::collections::BTreeSet<u64> = [TEST_RUNS_REL, TEST_RUNS_ARCHIVE_REL]
        .into_iter()
        .flat_map(|rel| read_test_records(&root.join(rel)))
        .filter_map(|(_, record)| {
            record["id"]
                .as_str()?
                .strip_prefix("T-")?
                .parse::<u64>()
                .ok()
        })
        .collect();
    // 保留第一条原编号(它是对应那次测试最早的记录),其余逐条改成未占用编号。
    // 按行扫描 + 出现次序计数,而不是按块内容匹配:同号块内容若完全相同,
    // replacen 会改错对象(把该保留的第一条也改掉),计数则精确到第几条。
    let mut changes = Vec::new();
    let mut change_i = 1usize; // dup_positions[0] 保留原编号,从第 2 条起改号。
    let mut seen = 0usize;
    let mut lines = text
        .split('\n')
        .map(|s| s.to_string())
        .collect::<Vec<String>>();
    for line in &mut lines {
        if !line.trim_start().starts_with(&format!("## {old_id}")) {
            continue;
        }
        if seen == 0 {
            seen += 1;
            continue;
        }
        let next = used
            .iter()
            .next_back()
            .copied()
            .map(|max| now_secs().max(max + 1))
            .unwrap_or_else(now_secs);
        used.insert(next);
        let new_id = format!("T-{next}");
        let title = dup_positions
            .get(change_i)
            .and_then(|&pos| records[pos].1.get("title"))
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .trim();
        // 只替换块首行的编号 token(`## T-xxx ... [status]`),标题/状态一字不动。
        let new_line = line.replacen(old_id, &new_id, 1);
        if new_line == *line {
            return Err(format!(
                "{old_id} 的块首行里找不到编号 token,未写入任何内容:\n{line}"
            ));
        }
        changes.push(format!("{old_id}「{title}」→ {new_id}"));
        *line = new_line;
        seen += 1;
        change_i += 1;
    }
    crate::atomic_file::write_atomic(&archive_path, &lines.join("\n"))
        .map_err(|e| e.to_string())?;
    let mut report = format!(
        "已修复 {old_id}(保留第一条「{}」原编号,其余 {n} 条改号):\n",
        records[dup_positions[0]]
            .1
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .trim(),
        n = changes.len()
    );
    for line in changes {
        report.push_str(&format!("  {line}\n"));
    }
    report.push_str("原记录的标题/状态/命令/摘要/关联字段一字未动。");
    Ok(report)
}

/// 从字符串里提取 `R-xxx` / `D-xxx` 条目号(去重保序)。
fn extract_entry_ids(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for tok in text.split_whitespace() {
        let tok = tok.trim_matches(|c: char| {
            !(c.is_ascii_alphanumeric() || c == '-' || c == '#' || c == ':')
        });
        if is_entry_id(tok) && seen.insert(tok.to_string()) {
            out.push(tok.to_string());
        }
    }
    out
}

/// 是否形如 `R-153` / `D-201`(条目号,字母 + 至少 2 位数字)。
fn is_entry_id(part: &str) -> bool {
    (part.starts_with("R-") || part.starts_with("D-"))
        && part.len() > 3
        && part[2..].chars().all(|c| c.is_ascii_digit())
}

/// 快照渲染成工具可读文本。
fn render_snapshot(snapshot: &serde_json::Value) -> String {
    let active = snapshot["active"].as_array().map(Vec::len).unwrap_or(0);
    let archived = snapshot["archived"].as_array().map(Vec::len).unwrap_or(0);
    // 本次分配到的编号必须回显:拿不到 id,「跑完带上 id 收尾」这条纪律在源头
    // 就无法执行,只能靠标题猜——D-227 里四条同号记录事后无法逐条引用,正是
    // 这个缺口(编号既不唯一、又从不告诉调用方)一起造成的。
    let recorded = snapshot["recorded_id"].as_str().unwrap_or_default();
    let mut lines = vec![if recorded.is_empty() {
        format!(
            "recorded. active: {active}, archived: {archived} (path: {})",
            snapshot["path"].as_str().unwrap_or_default()
        )
    } else {
        format!(
            "recorded {recorded}. active: {active}, archived: {archived} (path: {}, archive: {})",
            snapshot["path"].as_str().unwrap_or_default(),
            snapshot["archive_path"].as_str().unwrap_or_default()
        )
    }];
    let still_running = !recorded.is_empty()
        && snapshot["active"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|r| r["id"].as_str() == Some(recorded) && r["status"].as_str() == Some("running"));
    if still_running {
        lines.push(format!(
            "↳ 跑完请用 test_record 带 id={recorded} 记终态(passed/failed/skipped)。"
        ));
    }
    let mut stale = 0;
    for record in snapshot["active"].as_array().into_iter().flatten() {
        let is_stale = record["stale"].as_bool().unwrap_or(false);
        if is_stale {
            stale += 1;
        }
        lines.push(format!(
            "● {} {} [{}]{}",
            record["id"].as_str().unwrap_or_default(),
            record["title"].as_str().unwrap_or_default(),
            record["status"].as_str().unwrap_or_default(),
            if is_stale {
                format!(
                    " ⚠ 悬空 {} 分钟未收尾",
                    record["age_secs"].as_u64().unwrap_or(0) / 60
                )
            } else {
                String::new()
            },
        ));
        let refs = record["refs"].as_array().map(|r| {
            r.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        });
        if let Some(refs) = refs.filter(|r| !r.is_empty()) {
            lines.push(format!("   关联: {refs}"));
        }
    }
    if stale > 0 {
        lines.push(format!(
            "⚠ {stale} 条 running 记录已悬空:跑完请用 test_record 带上该条 id 收尾(status=passed/failed/skipped),\
             同标题的终态记录会自动认领。悬空记录会挡住相关条目的 close。"
        ));
    }
    // 归档只回显**本次记录的那一条**,不再逐条打印整份索引(理由见
    // RENDER_TITLE_CHARS 上方的注释)。总数已在表头 `archived: {n}` 给出;要查
    // 历史去表头新增的 archive 路径 grep——那是砍掉清单后唯一需要补偿的能力
    // 缺口,所以表头必须带上它。
    if !recorded.is_empty() {
        if let Some(record) = snapshot["archived"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|r| r["id"].as_str() == Some(recorded))
        {
            lines.push(format!(
                "○ {} {} [{}]",
                record["id"].as_str().unwrap_or_default(),
                truncate_title(record["title"].as_str().unwrap_or_default()),
                record["status"].as_str().unwrap_or_default(),
            ));
        }
    }
    lines.join("\n")
}

/// 标题按字符截断。走 chars() 而非字节切片——标题大量中文,按字节切会 panic 在
/// 非字符边界(与 grep.rs 的 MAX_LINE_CHARS 同写法)。
fn truncate_title(title: &str) -> String {
    if title.chars().count() <= RENDER_TITLE_CHARS {
        return title.to_string();
    }
    let mut out: String = title.chars().take(RENDER_TITLE_CHARS).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_project(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-test-record-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // R-141 后 ToolCtx::new 不再发现取根,但 DocStore/托管路径仍以 .kanzei
        // 为准;保留标记,让 fixture 与可能位于某个 checkout 之下的 CI 临时目录隔离。
        std::fs::create_dir(dir.join(".kanzei")).unwrap();
        dir
    }

    #[test]
    fn 终态就地收尾同名running而不是再追加一条() {
        let root = temp_project("claim");
        record_test_run(
            &root,
            None,
            "R-999 定向回归",
            "running",
            Some("cargo test"),
            None,
            None,
        )
        .unwrap();
        let snapshot = record_test_run(
            &root,
            None,
            "R-999 定向回归",
            "passed",
            None,
            Some("全绿"),
            None,
        )
        .unwrap();
        assert_eq!(
            snapshot["active"].as_array().unwrap().len(),
            0,
            "收尾后不该再有 running 挂着:{snapshot:#?}"
        );
        let archived = snapshot["archived"].as_array().unwrap();
        assert_eq!(
            archived.len(),
            1,
            "只应归档一条,而不是 running/passed 各一条"
        );
        assert_eq!(archived[0]["status"], "passed");
        let fields = archived[0]["fields"].as_array().unwrap();
        assert!(
            fields
                .iter()
                .any(|f| f["key"] == "命令" && f["value"] == "cargo test"),
            "收尾时没给命令就该沿用原记录的,不能覆盖成空:{fields:#?}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 悬空的running记录会被标记并能按条目号查出来() {
        let root = temp_project("stale");
        // 直接写一条 20 天前的 running 记录:id 里的时间戳就是判据。
        let old_id = now_secs() - 20 * 86_400;
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            format!("# Test Runs\n\n## T-{old_id} R-153 批6 回归 [running]\n- 命令: cargo test\n"),
        )
        .unwrap();
        let snapshot = test_runs_snapshot(&root).unwrap();
        let active = &snapshot["active"].as_array().unwrap()[0];
        assert_eq!(
            active["stale"], true,
            "20 天前的 running 必须判悬空:{active:#?}"
        );
        assert_eq!(unclosed_running_for(&root, "R-153").len(), 1);
        assert_eq!(
            unclosed_running_for(&root, "R-158").len(),
            0,
            "不该误伤别的条目"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn last_passed_at_取收尾时刻而不是开始时刻() {
        let root = temp_project("passedat");
        // 先起 running(id 时刻在很久以前),再收尾:门禁要认收尾那一刻。
        let started = now_secs() - 3600;
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            format!("# Test Runs\n\n## T-{started} R-1 回归 [running]\n- 命令: cargo test\n"),
        )
        .unwrap();
        assert_eq!(last_passed_at(&root), None, "只有 running 时不该有背书");
        record_test_run(&root, None, "R-1 回归", "passed", None, Some("全绿"), None).unwrap();
        let at = last_passed_at(&root).expect("收尾后必须有时间戳");
        assert!(
            at >= started + 3000,
            "取的必须是收尾时刻({at})而不是开始时刻({started})——否则先起 running 再改代码就能骗过提交门禁"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// R-212:coverage_from_command 从命令提取覆盖面——-p/--package/--workspace/
    /// 非 cargo 命令四类,裸 cargo test 视为 workspace(仓库根默认全量)。
    #[test]
    fn coverage_from_command_parses_cargo_flags() {
        use TestCoverage as C;
        assert_eq!(
            coverage_from_command("node scripts/ui-runtime-smoke.mjs"),
            C::NonRust
        );
        assert_eq!(coverage_from_command("verify.ps1"), C::NonRust);
        assert_eq!(coverage_from_command("cargo build --release"), C::NonRust);
        assert_eq!(
            coverage_from_command("cargo test -p kanzei-tools --lib memory::"),
            C::Crates(vec!["kanzei-tools".to_string()])
        );
        assert_eq!(
            coverage_from_command("cargo test -p kanzei-tools -p kanzei-core"),
            C::Crates(vec!["kanzei-core".to_string(), "kanzei-tools".to_string()])
        );
        assert_eq!(
            coverage_from_command("cargo test --workspace --all-targets"),
            C::Workspace
        );
        assert_eq!(coverage_from_command("cargo test"), C::Workspace);
        assert_eq!(
            coverage_from_command("cargo test -p kanzei-tools; cargo test -p kanzei-core"),
            C::Crates(vec!["kanzei-core".to_string(), "kanzei-tools".to_string()])
        );
        assert_eq!(
            coverage_from_command("node scripts/ui-runtime-smoke.mjs && cargo test -p kanzei-app"),
            C::Crates(vec!["kanzei-app".to_string()])
        );
        // 覆盖面语义:covers 判定。
        let crates = C::Crates(vec!["kanzei-tools".to_string()]);
        assert!(crates.covers("kanzei-tools"));
        assert!(!crates.covers("kanzei-core"));
        assert!(C::Workspace.covers("kanzei-app"));
        assert!(!C::NonRust.covers("kanzei-tools"));
    }

    /// R-212:last_passed 随时间戳一起返回覆盖面——前端冒烟记录是 NonRust,
    /// 定向 cargo test 是 Crates,老记录(无命令字段)从标题兜底提取。
    #[test]
    fn last_passed_returns_coverage_and_command() {
        let root = temp_project("coverage");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let now = now_secs();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            format!(
                "# Test Runs\n\n## T-{now} 前端冒烟 [passed]\n- 命令: node scripts/ui-runtime-smoke.mjs\n- 收尾: {now}\n"
            ),
        )
        .unwrap();
        let (at, coverage, command, _fp) = last_passed(&root).expect("必须有记录");
        assert_eq!(at, now);
        assert_eq!(coverage, TestCoverage::NonRust);
        assert!(command.contains("ui-runtime-smoke"));
        // 老记录无命令字段 → 从标题兜底("cargo test -p kanzei-llm (R-1 …)")。
        std::fs::write(
            root.join(TEST_RUNS_REL),
            format!("# Test Runs\n\n## T-{now} cargo test -p kanzei-llm (R-1 回归) [passed]\n- 收尾: {now}\n"),
        )
        .unwrap();
        let (_, coverage, command, _fp) = last_passed(&root).unwrap();
        assert_eq!(
            coverage,
            TestCoverage::Crates(vec!["kanzei-llm".to_string()])
        );
        assert!(command.contains("kanzei-llm"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn last_passed_normalizes_legacy_millisecond_id_before_comparing() {
        let root = temp_project("legacy-id-time-unit");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_ARCHIVE_REL),
            "# Test Runs Archive\n\n## T-1786922726036 legacy frontend [passed]\n- 命令: node scripts/ui-runtime-smoke.mjs\n\n## T-1786922726055 current rust [passed]\n- 命令: cargo test -p kanzei-tools\n- 收尾: 1786927698\n- 源码指纹: fp-current\n",
        )
        .unwrap();

        let (at, coverage, command, fingerprint) = last_passed(&root).unwrap();
        assert_eq!(at, 1_786_927_698);
        assert_eq!(coverage, TestCoverage::Crates(vec!["kanzei-tools".into()]));
        assert!(command.contains("kanzei-tools"));
        assert_eq!(fingerprint, "fp-current");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn last_passed_prefers_fingerprinted_group_over_newer_legacy_record() {
        let root = temp_project("fingerprinted-group-precedence");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_ARCHIVE_REL),
            "# Test Runs Archive\n\n## T-1786929999999 legacy frontend [passed]\n- 命令: node scripts/ui-runtime-smoke.mjs\n\n## T-1786922726055 current rust [passed]\n- 命令: cargo test -p kanzei-tools\n- 收尾: 1786927000\n- 源码指纹: fp-current\n",
        )
        .unwrap();

        let (at, coverage, command, fingerprint) = last_passed(&root).unwrap();
        assert_eq!(at, 1_786_927_000);
        assert_eq!(coverage, TestCoverage::Crates(vec!["kanzei-tools".into()]));
        assert!(command.contains("kanzei-tools"));
        assert_eq!(fingerprint, "fp-current");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn last_passed_unions_records_for_same_source_fingerprint() {
        let root = temp_project("coverage-union");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let now = now_secs();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            format!(
                "# Test Runs\n\n## T-{now} tools [passed]\n- 命令: cargo test -p kanzei-tools\n- 收尾: {now}\n- 源码指纹: fp-one\n\n## T-{} core [passed]\n- 命令: cargo test -p kanzei-core\n- 收尾: {}\n- 源码指纹: fp-one\n",
                now + 1,
                now + 1,
            ),
        )
        .unwrap();
        let (_, coverage, command, fingerprint) = last_passed(&root).unwrap();
        assert_eq!(
            coverage,
            TestCoverage::Crates(vec!["kanzei-core".into(), "kanzei-tools".into()])
        );
        assert!(command.contains("kanzei-tools") && command.contains("kanzei-core"));
        assert_eq!(fingerprint, "fp-one");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn parse_blocks_extracts_id_title_status_and_fields() {
        let text = "# Test Runs\n\n## T-1 cargo test [passed]\n- 命令: cargo test\n- 摘要: 全绿\n";
        let blocks = parse_test_blocks(text);
        assert_eq!(blocks.len(), 1);
        let (block, record) = &blocks[0];
        assert_eq!(record["id"], json!("T-1"));
        assert_eq!(record["title"], json!("cargo test"));
        assert_eq!(record["status"], json!("passed"));
        let fields = record["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0]["key"], json!("命令"));
        assert_eq!(fields[0]["value"], json!("cargo test"));
        assert!(block.contains("## T-1 cargo test [passed]"));
    }

    #[test]
    fn parse_blocks_extracts_refs_from关联字段() {
        let text = "# Test Runs\n\n## T-3 R-153 批6 回归 [passed]\n- 命令: cargo test\n- 关联: D-201 R-153\n";
        let blocks = parse_test_blocks(text);
        assert_eq!(blocks.len(), 1);
        let refs = blocks[0].1["refs"].as_array().unwrap();
        let ids = refs.iter().filter_map(|r| r.as_str()).collect::<Vec<_>>();
        assert_eq!(ids, vec!["D-201", "R-153"], "关联字段应解析为结构化 refs");
        // 非条目 token(命令、路径)不该混进 refs。
        let text2 = "# Test Runs\n\n## T-4 x [passed]\n- 关联: cargo D-x R-1 R-153\n";
        let blocks2 = parse_test_blocks(text2);
        let ids2 = blocks2[0].1["refs"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids2, vec!["R-153"], "只认 R-/D- 开头的条目号:{ids2:?}");
    }

    #[test]
    fn records_for_entry_prefers_refs_and_falls_back_to_title() {
        let root = temp_project("refsquery");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            "# Test Runs\n\n## T-1 D-201 回归 [passed]\n- 命令: cargo test\n",
        )
        .unwrap();
        // 标题命中(旧记录无 refs 字段)。
        let by_title = records_for_entry(&root, "D-201");
        assert_eq!(by_title.len(), 1, "标题兜底查询应命中 D-201:{by_title:#?}");
        assert_eq!(by_title[0]["id"], json!("T-1"));
        // 无关条目不误伤。
        assert_eq!(records_for_entry(&root, "R-999").len(), 0);
        // 结构化 refs 查询:显式关联命中。
        std::fs::write(
            root.join(TEST_RUNS_REL),
            "# Test Runs\n\n## T-2 冒烟 [passed]\n- 关联: D-999\n- 命令: x\n",
        )
        .unwrap();
        let by_refs = records_for_entry(&root, "D-999");
        assert_eq!(by_refs.len(), 1, "refs 字段应命中 D-999:{by_refs:#?}");
        assert_eq!(by_refs[0]["id"], json!("T-2"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn initialize_refs_backfills_entry_ids_from_title() {
        let root = temp_project("initrefs");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            "# Test Runs\n\n## T-1 R-153 批6 回归 [passed]\n- 命令: cargo test\n- 收尾: 123\n\n## T-2 冒烟测试 [passed]\n- 命令: x\n",
        )
        .unwrap();
        let result = initialize_refs(&root).unwrap();
        assert_eq!(
            result["backfilled"],
            json!(1),
            "只有标题含条目号的记录该回填"
        );
        let text = std::fs::read_to_string(root.join(TEST_RUNS_REL)).unwrap();
        assert!(
            text.contains("- 关联: R-153"),
            "标题里的 R-153 未回填进关联字段:\n{text}"
        );
        assert!(
            text.contains("## T-2 冒烟测试"),
            "无关记录不得被改写:\n{text}"
        );
        assert!(
            text.contains("收尾: 123"),
            "关联字段应插在收尾行之前,不破坏原字段:\n{text}"
        );
        // 幂等:再跑一次不应重复回填。
        let second = initialize_refs(&root).unwrap();
        assert_eq!(second["backfilled"], json!(0), "已结构化的记录不应重复回填");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn append_with_refs_writes_关联_field() {
        let root = temp_project("appendrefs");
        let snapshot = append_test_run(
            &root,
            "D-201 回归",
            "passed",
            Some("cargo test"),
            None,
            Some(&["D-201".to_string(), "R-153".to_string()]),
        )
        .unwrap();
        let archived = snapshot["archived"].as_array().unwrap();
        assert_eq!(archived[0]["refs"].as_array().unwrap().len(), 2);
        let ids = archived[0]["refs"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["D-201", "R-153"]);
        // 终态记录已被快照归档到 archive:关联字段应落在归档文件里。
        let archive_text = std::fs::read_to_string(root.join(TEST_RUNS_ARCHIVE_REL)).unwrap();
        assert!(
            archive_text.contains("- 关联: D-201 R-153"),
            "关联字段未写入归档:\n{archive_text}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// R-210:duration_secs 写入「时长」字段且可往返解析(门禁最慢环节可量化)。
    #[test]
    fn duration_secs_writes_时长_field_and_roundtrips() {
        let root = temp_project("duration");
        let snapshot = append_test_run_with_duration(
            &root,
            "R-210 定向",
            "passed",
            Some("cargo test -p kanzei-tools"),
            None,
            None,
            Some(12.345),
            None,
        )
        .unwrap();
        let recorded_id = snapshot["recorded_id"].as_str().unwrap().to_string();
        // 终态自动归档:时长字段应落在归档文件里,格式保留一位小数。
        let archive_text = std::fs::read_to_string(root.join(TEST_RUNS_ARCHIVE_REL)).unwrap();
        assert!(
            archive_text.contains("- 时长: 12.3s"),
            "时长字段未写入归档:\n{archive_text}"
        );
        // 往返:解析出的字段 key=时长 value=12.3s。
        let (_, record) = read_test_records(&root.join(TEST_RUNS_ARCHIVE_REL))
            .into_iter()
            .find(|(_, r)| r["id"].as_str() == Some(recorded_id.as_str()))
            .unwrap();
        let dur = record["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["key"].as_str() == Some("时长"))
            .and_then(|f| f["value"].as_str())
            .unwrap();
        assert_eq!(dur, "12.3s");
        // 未提供 duration 时不得凭空出现时长行。
        let root2 = temp_project("duration-none");
        append_test_run(
            &root2,
            "plain record no extra field",
            "passed",
            None,
            None,
            None,
        )
        .unwrap();
        let text2 = std::fs::read_to_string(root2.join(TEST_RUNS_ARCHIVE_REL)).unwrap();
        assert!(
            !text2.contains("时长"),
            "未提供 duration 不应写时长行:\n{text2}"
        );
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&root2).ok();
    }

    #[test]
    fn parse_blocks_handles_running_without_fields() {
        let text = "# Test Runs\n\n## T-2 long run [running]\n";
        let blocks = parse_test_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].1["id"], json!("T-2"));
        assert_eq!(blocks[0].1["status"], json!("running"));
        assert_eq!(blocks[0].1["fields"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn append_then_snapshot_archives_terminal_status() {
        let root = temp_project("archive");
        let snapshot = append_test_run(
            &root,
            "cargo test",
            "passed",
            Some("cargo test"),
            Some("全绿"),
            None,
        )
        .unwrap();
        assert_eq!(snapshot["active"].as_array().unwrap().len(), 0);
        assert_eq!(snapshot["archived"].as_array().unwrap().len(), 1);
        let archived = &snapshot["archived"][0];
        assert_eq!(archived["title"], json!("cargo test"));
        assert_eq!(archived["status"], json!("passed"));
        // 归档文件确实落盘。
        assert!(root.join(TEST_RUNS_ARCHIVE_REL).exists());
        let archive_text = std::fs::read_to_string(root.join(TEST_RUNS_ARCHIVE_REL)).unwrap();
        assert!(archive_text.contains("cargo test [passed]"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn running_status_stays_active_until_terminal() {
        let root = temp_project("running");
        append_test_run(&root, "long run", "running", None, None, None).unwrap();
        let snapshot = test_runs_snapshot(&root).unwrap();
        assert_eq!(snapshot["active"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["active"][0]["status"], json!("running"));
        assert_eq!(snapshot["archived"].as_array().unwrap().len(), 0);
        // 终态后自动归档:running 那条仍留 active,passed 那条进 archive。
        append_test_run(&root, "long run", "passed", None, None, None).unwrap();
        let snapshot = test_runs_snapshot(&root).unwrap();
        assert_eq!(snapshot["active"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["active"][0]["status"], json!("running"));
        assert_eq!(snapshot["archived"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["archived"][0]["status"], json!("passed"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn invalid_status_is_rejected() {
        let root = temp_project("invalid");
        let err = append_test_run(&root, "x", "bogus", None, None, None).unwrap_err();
        assert!(err.contains("passed"), "{err}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn tool_records_and_returns_snapshot_text() {
        let root = temp_project("tool");
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let out = TestRecordTool
            .execute(
                json!({"title": "cargo test -p kanzei-llm", "status": "passed", "command": "cargo test -p kanzei-llm"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("active: 0, archived: 1"),
            "{}",
            out.content
        );
        assert!(
            out.content.contains("cargo test -p kanzei-llm [passed]"),
            "{}",
            out.content
        );
        assert!(root.join(TEST_RUNS_REL).exists());
        std::fs::remove_dir_all(&root).ok();
    }

    /// 播一条编号等于"现在"的记录:强制分配器走单调推进分支,让"同一秒内连发"
    /// 这个前提确定成立,而不是靠测试跑得够快去撞运气。
    fn seed_now_baseline(root: &Path) -> u64 {
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let seed = now_secs();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            format!("# Test Runs\n\n## T-{seed} 基线 [running]\n"),
        )
        .unwrap();
        seed
    }

    #[test]
    fn 同秒串行四次登记必须拿到四个互不相同的id() {
        let root = temp_project("sameseconds");
        let seed = seed_now_baseline(&root);
        let titles = [
            "R-153 UI i18n 冒烟",
            "R-153 UI a11y 冒烟",
            "R-153 UI Markdown 冒烟",
            "R-153 UI runtime 冒烟",
        ];
        let mut ids = Vec::new();
        for title in titles {
            let snapshot = append_test_run(&root, title, "running", None, None, None).unwrap();
            ids.push(
                snapshot["recorded_id"]
                    .as_str()
                    .expect("登记必须回显分配到的编号")
                    .to_string(),
            );
        }
        // D-227 的核心判断:串行 ≠ 唯一。这四次是严格顺序执行的(等价于 wave
        // 排他/写租约下的实际执行),旧实现照样会给出同一个 T-<epoch>。
        let unique = ids.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), 4, "同秒四次登记必须拿到四个不同编号:{ids:?}");
        assert!(
            !ids.contains(&format!("T-{seed}")),
            "不得复用已占用的基线编号:{ids:?}"
        );
        let records = read_test_records(&root.join(TEST_RUNS_REL));
        assert_eq!(records.len(), 5, "四条记录加基线都该在:{records:#?}");
        for title in titles {
            assert!(
                records
                    .iter()
                    .any(|(_, r)| r["title"].as_str() == Some(title)),
                "标题 {title} 丢失:{records:#?}"
            );
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn 写排他下并发四次登记编号仍互不相同() {
        let root = temp_project("concurrent");
        seed_now_baseline(&root);
        // 互斥锁模拟生产**已经具备**的写排他:ToolConcurrency::write_worktree 切 wave
        // (harness/tool.rs)+ R-171 的项目写租约。D-227 的要害正在于此——这层排他
        // 生效了(四条记录全部落盘存活即为证据),编号照样撞。
        let gate = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let titles = [
            "R-153 UI i18n 冒烟",
            "R-153 UI a11y 冒烟",
            "R-153 UI Markdown 冒烟",
            "R-153 UI runtime 冒烟",
        ];
        let mut tasks = Vec::new();
        for title in titles {
            let root = root.clone();
            let gate = gate.clone();
            tasks.push(tokio::spawn(async move {
                let _guard = gate.lock().await;
                let ctx = ToolCtx::new(root.clone(), root.clone());
                let out = TestRecordTool
                    .execute(json!({ "title": title, "status": "running" }), &ctx)
                    .await;
                assert!(!out.is_error, "{}", out.content);
                out.content
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        let records = read_test_records(&root.join(TEST_RUNS_REL));
        assert_eq!(records.len(), 5, "四条并发记录加基线都该在:{records:#?}");
        let ids = records
            .iter()
            .map(|(_, r)| r["id"].as_str().unwrap_or_default().to_string())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), 5, "同秒并发登记必须编号互不相同(D-227):{ids:?}");
        for title in titles {
            assert!(
                records
                    .iter()
                    .any(|(_, r)| r["title"].as_str() == Some(title)),
                "标题 {title} 丢失:{records:#?}"
            );
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 新编号必须跳过归档里已占用的最大编号() {
        let root = temp_project("skiparchived");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let future = now_secs() + 500;
        std::fs::write(
            root.join(TEST_RUNS_ARCHIVE_REL),
            format!("# Test Runs Archive\n\n## T-{future} 历史记录 [passed]\n- 收尾: {future}\n"),
        )
        .unwrap();
        let snapshot = append_test_run(&root, "新记录", "running", None, None, None).unwrap();
        assert_eq!(
            snapshot["recorded_id"].as_str().unwrap(),
            format!("T-{}", future + 1),
            "分配器必须把归档里已占用的编号也算进去,否则归档条目会被新记录撞号"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 编号已被占用时拒绝再登记并说明理由() {
        let root = temp_project("idtaken");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            "# Test Runs\n\n## T-500 甲测试 [running]\n",
        )
        .unwrap();
        let before = std::fs::read(root.join(TEST_RUNS_REL)).unwrap();
        let err = ensure_id_unused(&root, "T-500", "乙测试").unwrap_err();
        // D-004:拒绝的理由要说全——冲突编号、已有标题、本次标题、下一步。
        assert!(err.contains("T-500"), "{err}");
        assert!(err.contains("甲测试"), "必须说出已有记录的标题:{err}");
        assert!(err.contains("乙测试"), "必须说出本次要写的标题:{err}");
        assert!(err.contains("未写入"), "必须明说什么都没写:{err}");
        assert!(err.contains("省略 id"), "必须给出可执行的下一步:{err}");
        assert_eq!(
            std::fs::read(root.join(TEST_RUNS_REL)).unwrap(),
            before,
            "拒绝路径不得改动文件"
        );
        assert!(
            ensure_id_unused(&root, "T-501", "丙测试").is_ok(),
            "未占用的编号不该被拦"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn 工具输出必须回显本次分配的编号() {
        let root = temp_project("echoid");
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let out = TestRecordTool
            .execute(json!({ "title": "R-1 长测试", "status": "running" }), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        let records = read_test_records(&root.join(TEST_RUNS_REL));
        let id = records[0].1["id"].as_str().unwrap().to_string();
        // 拿不到编号,「跑完带 id 收尾」这条纪律在源头就无法执行,只能靠标题猜。
        assert!(
            out.content.contains(&format!("recorded {id}")),
            "工具输出必须回显分配到的编号:{}",
            out.content
        );
        assert!(
            out.content.contains(&format!("id={id}")),
            "running 记录必须提示带该 id 收尾:{}",
            out.content
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// 返回文本不得随归档规模增长。
    ///
    /// 原实现把整份归档索引逐条 push 进工具文本:实测本仓归档 683 条时,单次调用
    /// 约 3.6 万字符,其中 ≥99% 与本次写入无关,且每记一条终态测试就给此后**所有**
    /// 调用永久加约 73 字符——一条随项目寿命线性膨胀的上下文税。
    /// 这条测试就是那条税的闸门:归档从 3 条涨到 60 条,输出长度不得跟着涨。
    #[tokio::test]
    async fn 工具输出不随归档规模增长() {
        let root = temp_project("archivescale");
        let ctx = ToolCtx::new(root.clone(), root.clone());

        // 先造 60 条终态记录(会被 snapshot 顺手归档)。
        for i in 0..60 {
            let out = TestRecordTool
                .execute(
                    json!({ "title": format!("R-{i} 历史记录"), "status": "passed" }),
                    &ctx,
                )
                .await;
            assert!(!out.is_error, "{}", out.content);
        }
        let big = TestRecordTool
            .execute(json!({ "title": "R-999 本次", "status": "passed" }), &ctx)
            .await;
        assert!(!big.is_error, "{}", big.content);

        // 只应出现**本次**这一条归档行,不应把历史逐条列出来。
        let bullet_lines = big.content.lines().filter(|l| l.starts_with('○')).count();
        assert!(
            bullet_lines <= 1,
            "归档清单不得逐条回灌(出现 {bullet_lines} 行 ○):\n{}",
            big.content
        );
        assert!(
            big.content.contains("R-999 本次"),
            "本次记录仍必须回显:\n{}",
            big.content
        );
        // 表头要给出归档文件路径——砍掉清单后这是唯一的历史查阅入口。
        assert!(
            big.content.contains("archive: "),
            "表头必须带归档路径,否则模型无处查历史 id:\n{}",
            big.content
        );
        // 绝对上限:与归档条数无关的常数级输出。60 条历史时仍应远小于 4KB。
        assert!(
            big.content.chars().count() < 4000,
            "输出 {} 字符,已随归档规模膨胀:\n{}",
            big.content.chars().count(),
            big.content
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 定点替换不得误伤内容相同的另一条记录() {
        let root = temp_project("splice");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        // 历史遗留:同号且字节相同的两条记录(编号复用的产物)。
        // 旧实现用 str::replace 会把两条一起换掉,收尾一条等于抹掉另一条。
        std::fs::write(
            root.join(TEST_RUNS_REL),
            "# Test Runs\n\n## T-500 甲测试 [running]\n- 命令: cargo test\n\n## T-500 甲测试 [running]\n- 命令: cargo test\n",
        )
        .unwrap();
        let snapshot = record_test_run(
            &root,
            Some("T-500"),
            "甲测试",
            "passed",
            None,
            Some("全绿"),
            None,
        )
        .unwrap();
        assert_eq!(
            snapshot["active"].as_array().unwrap().len(),
            1,
            "只该收尾一条,另一条必须原样留着:{snapshot:#?}"
        );
        assert_eq!(snapshot["active"][0]["status"], json!("running"));
        assert_eq!(
            snapshot["archived"].as_array().unwrap().len(),
            1,
            "被收尾的那条应归档:{snapshot:#?}"
        );
        assert_eq!(snapshot["archived"][0]["status"], json!("passed"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 归档已有同编号且内容不同时拒绝追加() {
        let root = temp_project("archivedup");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_ARCHIVE_REL),
            "# Test Runs Archive\n\n## T-500 甲测试 [passed]\n- 摘要: 甲\n",
        )
        .unwrap();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            "# Test Runs\n\n## T-500 乙测试 [passed]\n- 摘要: 乙\n",
        )
        .unwrap();
        let archive_before = std::fs::read(root.join(TEST_RUNS_ARCHIVE_REL)).unwrap();
        let active_before = std::fs::read(root.join(TEST_RUNS_REL)).unwrap();
        let err = test_runs_snapshot(&root).unwrap_err();
        assert!(err.contains("T-500"), "{err}");
        assert!(err.contains("未写入"), "必须明说什么都没写:{err}");
        assert_eq!(
            std::fs::read(root.join(TEST_RUNS_ARCHIVE_REL)).unwrap(),
            archive_before,
            "拒绝路径不得改动归档"
        );
        assert_eq!(
            std::fs::read(root.join(TEST_RUNS_REL)).unwrap(),
            active_before,
            "拒绝路径不得改动活动记录"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 编号保持纯u64且领先墙钟时不判悬空() {
        let root = temp_project("monotonic");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let ahead = now_secs() + 50;
        std::fs::write(
            root.join(TEST_RUNS_REL),
            format!("# Test Runs\n\n## T-{ahead} 基线 [running]\n"),
        )
        .unwrap();
        let snapshot = append_test_run(&root, "新记录", "running", None, None, None).unwrap();
        let id = snapshot["recorded_id"].as_str().unwrap().to_string();
        // 纯 u64:running_age_secs 与 last_passed_at 都靠 parse::<u64>(),
        // 一旦改成带后缀的编号,悬空检测和提交门禁会静默失效。
        let stamp = id.strip_prefix("T-").expect("编号必须形如 T-<整数>");
        assert!(
            stamp.parse::<u64>().is_ok(),
            "编号必须保持纯 u64,不得加后缀:{id}"
        );
        assert_eq!(
            running_age_secs(&id),
            Some(0),
            "编号领先墙钟时 age 应饱和到 0,而不是让字段整个消失"
        );
        let fresh = snapshot["active"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"].as_str() == Some(id.as_str()))
            .expect("新记录应在 active 中")
            .clone();
        assert_eq!(
            fresh["stale"],
            json!(false),
            "新记录不该被判悬空:{fresh:#?}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// D-261 验收① 的机械守护:生产路径不得再出现裸 `fs::write`。
    ///
    /// 做成守护测试而不是"评审时记得 grep 一下":两套写原语的复发形态就是顺手写
    /// 一行 `std::fs::write`,它在 diff 里长得完全无害,而代价是这个文件重新拥有
    /// 一套与 `atomic_file` 不同的失败语义(先截断再写、无临时文件留证)。
    /// 注释行不计入,否则连"为什么不用它"都不许写下来。
    #[test]
    fn 生产路径不得出现裸fs_write() {
        let source = include_str!("test_record.rs");
        // 按行切而不是按 "\n#[cfg(test)]\n" 整串匹配:仓库在 Windows 上检出的是
        // CRLF,整串匹配会静默落空。找不到标记时 take_while 会把测试区也算进来,
        // 于是这条直接红——宁可吵闹地失败,也不要悄悄退化成"什么都没检查"。
        let hits: Vec<&str> = source
            .lines()
            .take_while(|line| line.trim() != "#[cfg(test)]")
            .filter(|line| !line.trim_start().starts_with("//"))
            .filter(|line| line.contains("fs::write"))
            .collect();
        assert!(
            hits.is_empty(),
            "生产路径出现裸 fs::write,必须改走 atomic_file::write_atomic(D-261):{hits:#?}"
        );
    }

    /// D-261 验收②:并发登记既不撞号也不丢记录。
    ///
    /// 与 D-227 那条并发用例的区别是**没有 gate**:那条用互斥锁模拟已有的写排他,
    /// 证明"串行了照样撞号";这条不加任何外部串行,证明事务锁本身就够——写入口
    /// 自己扛得住并发,不依赖 wave 排他或写租约先把调用方排好队。
    #[test]
    fn 并发登记不撞号也不丢记录() {
        let root = temp_project("lockstress");
        // 播一条编号等于"现在"的基线,强制分配器走单调推进分支:否则墙钟秒一变,
        // 各线程天然拿到不同编号,用例就失去证明力。
        seed_now_baseline(&root);
        let 并发: usize = 8;
        let 线程: Vec<_> = (0..并发)
            .map(|i| {
                let root = root.clone();
                std::thread::spawn(move || {
                    append_test_run(&root, &format!("并发登记 {i}"), "running", None, None, None)
                })
            })
            .collect();
        for (i, 句柄) in 线程.into_iter().enumerate() {
            句柄
                .join()
                .unwrap()
                .unwrap_or_else(|e| panic!("第 {i} 次登记失败: {e}"));
        }
        let records = read_test_records(&root.join(TEST_RUNS_REL));
        assert_eq!(
            records.len(),
            并发 + 1,
            "{并发} 条并发记录加基线都该在(丢记录 = 有人的整文件回写覆盖了别人):{records:#?}"
        );
        let ids = records
            .iter()
            .map(|(_, r)| r["id"].as_str().unwrap_or_default().to_string())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), 并发 + 1, "并发登记必须编号互不相同:{ids:?}");
        for i in 0..并发 {
            let title = format!("并发登记 {i}");
            assert!(
                records
                    .iter()
                    .any(|(_, r)| r["title"].as_str() == Some(title.as_str())),
                "标题「{title}」丢失:{records:#?}"
            );
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// D-261 验收②:锁必须罩住**整段**事务,而不是只罩落盘那一下。
    ///
    /// 判据取"外部持锁期间文件没被创建":登记若能在此期间读到已占用编号并写回,
    /// 就说明读与写之间是敞开的——两个进程正是在这个缝里算出同一个 id 的。
    /// 跨进程那一层的机械证据在 atomic_file 的「独占句柄第二次打开必然失败」,
    /// 这里证明的是本模块把事务边界画在了正确的位置。
    #[test]
    fn 外部持锁期间登记必须等待而不是抢先写入() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let root = temp_project("lockspan");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let 目标 = root.join(TEST_RUNS_REL);
        // FileLock 是 !Send:锁在本线程取、本线程放,登记放到另一个线程去撞。
        let 锁 = crate::atomic_file::lock_exclusive(&目标).unwrap();
        let 完成 = Arc::new(AtomicBool::new(false));
        let 线程 = {
            let root = root.clone();
            let 完成 = 完成.clone();
            std::thread::spawn(move || {
                append_test_run(&root, "被挡住的登记", "running", None, None, None).unwrap();
                完成.store(true, Ordering::SeqCst);
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(!完成.load(Ordering::SeqCst), "持锁期间不该有人把登记做完");
        assert!(
            !目标.exists(),
            "持锁期间 tests.md 不该被创建:说明写入没等锁"
        );
        drop(锁);
        线程.join().unwrap();
        assert!(完成.load(Ordering::SeqCst), "释放后必须能完成登记");
        assert_eq!(
            read_test_records(&目标).len(),
            1,
            "等到的那次登记必须真的落盘"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// 快照顺手做的那次幂等归档拿不到锁时**跳过**,而不是报错或干等。
    ///
    /// 它是只读路径(面板刷新)上的维护动作:晚归档一轮没有代价,让面板卡住或弹
    /// 一个错误框有代价。真失败(编号复用)仍照常报错,那条由
    /// `归档已有同编号且内容不同时拒绝追加` 守着。
    #[test]
    fn 快照归档拿不到锁时跳过而不是报错() {
        let root = temp_project("archiveskip");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let 目标 = root.join(TEST_RUNS_REL);
        std::fs::write(
            &目标,
            "# Test Runs\n\n## T-700 甲测试 [passed]\n- 摘要: 全绿\n",
        )
        .unwrap();
        let 锁 = crate::atomic_file::lock_exclusive(&目标).unwrap();
        let snapshot = {
            let root = root.clone();
            std::thread::spawn(move || test_runs_snapshot(&root))
                .join()
                .unwrap()
        }
        .expect("拿不到锁不是错误:面板必须照常拿到读结果");
        assert_eq!(
            snapshot["active"].as_array().unwrap().len(),
            1,
            "跳过归档时终态记录仍留在 active,读结果照常返回:{snapshot:#?}"
        );
        assert!(
            !root.join(TEST_RUNS_ARCHIVE_REL).exists(),
            "拿不到锁就不该写归档文件"
        );
        drop(锁);
        // 锁一放,下一次快照把这轮欠下的归档补上——跳过是延后,不是丢弃。
        let snapshot = test_runs_snapshot(&root).unwrap();
        assert_eq!(snapshot["active"].as_array().unwrap().len(), 0);
        assert_eq!(snapshot["archived"].as_array().unwrap().len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    /// D-259:显式修复动作把历史同号记录逐条改成未占用编号,保留标题/字段。
    #[test]
    fn 修复归档重复编号_保留第一条其余改号且字段一字不动() {
        let root = temp_project("repair");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            "# Test Runs\n\n## T-500 活跃记录 [running]\n- 摘要: 还在跑\n",
        )
        .unwrap();
        std::fs::write(
            root.join(TEST_RUNS_ARCHIVE_REL),
            "# Test Runs Archive\n\n\
             ## T-1000 甲测试 [passed]\n- 命令: cargo test -p a\n- 摘要: 甲\n- 关联: R-1\n- 收尾: 1000\n\n\
             ## T-1000 乙测试 [passed]\n- 命令: cargo test -p b\n- 摘要: 乙\n- 关联: R-2\n- 收尾: 1001\n",
        )
        .unwrap();

        let report = repair_reused_archived_id(&root, "T-1000").unwrap();
        assert!(report.contains("保留第一条「甲测试」"), "{report}");
        assert!(report.contains("T-1000「乙测试」→ T-"), "{report}");
        assert!(report.contains("一字未动"), "{report}");

        // 全部编号唯一,且不与 active/archive 现存编号冲突(ensure_id_unused 判据)。
        let archived = read_test_records(&root.join(TEST_RUNS_ARCHIVE_REL));
        let ids: Vec<&str> = archived
            .iter()
            .map(|(_, r)| r["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids[0], "T-1000", "第一条保留原编号:{ids:?}");
        assert_ne!(ids[1], "T-1000", "第二条必须改号:{ids:?}");
        let all_ids: Vec<String> = [TEST_RUNS_REL, TEST_RUNS_ARCHIVE_REL]
            .iter()
            .flat_map(|rel| read_test_records(&root.join(rel)))
            .map(|(_, r)| r["id"].as_str().unwrap().to_string())
            .collect();
        let mut sorted = all_ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            all_ids.len(),
            "修复后编号必须全部唯一:{all_ids:?}"
        );

        // 标题/状态/命令/摘要/关联/收尾一字不动。
        let (block2, rec2) = &archived[1];
        assert_eq!(rec2["title"], json!("乙测试"));
        assert_eq!(rec2["status"], json!("passed"));
        for need in [
            "- 命令: cargo test -p b",
            "- 摘要: 乙",
            "- 关联: R-2",
            "- 收尾: 1001",
        ] {
            assert!(block2.contains(need), "字段被改坏了: {block2}");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// D-259:只有一条或不存在时拒绝修复,不做任何写。
    #[test]
    fn 修复单条编号时拒绝且不改文件() {
        let root = temp_project("repair1");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let before = "# Test Runs Archive\n\n## T-2000 唯一测试 [passed]\n- 摘要: 全绿\n";
        std::fs::write(root.join(TEST_RUNS_ARCHIVE_REL), before).unwrap();

        let err = repair_reused_archived_id(&root, "T-2000").unwrap_err();
        assert!(err.contains("只有 1 条"), "{err}");
        let err = repair_reused_archived_id(&root, "T-9999").unwrap_err();
        assert!(err.contains("只有 0 条"), "{err}");
        assert_eq!(
            std::fs::read_to_string(root.join(TEST_RUNS_ARCHIVE_REL)).unwrap(),
            before,
            "拒绝时必须一字不改"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// D-259:工具层把 repair_reused_archived_id 字段分派到修复动作并返回报告。
    #[tokio::test]
    async fn tool_repair_reused_archived_id_dispatches() {
        let root = temp_project("toolrepair");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_ARCHIVE_REL),
            "# Test Runs Archive\n\n\
             ## T-3000 丙 [passed]\n- 摘要: 丙\n\n\
             ## T-3000 丁 [passed]\n- 摘要: 丁\n",
        )
        .unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let out = TestRecordTool
            .execute(
                json!({
                    "title": "占位(修复动作忽略标题)",
                    "status": "passed",
                    "repair_reused_archived_id": "T-3000"
                }),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("保留第一条「丙」"), "{}", out.content);
        assert!(out.content.contains("T-3000「丁」→ T-"), "{}", out.content);
        // 未提供修复字段时走正常记录路径,不受影响。
        let out2 = TestRecordTool
            .execute(
                json!({"title": "cargo test -p k", "status": "passed"}),
                &ctx,
            )
            .await;
        assert!(!out2.is_error, "{}", out2.content);
        std::fs::remove_dir_all(&root).ok();
    }

    /// R-228 验收②:前端冒烟识别——`node scripts/ui-*.mjs` 运行型冒烟算,
    /// `node --check`(纯语法)不算,`cargo test` 不算。取最近一条 passed。
    // D-371:声称「前端冒烟全过」必须覆盖 verify.ps1 六条冒烟,差集非空即判红。
    #[test]
    fn d371_声称冒烟但只跑四条被拒() {
        let root = temp_project("d371-subset");
        let err = append_test_run(
            &root,
            "R-999 四条前端冒烟全过",
            "passed",
            Some("node scripts/ui-runtime-smoke.mjs; node scripts/ui-i18n-smoke.mjs; node scripts/ui-a11y-smoke.mjs; node scripts/ui-markdown-smoke.mjs"),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.contains("差集非空"), "{err}");
        assert!(err.contains("ui-lint-smoke"), "{err}");
        assert!(err.contains("parallel-lines-regression"), "{err}");
    }

    #[test]
    fn d371_六条全跑通过() {
        let root = temp_project("d371-full");
        let ok = append_test_run(
            &root,
            "R-999 前端冒烟六连全过",
            "passed",
            Some("node scripts/ui-runtime-smoke.mjs; node scripts/ui-lint-smoke.mjs; node scripts/parallel-lines-regression.mjs; node scripts/ui-a11y-smoke.mjs; node scripts/ui-i18n-smoke.mjs; node scripts/ui-markdown-smoke.mjs"),
            None,
            None,
        );
        assert!(ok.is_ok(), "{:?}", ok.err());
    }

    #[test]
    fn d371_非冒烟标题与running状态不受影响() {
        let root = temp_project("d371-other");
        // cargo 记录 title 不含「冒烟」,即使没命令也不触发六条校验。
        let ok = append_test_run(
            &root,
            "cargo test -p kanzei-memory",
            "passed",
            None,
            None,
            None,
        );
        assert!(ok.is_ok(), "{:?}", ok.err());
        // running 状态不触发(在跑,不是声称全过)。
        let ok2 = append_test_run(&root, "前端冒烟收集中", "running", None, None, None);
        assert!(ok2.is_ok(), "{:?}", ok2.err());
    }

    #[test]
    fn d371_声称冒烟但无命令被拒() {
        let root = temp_project("d371-nocmd");
        let err = append_test_run(&root, "前端冒烟全过", "passed", None, None, None).unwrap_err();
        assert!(err.contains("无法核验"), "{err}");
    }

    // D-371 验收④:回溯核查——历史「只跑四条却报全绿」的声称(R-253 B9 的形态)在新判据下会被拦下。
    #[test]
    fn d371_历史声称四条的记录会被新判据拦下() {
        let root = temp_project("d371-replay");
        let err = append_test_run(
            &root,
            "R-253 批9 四条前端冒烟(ui-runtime/i18n/a11y/markdown)",
            "passed",
            Some("node scripts/ui-runtime-smoke.mjs; node scripts/ui-i18n-smoke.mjs; node scripts/ui-a11y-smoke.mjs; node scripts/ui-markdown-smoke.mjs"),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.contains("差集非空"), "{err}");
    }

    #[test]
    fn frontend_smoke_passed_recognizes_ui_smoke_and_ignores_syntax_and_cargo() {
        let root = temp_project("frontend-gate");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        // 只有 cargo test passed:前端标签任务关闭应被拒(无前端冒烟)。
        std::fs::write(
            root.join(TEST_RUNS_REL),
            "# Test Runs\n\n## T-1 cargo [passed]\n- 命令: cargo test --workspace\n- 收尾: 100\n",
        )
        .unwrap();
        assert!(
            frontend_smoke_passed(&root).is_none(),
            "cargo test 不是前端冒烟"
        );
        // 只有 node --check:语法检查不算冒烟(验收②:smoke 断言过时才带病过关)。
        std::fs::write(
            root.join(TEST_RUNS_REL),
            "# Test Runs\n\n## T-2 syntax [passed]\n- 命令: node --check ui/07-events.js\n- 收尾: 200\n",
        )
        .unwrap();
        assert!(
            frontend_smoke_passed(&root).is_none(),
            "node --check 只查语法,不算前端冒烟"
        );
        // 前端运行型冒烟:识别通过,取最近收尾。
        std::fs::write(
            root.join(TEST_RUNS_REL),
            "# Test Runs\n\n## T-3 runtime [passed]\n- 命令: node scripts/ui-runtime-smoke.mjs\n- 收尾: 300\n\
             \n## T-4 i18n [passed]\n- 命令: node scripts/ui-i18n-smoke.mjs\n- 收尾: 400\n",
        )
        .unwrap();
        let got = frontend_smoke_passed(&root).expect("前端冒烟 passed 应被识别");
        assert_eq!(got.0, 400, "应取最近一条前端冒烟:{got:?}");
        std::fs::remove_dir_all(&root).ok();
    }
}
