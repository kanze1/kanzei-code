//! bash(shell)工具。设计红线 6:按实际检测到的 shell 动态生成描述,
//! 让模型知道自己面对的是 pwsh/cmd 还是 POSIX sh;超时返回结构化结果而非报错。

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::io::AsyncReadExt;

use crate::shell::{detected_shell, kill_tree};

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 600_000;
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

/// 托管目录:write/edit 对它们硬 deny,shell 也不许绕过去改(D-173)。
const MANAGED_ROOTS: &[&str] = &[".kanzei/project", ".kanzei/memory"];
/// 单文件镜像上限:超过就只记指纹,能检测但无法回滚(会如实说明)。
const MANAGED_SNAPSHOT_FILE_LIMIT: u64 = 4 * 1024 * 1024;
/// 镜像文件数上限,防止有人往托管目录塞进一整棵大树把每次 bash 拖垮。
const MANAGED_SNAPSHOT_MAX_FILES: usize = 2000;

#[derive(Deserialize, JsonSchema)]
struct BashInput {
    /// 要执行的命令
    #[serde(alias = "cmd", alias = "script")]
    command: String,
    /// 超时毫秒(默认 120000,上限 600000)
    #[serde(default)]
    timeout_ms: Option<u64>,
    /// 工作目录(默认 cwd)
    #[serde(default)]
    workdir: Option<String>,
    /// 后台运行:立刻返回进程句柄,用 process 工具查输出/停止(长驻服务、watch 用)
    #[serde(default)]
    background: bool,
}

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> String {
        let shell = detected_shell();
        let syntax = match shell.name {
            "pwsh" | "powershell" => {
                "PowerShell syntax (NOT POSIX: use ; or && (pwsh7), $env:VAR, Get-ChildItem)"
            }
            "cmd" => "cmd.exe syntax (NOT POSIX: use %VAR%, dir, &&)",
            _ => "POSIX sh syntax",
        };
        format!(
            "Run a shell command via {} — {syntax}. Params: command; optional timeout_ms, workdir, \
             background. stdin is closed: interactive prompts get EOF instead of hanging. \
             Set background=true for long-running processes (dev server, watch): it returns a \
             process id immediately; use the `process` tool to read output, check liveness or stop it.",
            shell.name
        )
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(BashInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        let command = input["command"].as_str().unwrap_or("*");
        let Some(workdir) = input["workdir"].as_str().filter(|dir| !dir.is_empty()) else {
            return vec![command.to_string()];
        };
        vec![serde_json::json!({
            "command": command,
            "workdir": kanzei_harness::permission::normalize_resource(workdir),
        })
        .to_string()]
    }

    fn resources_with_ctx(&self, input: &serde_json::Value, ctx: &ToolCtx) -> Vec<String> {
        let command = input["command"].as_str().unwrap_or("*");
        let workdir = input["workdir"].as_str().filter(|dir| !dir.is_empty());
        let effective_workdir = ctx.cwd.join(
            workdir
                .map(kanzei_harness::permission::normalize_resource)
                .unwrap_or_else(|| ".".into()),
        );
        vec![serde_json::json!({
            "command": command,
            "workdir": kanzei_harness::permission::normalize_resource(
                &effective_workdir.display().to_string(),
            ),
        })
        .to_string()]
    }

    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        // Shell 命令可产生任意副作用，不能靠解析命令文本猜测“只读”。
        ToolConcurrency::write_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: BashInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        // D-113 硬门禁:整文件覆写 cmdlet 绕过 edit/write 的语法校验与 diff 展示,一律拦截。
        if let Some(cmdlet) = full_file_write_cmdlet(&input.command) {
            return ToolOutput::error(format!(
                "`{cmdlet}` is blocked: whole-file rewrites via shell bypass the edit/write \
                 tools' syntax validation and diff display. Use `edit` for targeted changes \
                 (it tolerates line-ending differences and, after two misses, shows you the \
                 file's actual content) or `write` to create/replace a file deliberately."
            ));
        }
        let timeout = Duration::from_millis(
            input
                .timeout_ms
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .min(MAX_TIMEOUT_MS),
        );
        let workdir = match &input.workdir {
            Some(dir) => ctx
                .cwd
                .join(kanzei_harness::permission::normalize_resource(dir)),
            None => ctx.cwd.clone(),
        };
        if !workdir.is_dir() {
            return ToolOutput::error(format!("workdir does not exist: {}", workdir.display()));
        }

        // Git 写操作必须走结构化 git 工具。只拦截写子命令，status/diff/log/show 等读操作
        // 仍可在 shell 中执行；这样权限和暂存区 CAS 不会被 `git add/commit` 文本旁路。
        if let Some(form) = git_mutation_form(&input.command) {
            return ToolOutput::error(format!(
                "`{form}` is blocked in bash: Git mutations must use the structured `git` tool. \
                 Use `git stage` with explicit files, review its staged_hash/diff, then `git commit` \
                 with that hash. Fast-forward merges go through `git merge_ff` (from/into; it finds \
                 the worktree where `into` is checked out). Other branch/index mutations not covered \
                 by that tool require the user to run them directly; do not route them through \
                 another shell spelling."
            ));
        }

        // D-173 硬围栏:托管文档只能走专用工具,而"能不能绕过"绝不能靠猜命令文本。
        // [System.IO.File]::WriteAllText、重定向、python/node 一行流、git checkout 单文件
        // 都能避开任何字符串匹配,所以这里改成**结果侧**判定:跑之前拍下托管目录的镜像,
        // 跑完再比一次。改了就先把改后的版本隔离留证,再整体回滚,并按错误回喂模型。
        let managed_before = ManagedSnapshot::capture(&ctx.project_root);
        if !managed_before.is_complete() {
            return ToolOutput::error(format!(
                "bash refused before execution: the managed-document snapshot is incomplete \
                 (more than {MANAGED_SNAPSHOT_MAX_FILES} files or a file over \
                 {MANAGED_SNAPSHOT_FILE_LIMIT} bytes). A shell command cannot run when its \
                 protected-path effects cannot be fully rolled back."
            ));
        }
        // 后台 shell 在本次工具返回后仍可写盘，无法把副作用可靠归因于某次调用；
        // 事后轮询还会把用户/专用工具的合法并发写误判为越权。安全隔离落地前明确禁用。
        if input.background && managed_scope_exists(&ctx.project_root) {
            return ToolOutput::error(
                "background bash is unavailable in a managed project: asynchronous writes cannot \
                 be fenced without misclassifying later user/dedicated-tool edits. Run the command \
                 in the foreground, or ask the user to start the long-lived service directly.",
            );
        }

        let shell = detected_shell();
        let mut command = tokio::process::Command::new(&shell.program);
        command
            .args(&shell.args)
            .arg(&input.command)
            .current_dir(&workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        hide_console_window(&mut command);

        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(format!("failed to spawn {}: {e}", shell.name)),
        };

        // 后台模式:交给注册表托管,立刻返回句柄,不等它结束也不受 timeout 约束。
        if input.background {
            let process = crate::background::register(
                child,
                input.command.clone(),
                &ctx.project_root,
                &workdir,
            );
            let rendered = format!(
                "background: true\nprocess_id: {}\npid: {}\ncommand: {}\n\
                 Use the `process` tool: {{\"action\":\"output\",\"id\":\"{}\"}} to read output, \
                 {{\"action\":\"stop\",\"id\":\"{}\"}} to terminate.",
                process.id,
                process.pid().map_or("unknown".into(), |p| p.to_string()),
                input.command,
                process.id,
                process.id,
            );
            return ToolOutput::ok(rendered).with_display(serde_json::json!({
                "kind": "terminal",
                "command": input.command,
                "background": true,
                "processId": process.id,
                "output": "(后台运行中,用 process 工具查看输出)",
            }));
        }

        let pid = child.id();

        let mut stdout = child.stdout.take().expect("stdout piped");
        let mut stderr = child.stderr.take().expect("stderr piped");
        // 缓冲放在 future 外:超时把整个 future drop 掉时,已经读到的输出必须还在,
        // 否则模型对"卡在哪一步"一无所知,只能盲目加大 timeout 重跑并重复副作用(D-062)。
        let (mut out_buf, mut err_buf) = (Vec::new(), Vec::new());
        let capture = {
            let out_buf = &mut out_buf;
            let err_buf = &mut err_buf;
            async move {
                // 有界读取:两条流各自最多 MAX_CAPTURE_BYTES,超出丢弃(内存红线)。
                let (a, b) = tokio::join!(
                    read_capped(&mut stdout, out_buf),
                    read_capped(&mut stderr, err_buf)
                );
                let status = child.wait().await;
                (status, a, b)
            }
        };

        let outcome = tokio::time::timeout(timeout, capture).await;
        match outcome {
            Ok((status, out_capped, err_capped)) => {
                let mut text = String::from_utf8_lossy(&out_buf).into_owned();
                if out_capped {
                    text.push_str("\n[stdout truncated at 1 MiB]");
                }
                if !err_buf.is_empty() {
                    text.push_str("\n[stderr]\n");
                    text.push_str(&String::from_utf8_lossy(&err_buf));
                    if err_capped {
                        text.push_str("\n[stderr truncated at 1 MiB]");
                    }
                }
                let code = status.as_ref().ok().and_then(|s| s.code());
                let ok = code == Some(0);
                let text = if text.trim().is_empty() {
                    "(no output)".to_string()
                } else {
                    text
                };
                let mut rendered = format!(
                    "exit code: {}\n{text}",
                    code.map_or("unknown".into(), |c| c.to_string())
                );
                let breach = enforce_managed_files(&ctx.project_root, managed_before);
                if let Some(report) = &breach {
                    rendered.push('\n');
                    rendered.push_str(report);
                }
                let display = serde_json::json!({
                    "kind": "terminal",
                    "command": input.command,
                    "exitCode": code,
                    "output": text.chars().take(4000).collect::<String>(),
                    // D-237:活动面板要能看到 bash 的"实际内容",4000 截断对长输出
                    // (cargo test 等)直接丢后半段。完整输出随 display 透传,
                    // 前端 detail 展开区消费;上限 200k 防事件体被单条输出打爆。
                    "full": text.chars().take(200_000).collect::<String>(),
                });
                let output = if ok && breach.is_none() {
                    ToolOutput::ok(rendered)
                } else {
                    ToolOutput::error(rendered)
                };
                output.with_display(display)
            }
            Err(_) => {
                if let Some(pid) = pid {
                    kill_tree(pid).await;
                }
                // 超时是可预期结果:结构化告知,并回传已捕获的输出(卡在哪一步全靠它)。
                let mut text = format!(
                    "timeout: true — command did not finish within {} ms and was killed. Retry with a larger timeout_ms if needed.",
                    timeout.as_millis()
                );
                let partial_out = String::from_utf8_lossy(&out_buf).into_owned();
                let partial_err = String::from_utf8_lossy(&err_buf).into_owned();
                if partial_out.trim().is_empty() && partial_err.trim().is_empty() {
                    text.push_str("\n[no output captured before timeout]");
                } else {
                    if !partial_out.trim().is_empty() {
                        text.push_str("\n[partial stdout before timeout]\n");
                        text.push_str(&partial_out);
                    }
                    if !partial_err.trim().is_empty() {
                        text.push_str("\n[partial stderr before timeout]\n");
                        text.push_str(&partial_err);
                    }
                }
                // 被杀掉的命令一样可能已经改过托管文件,围栏必须照跑。
                if let Some(report) = enforce_managed_files(&ctx.project_root, managed_before) {
                    text.push('\n');
                    text.push_str(&report);
                }
                let display = serde_json::json!({
                    "kind": "terminal",
                    "command": input.command,
                    "exitCode": serde_json::Value::Null,
                    "timeout": true,
                    "output": text.chars().take(4000).collect::<String>(),
                    "full": text.chars().take(200_000).collect::<String>(),
                });
                // 超时不是成功:按错误返回,上层不再把它计入正常完成。
                ToolOutput::error(text).with_display(display)
            }
        }
    }
}

