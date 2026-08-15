//! Git 交付工作流域(R-257 B4):finalize 事务化交付(fmt → 相关测试 → test_record →
//! stage → CAS commit)与全部提交门禁组件(staged 指纹/占位符 ID 门禁/compile/fmt/
//! clippy/source_test 门禁)。按 R-257 定位:finalize 是 delivery workflow,不是
//! 一个 git action。自 git.rs 原样迁出,零行为变更。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use kanzei_harness::{ToolCtx, ToolOutput};

use super::commands::{commit, stage};

/// D-332 验收④:暂存**源码**的指纹 hash(fnv)——test_record 收尾时用它背书「测试跑的是
/// 这份源码」,提交门禁优先比指纹而不是纯 mtime。要点:
/// - 只对源码路径(`is_source_path`)的 staged diff 求 hash:tests.md/tracker 等托管文档
///   的写入不改变源码指纹,不会再让「test_record 自己改 tests.md」触发源码重测。
/// - 与 staged_state 的全体 hash 不同:commit 门禁的 CAS 用全体 hash(防任何内容漂移),
///   测试背书用源码 hash(fmt 后源码 diff 变 → 指纹变 → 要求重测,保守正确)。
/// - 同步实现(内部 std::process::Command 跑 git):test_record 工具(async)与
///   source_test_gate(同步)都要用,拆两个 async 版本会让门禁被迫改签名。
pub fn staged_source_fingerprint(cwd: &Path) -> Result<String, String> {
    let mut command = std::process::Command::new("git");
    // D-369:kzapp 是 GUI 进程无控制台,子进程跑 git 不隐藏会被 Windows 新建控制台
    // 窗口——提交门禁每次调用都弹黑窗闪现。与 D-238 的 async 路径同纪律。
    crate::hide_console(&mut command);
    let output = command
        .args([
            "diff",
            "--cached",
            "--binary",
            "--no-ext-diff",
            "--no-color",
        ])
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("cannot run git diff: {e}"))?;
    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    let paths = staged_paths_sync(cwd)?;
    let source_paths: Vec<String> = paths
        .iter()
        .filter(|p| is_source_path(p))
        .cloned()
        .collect();
    if source_paths.is_empty() {
        // 本次暂存全是非源码(测试记录/文档):指纹为空,门禁按旧逻辑(mtime)走。
        return Ok(String::new());
    }
    let mut hasher = DefaultHasher::new();
    // 只对源码路径的 diff 段求 hash:按 `diff --git a/<path> b/<path>` 头切块,
    // 命中源码路径的块进 hash,其余(测试记录/文档)排除。
    let mut in_source = false;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            let path = rest
                .split_once(" b/")
                .map(|(p, _)| p.to_string())
                .unwrap_or_default();
            in_source = source_paths.contains(&path);
        }
        if in_source {
            line.hash(&mut hasher);
        }
    }
    Ok(format!("{:016x}", hasher.finish()))
}

fn staged_paths_sync(cwd: &Path) -> Result<Vec<String>, String> {
    let mut command = std::process::Command::new("git");
    // D-369:同 staged_source_fingerprint——GUI 进程跑 git 必须隐藏控制台窗口,
    // 否则提交门禁每次弹黑窗。
    crate::hide_console(&mut command);
    let output = command
        .args([
            "-c",
            "core.quotepath=false", // D-347:非 ASCII 路径以真实 UTF-8 返回,与请求路径可比
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
        ])
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("cannot run git diff --name-only: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git diff --cached failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

/// 提交里算「源码」的路径。改这两棵树就要有测试背书;`.kanzei/` 下的文档不算。
///
/// R-261:kanzei-app/ui/ 下的纯前端资源(js/css/html)不算 Rust source——它们由前端
/// 冒烟集(node --check + ui-runtime/lint/i18n/a11y/markdown,R-228 强制前端标签条目
/// 关闭前有 passed 冒烟)背书,要求 cargo test -p kanzei-app 跑全套 Rust 测试对它们
/// 零信息量(实测 R-260 改 10 行 js 被迫重跑 163 个 Rust 测试)。staged 同时含 Rust
/// 源码与前端资源时,Rust 部分仍按原规则要求测试背书,不受影响。
pub(crate) fn is_source_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let frontend_resource = path.starts_with("crates/kanzei-app/ui/");
    (path.starts_with("crates/") || path.starts_with("scripts/"))
        && !path.contains("/.kanzei/")
        && !frontend_resource
}

/// 提交里算「tracker 文档」的路径:需求/缺陷/测试记录及其归档。
/// R-227 门禁只扫这些文件——占位符测试 ID 只可能出现在关闭证据/进展叙述里。
fn is_tracker_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let file = path.rsplit('/').next().unwrap_or(&path);
    matches!(
        file,
        "requirements.md"
            | "requirements-archive.md"
            | "defects.md"
            | "defects-archive.md"
            | "tests.md"
            | "tests-archive.md"
    )
}

