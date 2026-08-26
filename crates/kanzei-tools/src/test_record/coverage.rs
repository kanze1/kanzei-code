//! 测试记录的覆盖面解析、背书查询与前端冒烟判定。
//!
//! 这些函数共享父模块的记录解析器与路径常量，但不参与写事务；单独成域让
//! `test_record` 的写入协议与测试背书查询分别可读，同时保持原有公开 API。

use std::path::Path;

use serde_json::json;

use super::{now_secs, read_test_records, TEST_RUNS_ARCHIVE_REL, TEST_RUNS_REL};

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
    // 匹配走背书语义而非串相等:v2 清单允许「记录是超集」(同一轮测试分多笔提交,
    // 后续批次的暂存内容仍被首轮记录背书);相等仍然成立,旧格式只能相等。
    let newest = passed
        .iter()
        .filter(|record| {
            fingerprint_filter
                .is_none_or(|expected| crate::git::fingerprint_endorses(&record.3, expected))
        })
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

/// D-664:关闭涉及设计文档或显著变更的条目时，验证证据必须同时满足三点：
/// ① `dist/verification.json` 绑定当前 HEAD；② verify 全部通过；③存在关联该条目的
/// passed test_record 且命令确实调用 `verify.ps1`。只看一个全局旧记录会把别的提交的
/// 证据错归到当前关闭，因此不接受无当前 HEAD 绑定的记录。
pub fn verification_passed_for(root: &Path, entry_id: &str) -> bool {
    let Some(head) = current_head(root) else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(root.join("dist/verification.json")) else {
        return false;
    };
    let Ok(evidence) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    if evidence["commit"].as_str() != Some(head.as_str())
        || evidence["all_pass"].as_bool() != Some(true)
    {
        return false;
    }
    records_for_entry(root, entry_id).into_iter().any(|record| {
        record["status"].as_str() == Some("passed")
            && record_command_text(&record).contains("verify.ps1")
    })
}

/// R-309 B3:verify 产出的当前 HEAD 证据可替代重复的前端冒烟记录。
///
/// targeted verify 也可能运行全部前端步骤,所以这里只要求关闭门禁真正需要的三项
/// (`ui_runtime` / `ui_lint` / `ui_i18n`)通过;是否 full verify 由 package 门禁另行判断。
fn verification_frontend_smoke_passed(root: &Path, expected_commit: &str) -> Option<(u64, String)> {
    let path = root.join("dist/verification.json");
    let text = std::fs::read_to_string(&path).ok()?;
    let evidence: serde_json::Value = serde_json::from_str(&text).ok()?;
    if evidence["commit"].as_str() != Some(expected_commit)
        || evidence["all_pass"].as_bool() != Some(true)
    {
        return None;
    }
    let checks = evidence["checks"].as_object()?;
    for key in ["ui_runtime", "ui_lint", "ui_i18n"] {
        let passed = checks
            .get(key)
            .and_then(|value| value.as_str())
            .is_some_and(|value| value.starts_with("pass "));
        if !passed {
            return None;
        }
    }
    let at = std::fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())?;
    Some((
        at,
        "dist/verification.json (ui_runtime/ui_lint/ui_i18n)".to_string(),
    ))
}

fn current_head(root: &Path) -> Option<String> {
    let mut command = std::process::Command::new("git");
    crate::hide_console(&mut command);
    let output = command
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let head = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!head.is_empty()).then_some(head)
}

const FRONTEND_SMOKE_MAX_AGE_SECS: u64 = 24 * 60 * 60;

/// 旧 test_record 仍可作为兼容路径,但不能再靠三天前的 passed 记录放行。
/// 有当前工作区源码指纹时必须由记录背书;干净树无法生成工作区差集时仍由时间窗兜底。
fn record_is_fresh(root: &Path, record: &serde_json::Value, finished: u64) -> bool {
    if now_secs().saturating_sub(finished) > FRONTEND_SMOKE_MAX_AGE_SECS {
        return false;
    }
    let Some(current) = crate::git::source_endorsement_fingerprint(root)
        .ok()
        .filter(|value| !value.is_empty())
    else {
        return true;
    };
    let recorded = record["fields"]
        .as_array()
        .and_then(|fields| {
            fields
                .iter()
                .find(|field| field["key"].as_str() == Some("源码指纹"))
                .and_then(|field| field["value"].as_str())
        })
        .unwrap_or_default();
    !recorded.is_empty() && crate::git::fingerprint_endorses(recorded, &current)
}