/// 命令中出现整文件覆写 cmdlet 时返回其名称(词边界匹配,Get-Content 不误伤)。
fn full_file_write_cmdlet(command: &str) -> Option<&'static str> {
    let lower = command.to_ascii_lowercase();
    for (needle, name) in [("set-content", "Set-Content"), ("out-file", "Out-File")] {
        let mut search_from = 0;
        while let Some(pos) = lower[search_from..].find(needle) {
            let absolute = search_from + pos;
            let bounded_left = absolute == 0
                || !matches!(lower.as_bytes()[absolute - 1], b'a'..=b'z' | b'0'..=b'9' | b'-');
            let after = absolute + needle.len();
            let bounded_right = after >= lower.len()
                || !matches!(lower.as_bytes()[after], b'a'..=b'z' | b'0'..=b'9' | b'-');
            if bounded_left && bounded_right {
                return Some(name);
            }
            search_from = after;
        }
    }
    None
}

/// shell 中的 Git 写子命令。读命令仍放行；写命令统一走结构化 `git` 工具。
fn git_mutation_form(command: &str) -> Option<String> {
    for segment in command.split([';', '\n', '|', '&']) {
        let tokens: Vec<&str> = segment.split_whitespace().collect();
        let Some(git_at) = tokens.iter().position(|t| {
            let t = t.trim_matches(['"', '\'']);
            t.eq_ignore_ascii_case("git") || t.to_ascii_lowercase().ends_with("/git")
        }) else {
            continue;
        };
        let rest = &tokens[git_at + 1..];
        // 跳过 `-C <dir>` / `-c k=v` 之类的全局开关,找到真正的子命令。
        let mut index = 0usize;
        while index < rest.len() && rest[index].starts_with('-') {
            index += if matches!(rest[index], "-C" | "-c" | "--git-dir" | "--work-tree") {
                2
            } else {
                1
            };
        }
        let Some(subcommand) = rest.get(index).map(|s| s.to_ascii_lowercase()) else {
            continue;
        };
        if matches!(
            subcommand.as_str(),
            "add"
                | "stage"
                | "commit"
                | "checkout"
                | "switch"
                | "reset"
                | "restore"
                | "merge"
                | "rebase"
                | "pull"
                | "cherry-pick"
                | "revert"
                | "clean"
                | "rm"
                | "mv"
        ) {
            return Some(format!("git {subcommand}"));
        }
    }
    None
}