/// R-227:占位符测试 ID 门禁。tracker 文件 diff 里出现 `T-\d+xxx` 形态的占位符
/// (真实测试 ID 是 `T-<10位时间戳>` 如 `T-1786565253`;占位符是数字后直接跟 xxx)
/// 即拒——把「全量跑过但没记 test_record」写成占位符,等于隔时凭记忆写证据,
/// R-198/R-199 的关闭证据就是这么漏出 D-320 的。只扫 tracker 文件的 diff 块,
/// 新增行(以 `+` 开头)与删除行(以 `-` 开头)都查:占位符不该出现在任何一侧。
pub(crate) fn placeholder_id_gate(diff: &str, paths: &[String]) -> Result<(), String> {
    let tracker: Vec<String> = paths
        .iter()
        .filter(|p| is_tracker_path(p))
        .cloned()
        .collect();
    if tracker.is_empty() {
        return Ok(());
    }
    // 占位符模式:`T-` + 至少一位数字 + `xxx`(数字不能为空,避免误伤 `T-xxx` 说明文字;
    // 也不能是完整 10 位时间戳——那是真 ID)。真实 ID 之后绝不该跟 `xxx`。
    // 手写扫描替代 regex:找 `T-` 后数连续数字,数字后紧跟 `xxx` 即命中。
    let is_placeholder = |line: &str| {
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i + 6 <= bytes.len() {
            if &bytes[i..i + 2] == b"T-" {
                let mut j = i + 2;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 2 && line[j..].starts_with("xxx") {
                    return true;
                }
            }
            i += 1;
        }
        false
    };
    // D-357:只扫**新增行**。删除行里的占位符正是这次提交要清掉的东西,连它一起拒,
    // 等于门禁把自己配套的清理通道(archive_fill 回填)堵死——回填之后的 diff 必然
    // 带着 8 行 `-...T-1786565xxx...`,提交一次拒一次。人还能在 shell 里绕过去,
    // 自举 agent 只能走结构化 git 工具,没有退路,于是「按门禁要求回填」这件事
    // 在 agent 手里永远做不完。`+++ b/path` 是文件头不是内容,先剔掉。
    let mut hits: Vec<String> = Vec::new();
    for line in diff.lines() {
        let Some(added) = line.strip_prefix('+') else {
            continue;
        };
        if added.starts_with("++") {
            continue;
        }
        if is_placeholder(added) {
            hits.push(added.trim().to_string());
        }
    }
    if hits.is_empty() {
        return Ok(());
    }
    let truncated: Vec<String> = hits
        .iter()
        .map(|l| {
            if l.chars().count() > 160 {
                format!("{}…", l.chars().take(160).collect::<String>())
            } else {
                l.clone()
            }
        })
        .collect();
    Err(format!(
        "tracker 文件 diff 出现 {} 处占位符测试 ID(`T-<数字>xxx` 形态):\n{}\n\
         占位符 = 把「测试跑过但没记 test_record」隔时凭记忆写进证据,正是 D-320 根因链。\
         先 test_record 记真实 ID 再引用;存量占位符用 `archive_fill` 回填真值。",
        truncated.len(),
        truncated.join("\n")
    ))
}