/// R-228/R-309 B3:最近一次可用于关闭前端条目的验证证据(收尾时刻, 标题)。
///
/// 优先消费 `dist/verification.json`:它必须绑定当前 HEAD,且三项关闭所需 UI 检查均为
/// pass。没有证据时兼容旧 test_record,但要求记录不超过 24 小时且通过当前源码指纹
/// 背书(若工作区存在源码差集)。
pub fn frontend_smoke_passed(root: &Path) -> Option<(u64, String)> {
    if let Some(head) = current_head(root) {
        if let Some(evidence) = verification_frontend_smoke_passed(root, &head) {
            return Some(evidence);
        }
    }

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
            if let Some(at) = finished.filter(|at| record_is_fresh(root, &record, *at)) {
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

/// 门禁只对**本项目真的提供了的**脚本生效。
///
/// 这六个名字和 `scripts/verify.ps1` 都是 kanzei 自己仓库的形状。同一套 harness
/// 跑在别的项目上时,项目里根本没有 `scripts/ui-*.mjs`——门禁照旧索要,条目就永远
/// 关不掉,agent 只能一轮轮去 glob 一个不存在的脚本,或者把测试记录标题从「前端冒烟」
/// 改成「定向测试」来绕开判据。后者尤其糟:门禁把模型教成了**谎报自己跑了什么**。
///
/// 判据因此改成"能力条件式":项目提供了哪几条,就要求哪几条;一条都没有就不是
/// 「没跑」,而是「这个项目没有这类冒烟」,门禁自动让开。
pub(crate) fn available_frontend_smokes(root: &Path) -> Vec<&'static str> {
    FRONTEND_SMOKE_LIST
        .iter()
        .copied()
        .filter(|script| root.join("scripts").join(script).is_file())
        .collect()
}

/// 本项目是否提供 `scripts/verify.ps1`。关闭门禁索要 verify 全绿证据之前必须先问这一句:
/// 没有这个脚本的项目里,「先跑 verify.ps1」是一条无法执行的指令。
pub fn project_has_verify_script(root: &Path) -> bool {
    root.join("scripts").join("verify.ps1").is_file()
}

/// D-371 机械判据:title 声称「冒烟」且 status=passed 时,command 必须覆盖
/// verify.ps1 的六条前端冒烟,差集非空即拒绝写入——「全绿」的定义是 verify.ps1
/// 十步,不是任意子集。未提供命令时同样拒绝(无法核验 = 判红)。
pub(super) fn check_frontend_smoke_claim(
    root: &Path,
    title: &str,
    command: Option<&str>,
    status: &str,
) -> Result<(), String> {
    if status != "passed" || !title.contains("冒烟") {
        return Ok(());
    }
    // 只按本项目真的提供的冒烟脚本判。一条都没有 = 这个项目没有前端冒烟
    // 这回事,不是「跑了子集、报了全称」,门禁让开。原判据把 kanzei 自己仓库的
    // 六个脚本名当成普遍真理,别的项目里它无法满足——实测代价是模型把测试记录
    // 标题从「前端冒烟」改成「定向测试」来绕开,门禁反过来教会了谎报跑过什么。
    let expected = available_frontend_smokes(root);
    if expected.is_empty() {
        return Ok(());
    }
    let Some(command) = command.map(str::trim).filter(|c| !c.is_empty()) else {
        return Err(format!(
            "声称「{title}」是前端冒烟全过,但未提供命令,无法核验覆盖。\
             本项目提供 {} 条冒烟({}),差集非空即判红(D-371)",
            expected.len(),
            expected.join(" ")
        ));
    };
    let missing: Vec<&str> = expected
        .iter()
        .copied()
        .filter(|script| !command.contains(script))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "声称「{title}」是前端冒烟,但命令只覆盖 {}/{} 条,缺:{}。\
         差集非空即判红(D-371)。请补跑:{}",
        expected.len() - missing.len(),
        expected.len(),
        missing.join("、"),
        expected.join(" ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-coverage-{tag}-{}-{}",
            std::process::id(),
            now_secs()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        root
    }

    #[test]
    fn verification_evidence_requires_current_commit_and_three_ui_checks() {
        let root = temp_root("evidence");
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(
            root.join("dist/verification.json"),
            r#"{
                "commit": "head-1",
                "all_pass": true,
                "checks": {
                    "ui_runtime": "pass 1.0s",
                    "ui_lint": "pass 1.0s",
                    "ui_i18n": "pass 1.0s"
                }
            }"#,
        )
        .unwrap();
        assert!(verification_frontend_smoke_passed(&root, "head-1").is_some());
        assert!(verification_frontend_smoke_passed(&root, "head-2").is_none());

        std::fs::write(
            root.join("dist/verification.json"),
            r#"{
                "commit": "head-1",
                "all_pass": true,
                "checks": {
                    "ui_runtime": "pass 1.0s",
                    "ui_lint": "fail 1.0s",
                    "ui_i18n": "pass 1.0s"
                }
            }"#,
        )
        .unwrap();
        assert!(verification_frontend_smoke_passed(&root, "head-1").is_none());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn stale_frontend_record_is_rejected() {
        let root = temp_root("stale");
        let stale = now_secs().saturating_sub(FRONTEND_SMOKE_MAX_AGE_SECS + 1);
        std::fs::write(
            root.join(TEST_RUNS_REL),
            format!(
                "# Test Runs\n\n## T-1 old smoke [passed]\n- 命令: node scripts/ui-runtime-smoke.mjs\n- 收尾: {stale}\n"
            ),
        )
        .unwrap();
        assert!(frontend_smoke_passed(&root).is_none());
        std::fs::remove_dir_all(root).ok();
    }
}