/// 托管目录的执行前镜像。`None` 内容 = 文件超过镜像上限,只能检测不能回滚。
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ManagedSnapshot {
    files: BTreeMap<String, Option<Vec<u8>>>,
    /// 目录规模超限时放弃镜像:此时既不检测也不回滚,并在输出里如实说明。
    truncated: bool,
}

impl ManagedSnapshot {
    pub(crate) fn capture(project_root: &Path) -> Self {
        let mut snapshot = ManagedSnapshot::default();
        for root in MANAGED_ROOTS {
            collect_files(&project_root.join(root), project_root, &mut snapshot);
            if snapshot.truncated {
                break;
            }
        }
        snapshot
    }

    fn is_complete(&self) -> bool {
        !self.truncated && self.files.values().all(Option::is_some)
    }
}

fn managed_scope_exists(project_root: &Path) -> bool {
    project_root.join(".kanzei").is_dir()
}

fn collect_files(dir: &Path, project_root: &Path, snapshot: &mut ManagedSnapshot) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if snapshot.files.len() >= MANAGED_SNAPSHOT_MAX_FILES {
            snapshot.truncated = true;
            return;
        }
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => collect_files(&path, project_root, snapshot),
            Ok(kind) if kind.is_file() => {
                let Ok(relative) = path.strip_prefix(project_root) else {
                    continue;
                };
                let key = relative.display().to_string().replace('\\', "/");
                let size = entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
                let content = (size <= MANAGED_SNAPSHOT_FILE_LIMIT)
                    .then(|| std::fs::read(&path).ok())
                    .flatten();
                snapshot.files.insert(key, content);
            }
            _ => {}
        }
    }
}