fn modified_secs(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// 编译底线门禁。R-210 曾把它降级成诊断回退(理由:clippy 的 `--all-targets` 走同一
/// 编译管线,双份全仓分析是冗余)。现已复位成**主门禁**:实测 `clippy --all-targets`
/// 37.9s 而 `check --all-targets` 只要 7.2s——同样的编译覆盖,四分之一的价钱。lint 那半
/// 由 clippy_gate 用不含测试目标的轻量形态接手。R-210 的判断没错,只是方向反了:
/// 该让便宜的那个覆盖编译,而不是让贵的那个顺带覆盖。
///
/// 为什么不能只看测试记录:记录是 agent 自己写的。实测 2026-08-09 夜里,run.rs 里连续
/// 混入四处「插入却把签名吃掉」的破损(`async fn fast_summarize -> ...` 少了参数、
/// `pub(crate) async fn run_promptpub(crate) fn run_metrics(` 两个签名黏在一起),
/// 而每个提交都配着一条 passed 记录——记录的时间戳比改动新,时序门禁完全满意,
/// 但 kanzei-app 根本编译不过。时序判据防的是「改完没重跑」,防不住「没跑却说跑了」。
/// 编译这条底线必须由工具亲自验。
async fn compile_gate(cwd: &Path) -> Result<(), String> {
    // 非 Rust 仓库不做这件事:门禁要么真的能验,要么不装样子。
    if !cwd.join("Cargo.toml").is_file() {
        return Ok(());
    }
    let mut command = tokio::process::Command::new("cargo");
    command
        .args(["check", "--workspace", "--all-targets", "--quiet"])
        .current_dir(cwd);
    crate::hide_console_async(&mut command);
    let output = command.output().await;
    match output {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let head: Vec<&str> = stderr
                .lines()
                .filter(|l| l.starts_with("error") || l.trim_start().starts_with("-->"))
                .take(12)
                .collect();
            Err(format!(
                "提交被拦下:`cargo check --workspace --all-targets` 不过,这份代码编译不了。\n{}",
                head.join("\n")
            ))
        }
        // cargo 跑不起来就说清楚,不要静默放行——放行等于门禁在说谎。
        Err(error) => Err(format!(
            "提交被拦下:无法执行 cargo check({error})。装好 cargo 或在非 Rust 仓库里提交。"
        )),
    }
}