/// 比对执行前后的托管目录镜像。有改动就隔离留证 + 整体回滚,并生成回喂模型的报告。
fn enforce_managed_files(project_root: &Path, before: ManagedSnapshot) -> Option<String> {
    let after = ManagedSnapshot::capture(project_root);
    if after == before {
        return None;
    }
    let after_incomplete = !after.is_complete();
    let mut modified: Vec<String> = Vec::new();
    let mut created: Vec<String> = Vec::new();
    let mut deleted: Vec<String> = Vec::new();
    for (path, content) in &after.files {
        match before.files.get(path) {
            None => created.push(path.clone()),
            Some(old) if old != content => modified.push(path.clone()),
            Some(_) => {}
        }
    }
    for path in before.files.keys() {
        if !after.files.contains_key(path) {
            deleted.push(path.clone());
        }
    }
    if modified.is_empty() && created.is_empty() && deleted.is_empty() {
        return None;
    }

    let touched: Vec<&String> = modified
        .iter()
        .chain(created.iter())
        .chain(deleted.iter())
        .collect();
    let listed = touched
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    // 先隔离再回滚:哪怕这次改动其实来自用户手改,内容也一份不丢,可原样取回。
    let quarantine = project_root.join(".kanzei/quarantine").join(format!(
        "shell-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default()
    ));
    let mut restored = 0usize;
    for path in modified.iter().chain(created.iter()) {
        let absolute = project_root.join(path);
        let saved = quarantine.join(path);
        if let Some(parent) = saved.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::copy(&absolute, &saved);
        match before.files.get(path) {
            // 执行前存在:写回原内容(内容超限没镜像的只能保持现状,下面单独点名)。
            Some(Some(original)) => {
                if std::fs::write(&absolute, original).is_ok() {
                    restored += 1;
                }
            }
            Some(None) => {}
            // 执行前不存在:删掉新建的。
            None => {
                if std::fs::remove_file(&absolute).is_ok() {
                    restored += 1;
                }
            }
        }
    }
    for path in &deleted {
        if let Some(Some(original)) = before.files.get(path) {
            let absolute = project_root.join(path);
            if let Some(parent) = absolute.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&absolute, original).is_ok() {
                restored += 1;
            }
        }
    }

    let incomplete = if after_incomplete {
        format!(
            "\nWARNING: the post-command snapshot exceeded its safety bound. Known changes were \
             rolled back, but the managed tree must be inspected manually before any further shell \
             command (limit: {MANAGED_SNAPSHOT_MAX_FILES} files / {MANAGED_SNAPSHOT_FILE_LIMIT} bytes per file)."
        )
    } else {
        String::new()
    };
    Some(format!(
        "[managed-files] BLOCKED AND ROLLED BACK. This command modified files under {} — those \
         paths are policy-managed and the shell is not a write channel for them, no matter which \
         mechanism is used (redirect, Set-Content/Out-File, [System.IO.File]::WriteAllText, \
         python/node one-liner, git checkout of a single file).\n\
         touched: {listed}\n\
         restored {restored} file(s) to their pre-command contents; your versions were kept at \
         {} so nothing is lost.\n\
         Redo the change through the dedicated tool (`req`/`defect`/`goal`/`decision` for tracker \
         entries, `architecture` for the architecture index, `memory_note` for memory). If no \
         tool covers what you need, that is an unimplemented capability: record it and tell the \
         user — do not look for another shell route.{incomplete}",
        MANAGED_ROOTS.join(" and "),
        quarantine.display(),
    ))
}