/// fmt 门禁:提交源码前由工具**亲自**跑 `cargo fmt --all -- --check`(D-264)。
///
/// 与 compile_gate 同理由:测试记录是自报证据,挡不住「没跑却说跑了」。规则层
/// (conventions §1.4)写「提交前跑 fmt/clippy」已被自举漏掉三次(D-264 复现 +
/// 2026-08-12 第三次复发),第三次复发才确认必须代码强制。命令与 CI(ci.yml)
/// 和发版门禁(scripts/verify.ps1)完全同参数,任何一处增删门禁都要同步——
/// 守护测试 stage_fmt_clippy_gates_align_with_ci 比对三处清单。
pub(crate) async fn fmt_gate(cwd: &Path) -> Result<(), String> {
    if !cwd.join("Cargo.toml").is_file() {
        return Ok(());
    }
    let mut command = tokio::process::Command::new("cargo");
    command
        .args(["fmt", "--all", "--", "--check"])
        .current_dir(cwd);
    crate::hide_console_async(&mut command);
    let output = command.output().await;
    match output {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            // rustfmt 的 diff 清单走 stdout(Windows 上尤其),stderr 可能只有
            // "Diff in ..." 的行首;两路都读,避免漏掉违规文件清单。
            let combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            // rustfmt --check 的违规清单长这样:`Diff in crates/foo/src/lib.rs at line 4:`
            let files: Vec<String> = combined
                .lines()
                .filter(|l| l.starts_with("Diff in "))
                .map(|l| {
                    l.strip_prefix("Diff in ")
                        .and_then(|s| s.split(" at line ").next())
                        .unwrap_or(l)
                        .to_string()
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let target = if files.is_empty() {
                "cargo fmt --all -- --check".to_string()
            } else {
                files.join(", ")
            };
            Err(format!(
                "提交被拦下:`cargo fmt --all -- --check` 不过,以下文件格式未归一。\
                 \n{target}\n先跑 `cargo fmt --all` 再提交(D-264)。"
            ))
        }
        Err(error) => Err(format!(
            "提交被拦下:无法执行 cargo fmt({error})。装好 cargo/rustfmt 或在非 Rust 仓库里提交。"
        )),
    }
}

/// 提交前的编译 + lint 硬门禁(D-264)。两条命令串起来跑,比原先单条
/// `cargo clippy --workspace --all-targets -- -D warnings` 便宜得多,而底线一分没丢:
///
///   1. `cargo check --workspace --all-targets --quiet`(compile_gate)——**编译底线**,
///      含测试代码。这条不可省:见 compile_gate 注释里 2026-08-09 的事故(四处破损
///      签名配着自写 passed 记录进库),编译必须由工具亲自验。
///   2. `cargo clippy --workspace -- -D warnings`——lint,**不含**测试目标。
///
/// 实测(碰 kanzei-harness/src/lib.rs 后,rust-lld 链接):
///   原 `clippy --all-targets` 37.9s  vs  `check --all-targets` 7.2s + `clippy` 5.0s = 12.2s
/// 省 25.7s。丢掉的只有**测试代码的 lint**,那份覆盖由 CI 每次 push 跑的
/// `cargo clippy --workspace --all-targets` 兜住(ci.yml)。
///
/// 这是刻意的三处分工——提交门禁(此处)与 verify.ps1 走轻量 lint,CI 走全量——
/// 由守护测试 gate_checklists_align_across_git_verify_and_ci 显式断言,不是漂移。
/// 代价明写:测试代码的 lint 违规会本地绿、push 后 CI 红。
///
/// 2026-08-11 实例(为什么 lint 仍必须全 workspace,不能退成 `-p <改动 crate>`):
/// 新增集成测试落在 crates/kanzei/tests/,自举只跑了「改动最多的 crate」的定向测试,
/// 6 条 lint 红灯随提交进库。所以这里保持 `--workspace`,只是不再 `--all-targets`。
pub(crate) async fn clippy_gate(cwd: &Path) -> Result<(), String> {
    if !cwd.join("Cargo.toml").is_file() {
        return Ok(());
    }
    // 编译底线先跑:编译不过时,报带 `-->` 的编译错误远比报 lint 有用,
    // 也省下在编译不了的代码上再跑一遍 lint 分析的时间。
    compile_gate(cwd).await?;

    let mut command = tokio::process::Command::new("cargo");
    command
        .args(["clippy", "--workspace", "--", "-D", "warnings"])
        .current_dir(cwd);
    crate::hide_console_async(&mut command);
    let output = command.output().await;
    match output {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let head: Vec<&str> = stderr
                .lines()
                .filter(|l| l.starts_with("error") || l.trim_start().starts_with("-->"))
                .take(16)
                .collect();
            Err(format!(
                "提交被拦下:`cargo clippy --workspace -- -D warnings` 不过。\n{}",
                head.join("\n")
            ))
        }
        Err(error) => Err(format!(
            "提交被拦下:无法执行 cargo clippy({error})。装好 cargo/clippy 或在非 Rust 仓库里提交。"
        )),
    }
}

/// 源码提交的硬门禁:必须存在**改完之后**才收尾的 passed 测试记录。
///
/// 这条纪律此前只写在提示词里,实测一天里被绕过三次(R-158 顶掉 reasoning effort、
/// 批4/5 让 HEAD 编译不过、批6 漏 use Path),每次都是"跑了 cargo check 就提交"。
/// 判据放在工具层,提示词说什么都绕不过去。
pub(crate) fn source_test_gate(
    project_root: &Path,
    cwd: &Path,
    paths: &[String],
) -> Result<(), String> {
    let sources: Vec<&String> = paths.iter().filter(|p| is_source_path(p)).collect();
    if sources.is_empty() {
        return Ok(());
    }
    // 删除的文件取不到 mtime,跳过;全是删除时没有可比的时间点,放行。
    let Some(newest_change) = sources
        .iter()
        .filter_map(|p| modified_secs(&cwd.join(p)))
        .max()
    else {
        return Ok(());
    };
    let listed = sources
        .iter()
        .take(5)
        .map(|p| format!("  - {p}"))
        .collect::<Vec<_>>()
        .join("\n");
    let remedy = format!(
        "本次暂存的源码:\n{listed}{}\n\
         做法:跑 `cargo test --workspace`(或本次改动的定向 `cargo test -p <crate>`),\
         再用 test_record 记一条 status=passed(带上命令与摘要),然后重新 commit。\
         cargo check 不算——它编译不了测试目标,R-158 那处被顶掉的 reasoning effort 就是这么漏过去的。",
        if sources.len() > 5 { format!("\n  - …还有 {} 个文件", sources.len() - 5) } else { String::new() }
    );
    match crate::test_record::last_passed(project_root) {
        None => Err(format!("提交被拦下:没有任何 passed 的测试记录。\n{remedy}")),
        Some((passed_at, coverage, command_text, fingerprint)) => {
            // D-332 验收④:优先比源码指纹——测试记录背书的是「收尾那一刻的暂存源码」。
            // 指纹非空且与当前暂存源码一致 = 背书成立(即使 mtime 因 test_record 自己写
            // tests.md 而变新,源码没变就不要求重测)。指纹为空(旧记录/非 git)时退回 mtime。
            let current_fingerprint = staged_source_fingerprint(cwd).unwrap_or_default();
            if !fingerprint.is_empty() && !current_fingerprint.is_empty() {
                if fingerprint != current_fingerprint {
                    return Err(format!(
                        "提交被拦下:最近一条 passed 测试记录背书的源码指纹与当前暂存源码不一致\
                         (记录: {fingerprint}, 当前: {current_fingerprint})——fmt/改动后源码变了,\
                         这条记录背书的不是要提交的这份代码。\n{remedy}"
                    ));
                }
            } else if passed_at < newest_change {
                return Err(format!(
                    "提交被拦下:最近一条 passed 测试记录收尾于 {} 秒前,而暂存的源码在那之后又改过\
                     ({} 秒前)——这条记录背书的不是要提交的这份代码。\n{remedy}",
                    now_secs().saturating_sub(passed_at),
                    now_secs().saturating_sub(newest_change)
                ));
            }
            // R-212:相关性——暂存源码所属 crate 必须被最近 passed 记录覆盖。
            // 只按时间戳背书(改完没重跑)已经防不住「跑了 A 测试以为覆盖了 B」
            // 的诚实失误:前端冒烟记录的时间戳比 Rust 改动新,却背不了这份源码。
            let staged = source_crates(paths);
            let missing: Vec<String> = staged
                .iter()
                .filter(|c| !coverage.covers(c))
                .cloned()
                .collect();
            if missing.is_empty() {
                return Ok(());
            }
            let run_hint = missing
                .iter()
                .map(|c| format!("cargo test -p {c}"))
                .collect::<Vec<_>>()
                .join("、");
            Err(format!(
                "提交被拦下:最近一条 passed 测试记录(命令: {command_text})的覆盖面是 {}——\
                 不覆盖本次暂存源码所属 crate:{}。\n\
                 做法:跑 `{run_hint}`(或 `cargo test --workspace`),再用 test_record 记一条 \
                 status=passed(带上命令与摘要),然后重新 commit。cargo check 不算——它编译不了\
                 测试目标,R-158 那处被顶掉的 reasoning effort 就是这么漏过去的。",
                coverage.describe(),
                missing.join(", "),
            ))
        }
    }
}