#[cfg(windows)]
fn hide_console_window(command: &mut tokio::process::Command) {
    crate::hide_console_async(command);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut tokio::process::Command) {}

async fn read_capped(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    buffer: &mut Vec<u8>,
) -> bool {
    let mut chunk = [0u8; 8192];
    let mut capped = false;
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                // 进度旁路:每读到一段就上报(runner 注入通道时才生效)。长命令
                // (装依赖/发版脚本)的输出因此能边跑边出现在活动面板,而不是
                // 结束后一次性砸出来。截断上限只约束回喂模型的缓冲,不约束进度流。
                kanzei_harness::progress::emit(&String::from_utf8_lossy(&chunk[..n]));
                if buffer.len() < MAX_CAPTURE_BYTES {
                    let take = n.min(MAX_CAPTURE_BYTES - buffer.len());
                    buffer.extend_from_slice(&chunk[..take]);
                    if take < n {
                        capped = true;
                    }
                } else {
                    capped = true;
                }
            }
            Err(_) => break,
        }
    }
    capped
}

#[cfg(test)]
mod tests {
    use super::{full_file_write_cmdlet, git_mutation_form, BashTool};
    use kanzei_harness::{Tool, ToolCtx};
    use std::path::PathBuf;

    fn temp_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-bash-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        dir
    }

    /// D-173:托管文件的 shell 写入必须被检测、隔离、回滚——不靠匹配命令文本,
    /// 所以 [System.IO.File]::WriteAllText 这类没人预料到的写法一样拦得住。
    #[tokio::test]
    async fn shell_writes_to_managed_docs_are_rolled_back() {
        let root = temp_project("fence");
        let managed = root.join(".kanzei/project/defects.md");
        std::fs::write(&managed, "# Defects\n\n## D-001 原始内容 [open]\n").unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
        };
        let target = managed.display().to_string().replace('\\', "/");
        let command = match super::detected_shell().name {
            "pwsh" | "powershell" => {
                format!("[System.IO.File]::WriteAllText('{target}', 'BYPASSED')")
            }
            "cmd" => format!("echo BYPASSED> \"{target}\""),
            _ => format!("printf BYPASSED > '{target}'"),
        };

        let out = BashTool
            .execute(serde_json::json!({ "command": command }), &ctx)
            .await;
        assert!(out.is_error, "越权写入必须按错误回喂: {}", out.content);
        assert!(out.content.contains("[managed-files]"), "{}", out.content);
        assert!(out.content.contains("defects.md"), "{}", out.content);
        assert_eq!(
            std::fs::read_to_string(&managed).unwrap(),
            "# Defects\n\n## D-001 原始内容 [open]\n",
            "文件必须被回滚到执行前内容"
        );
        // 改后的版本留在隔离区,万一那其实是用户手改也不丢。
        let quarantine = root.join(".kanzei/quarantine");
        assert!(quarantine.is_dir(), "改后的内容必须留证");

        // 非托管路径照常放行,围栏不误伤。
        let plain = root
            .join("scratch.txt")
            .display()
            .to_string()
            .replace('\\', "/");
        let command = match super::detected_shell().name {
            "pwsh" | "powershell" => format!("[System.IO.File]::WriteAllText('{plain}', 'ok')"),
            "cmd" => format!("echo ok> \"{plain}\""),
            _ => format!("printf ok > '{plain}'"),
        };
        let out = BashTool
            .execute(serde_json::json!({ "command": command }), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(root.join("scratch.txt").is_file());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn git_mutations_are_blocked_without_false_positives() {
        let blocked = [
            "git add -A",
            "git add src/main.rs",
            "git commit -m x",
            "git status --short; git add .kanzei/project",
            "git checkout other-branch",
            "git reset --hard HEAD~1",
            "git pull --ff-only",
        ];
        for command in blocked {
            assert!(git_mutation_form(command).is_some(), "应拦截: {command}");
        }
        let allowed = [
            "git log --all --oneline",
            "git diff --stat",
            "git status --short",
            "git show HEAD",
            "cargo add serde",
        ];
        for command in allowed {
            assert!(git_mutation_form(command).is_none(), "不该拦截: {command}");
        }
    }

    #[tokio::test]
    async fn empty_managed_directory_is_still_fenced() {
        let root = temp_project("empty-fence");
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
        };
        let target = root
            .join(".kanzei/project/new.md")
            .display()
            .to_string()
            .replace('\\', "/");
        let command = match super::detected_shell().name {
            "pwsh" | "powershell" => format!("[System.IO.File]::WriteAllText('{target}', 'x')"),
            "cmd" => format!("echo x> \"{target}\""),
            _ => format!("printf x > '{target}'"),
        };
        let out = BashTool
            .execute(serde_json::json!({"command": command}), &ctx)
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(!root.join(".kanzei/project/new.md").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn background_shell_is_refused_in_managed_projects() {
        let root = temp_project("background-fence");
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
        };
        let out = BashTool
            .execute(
                serde_json::json!({"command":"echo ok","background":true}),
                &ctx,
            )
            .await;
        assert!(out.is_error);
        assert!(
            out.content.contains("background bash is unavailable"),
            "{}",
            out.content
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn whole_file_write_cmdlets_are_detected_with_word_boundaries() {
        // D-113:拦截整文件覆写,但不误伤 Get-Content 等读取。
        assert_eq!(
            full_file_write_cmdlet("Set-Content -Path main.rs -Value $code"),
            Some("Set-Content")
        );
        assert_eq!(
            full_file_write_cmdlet("$lines | out-file -Encoding utf8 x.txt"),
            Some("Out-File")
        );
        assert_eq!(full_file_write_cmdlet("Get-Content main.rs"), None);
        assert_eq!(full_file_write_cmdlet("cargo test --workspace"), None);
        assert_eq!(full_file_write_cmdlet("echo reset-contentious"), None);
    }

    #[tokio::test]
    async fn set_content_command_is_blocked_before_spawn() {
        let out = BashTool
            .execute(
                serde_json::json!({"command": "Set-Content -Path x.rs -Value 'fn main(){}'"}),
                &ToolCtx {
                    cwd: std::env::temp_dir(),
                    project_root: std::env::temp_dir(),
                },
            )
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("edit"), "{}", out.content);
    }

    #[tokio::test]
    async fn timeout_kills_command_and_returns_explicit_error() {
        let command = match super::detected_shell().name {
            "pwsh" | "powershell" => "Start-Sleep -Seconds 5",
            "cmd" => "ping 127.0.0.1 -n 6 > nul",
            _ => "sleep 5",
        };
        let started = std::time::Instant::now();
        let out = BashTool
            .execute(
                serde_json::json!({"command": command, "timeout_ms": 50}),
                &ToolCtx {
                    cwd: std::env::temp_dir(),
                    project_root: std::env::temp_dir(),
                },
            )
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("timeout: true"), "{}", out.content);
        assert!(started.elapsed() < std::time::Duration::from_secs(4));
    }

    #[test]
    fn description_explicitly_says_interactive_stdin_is_unavailable() {
        let description = BashTool.description();
        assert!(description.contains("stdin is closed"));
        assert!(description.contains("EOF"));
    }

    #[test]
    fn resources_keep_the_complete_command_without_prefix_generalization() {
        let input = serde_json::json!({
            "command": "git status > .kanzei/project/requirements.md",
            "workdir": "subdir"
        });
        let resources = BashTool.resources(&input);
        assert_eq!(resources.len(), 1);
        let resource: serde_json::Value = serde_json::from_str(&resources[0]).unwrap();
        assert_eq!(
            resource["command"],
            "git status > .kanzei/project/requirements.md"
        );
        assert_eq!(resource["workdir"], "subdir");
        let resource: serde_json::Value = serde_json::from_str(
            &BashTool.resources_with_ctx(
                &serde_json::json!({"command": "git status", "workdir": "subdir"}),
                &ToolCtx {
                    cwd: PathBuf::from("C:/project"),
                    project_root: PathBuf::from("C:/project"),
                },
            )[0],
        )
        .unwrap();
        assert_eq!(
            resource["workdir"],
            kanzei_harness::permission::normalize_resource("C:/project/subdir")
        );
        let resource: serde_json::Value = serde_json::from_str(
            &BashTool.resources_with_ctx(
                &serde_json::json!({"command": "git status"}),
                &ToolCtx {
                    cwd: PathBuf::from("C:/project"),
                    project_root: PathBuf::from("C:/project"),
                },
            )[0],
        )
        .unwrap();
        assert_eq!(
            resource["workdir"],
            kanzei_harness::permission::normalize_resource("C:/project")
        );
    }
}