/// 暂存源码所属 crate 集合(路径 `crates/<name>/...` → <name>;scripts/ 等不在 crate 内)。
fn source_crates(paths: &[String]) -> std::collections::BTreeSet<String> {
    paths
        .iter()
        .map(|p| p.replace('\\', "/"))
        .filter(|p| p.starts_with("crates/"))
        .filter_map(|p| {
            let mut parts = p.split('/');
            let _ = parts.next(); // "crates"
            parts.next().map(|name| name.to_string())
        })
        .collect()
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// D-334:finalize 事务化——把「fmt → 相关测试 → test_record → stage → CAS commit」
/// 收敛为一次机械调用,Agent 不再手动驾驶 Harness 状态机。
///
/// 内部顺序(任一失败立即返回该阶段,不留半状态):
///   1. `fmt_gate`(提交前代码门禁,D-264):fmt 不过先拦,省得 Agent 先 test 再被拦;
///   2. `clippy_gate`:与 commit 一致的全仓 clippy 硬门禁;
///   3. 按暂存源码推导相关 crate,构造定向测试命令(无 crate 改动时退化为
///      `cargo test --workspace` 由调用方显式指定 —— 见参数说明);
///   4. 跑测试(超时 10 分钟防挂死),失败返回测试输出;
///   5. `test_record::record_test_run_with_duration` 记 passed(带 source_fingerprint,
///      与 staged_source_fingerprint 一致,背书「这份源码测过」);
///   6. `stage`(显式文件,与既有 stage 同语义);
///   7. CAS commit(消费 stage 的 staged_hash,与既有 commit 同语义)。
///
/// 与手工「test→record→stage→commit」的差异:fmt/clippy 在测试**之前**拦,且全部
/// 在一个调用内完成——Agent 只发一次 finalize,不手动编排每一步。
pub(crate) async fn finalize(
    ctx: &ToolCtx,
    files: Vec<String>,
    message: Option<String>,
) -> ToolOutput {
    let cwd = &ctx.cwd;
    let message = message.unwrap_or_default();
    if message.trim().is_empty() {
        return ToolOutput::error("`message` is required for finalize");
    }
    if files.is_empty() {
        return ToolOutput::error(
            "`files` is required for finalize: explicitly list the files to commit",
        );
    }
    let sources: Vec<String> = files
        .iter()
        .filter(|p| p.ends_with(".rs") || p.ends_with("Cargo.toml"))
        .cloned()
        .collect();

    // 1. fmt gate(先于测试——D-334 核心:别再「测完了才发现 fmt 没过」)。
    // R-261:fmt 与 clippy 互不依赖,并行执行,与 commit 门禁同一节奏。
    if !sources.is_empty() {
        let (fmt_result, clippy_result) = tokio::join!(fmt_gate(cwd), clippy_gate(cwd));
        if let Err(error) = fmt_result {
            return ToolOutput::error(format!(
                "[finalize] fmt gate failed (before tests):\n{error}"
            ));
        }
        if let Err(error) = clippy_result {
            return ToolOutput::error(format!(
                "[finalize] clippy gate failed (before tests):\n{error}"
            ));
        }
    }

    // 2. 相关测试命令:暂存源码所属 crate 集合;无 crate 改动时退化为 workspace。
    let staged_crates = source_crates(&sources);
    let test_command = if staged_crates.is_empty() {
        "cargo test --workspace".to_string()
    } else {
        staged_crates
            .iter()
            .map(|c| format!("cargo test -p {c}"))
            .collect::<Vec<_>>()
            .join(" && ")
    };

    // 3. 跑测试(超时 10 分钟;失败返回测试输出,不 stage 不 commit)。
    let started = std::time::Instant::now();
    let mut command = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(&test_command);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(&test_command);
        c
    };
    command.current_dir(cwd);
    crate::hide_console_async(&mut command);
    let output =
        match tokio::time::timeout(std::time::Duration::from_secs(600), command.output()).await {
            Ok(out) => match out {
                Ok(output) => output,
                Err(error) => {
                    return ToolOutput::error(format!(
                        "[finalize] failed to run `{test_command}`: {error}"
                    ))
                }
            },
            Err(_) => {
                return ToolOutput::error(format!(
                    "[finalize] tests timed out after 600s: `{test_command}`"
                ))
            }
        };
    let duration_secs = started.elapsed().as_secs_f64();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = stderr
            .lines()
            .filter(|l| l.contains("test result") || l.starts_with("error"))
            .take(12)
            .collect();
        return ToolOutput::error(format!(
            "[finalize] tests failed: `{test_command}`\n{}",
            tail.join("\n")
        ));
    }
    let passed_summary = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| l.contains("test result: ok"))
        .map(str::to_string)
        .collect::<Vec<_>>()
        .join("; ");

    // 4. test_record:记 passed,带 source_fingerprint(与暂存源码一致)与时长。
    let fingerprint = staged_source_fingerprint(cwd).unwrap_or_default();
    let summary = if passed_summary.is_empty() {
        "finalize 测试通过(无 test result 行,或纯非 Rust 改动)".to_string()
    } else {
        passed_summary
    };
    if let Err(error) = crate::test_record::record_test_run_with_duration(
        &ctx.project_root,
        None,
        &format!("git finalize (auto): {test_command}"),
        "passed",
        Some(&test_command),
        Some(&summary),
        None,
        Some(duration_secs),
        Some(&fingerprint),
    ) {
        return ToolOutput::error(format!("[finalize] test_record failed: {error}"));
    }

    // 5. stage(显式文件,与既有 stage 同语义)。
    let staged = stage(cwd, &files).await;
    let ToolOutput {
        content,
        is_error,
        display,
        ..
    } = staged;
    if is_error {
        return ToolOutput::error(format!("[finalize] stage failed:\n{content}"));
    }
    // stage 返回里解析 staged_hash(格式固定:含 `staged_hash: <hash>` 行)。
    let Some(hash_line) = content.lines().find(|l| l.contains("staged_hash:")) else {
        return ToolOutput::error(format!(
            "[finalize] stage succeeded but staged_hash not found in output:\n{content}"
        ));
    };
    let staged_hash = hash_line
        .split("staged_hash:")
        .nth(1)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if staged_hash.is_empty() {
        return ToolOutput::error("[finalize] staged_hash empty after stage");
    }

    // 6. CAS commit(消费 staged_hash)。
    let committed = commit(ctx, Some(message), Some(staged_hash.clone())).await;
    if committed.is_error {
        return ToolOutput::error(format!(
            "[finalize] commit failed after successful stage+test (staged_hash {staged_hash}):\n{}",
            committed.content
        ));
    }
    ToolOutput::ok(format!(
        "[finalize] complete: {test_command} passed in {duration_secs:.1}s → staged {staged_hash} → committed\n{content}\n{}",
        committed.content
    ))
    .with_display(display.unwrap_or_else(|| serde_json::json!({ "kind": "terminal" })))
}

/// 引用名校验:只放行分支/标签的常规形态。拒绝 `-` 开头(选项注入)、区间语法
/// (`..`)、修订运算符(`~`/`^`/`:`)与空白——merge_ff 只该拿到一个干净的名字。
pub(crate) fn validate_ref(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("引用名不能为空".into());
    }
    if name.starts_with('-')
        || name.contains("..")
        || name
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '~' | '^' | ':' | '\\' | '*' | '?' | '['))
    {
        return Err(format!(
            "非法引用名 `{name}`:merge_ff 只接受干净的分支/标签名"
        ));
    }
    Ok(name.to_string())
}
