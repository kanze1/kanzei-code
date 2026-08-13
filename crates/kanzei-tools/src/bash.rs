//! bash(shell)工具。设计红线 6:按实际检测到的 shell 动态生成描述,
//! 让模型知道自己面对的是 pwsh/cmd 还是 POSIX sh;超时返回结构化结果而非报错。

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::io::AsyncReadExt;

use crate::managed::{
    enforce_managed_files, managed_scope_exists, ManagedSnapshot, MANAGED_SNAPSHOT_FILE_LIMIT,
    MANAGED_SNAPSHOT_MAX_FILES,
};
use crate::shell::{detected_shell, kill_tree};

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 600_000;
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

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
    /// R-180:长驻档位(仅 background=true 有意义)。默认 false = 跟随 owner run
    /// (owner run 收尾即收尾,D-174 安全降级);true = 生命周期显式脱离 owner run,
    /// 跨 run 存活,由注册表/日志落盘承接(R-180 B2/B3)。只有用户或 agent 明确
    /// 声明"这是长驻服务"时才置 true——不改变默认档位。
    #[serde(default)]
    persistent: bool,
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
             process id immediately; use the `process` tool to read output, check liveness or stop it. \
             In a managed project a background task is fenced and owned by the current run: it may \
             not write under .kanzei/project or .kanzei/memory (such writes are quarantined and \
             rolled back), its workdir must stay inside the project and outside .kanzei/, and it is \
             finished when the next run starts — so it cannot outlive this turn.",
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
        // D-174 静态第一道:托管项目里的后台任务不得把工作目录扎进托管树,
        // 也不得跑到项目根之外(跑到外面就无从归因,守卫的对账范围也失去意义)。
        if input.background && managed_scope_exists(&ctx.project_root) {
            if let Some(breach) = background_workdir_breach(&ctx.cwd, &ctx.project_root, &workdir) {
                return ToolOutput::error(breach);
            }
        }
        // D-174 生命周期包含关系:上一个 run 遗留的后台任务在这里收尾。守卫把
        // "没有专用工具窗口解释的托管变化"一律判给后台进程,这个判据只有在
        // 「后台任务生命周期 ⊆ owner run」时才成立——跨 run 存活会让本 run 的
        // 专用工具写入被上一个 run 的守卫误判成越界。
        crate::background::finish_foreign_owners(&ctx.project_root, ctx.run_id.as_deref()).await;

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
            // 归因地基:owner 身份来自 ToolCtx(R-171 双键),托管基线就是本次
            // spawn 之前刚拍下的 managed_before——后台守卫的对账起点必须是
            // "进程还没跑起来"的那一刻,晚一点都会把自己的副作用算进基线。
            let process = crate::background::register(
                child,
                input.command.clone(),
                &ctx.project_root,
                &workdir,
                background_owner(ctx),
                managed_before,
                // R-180:长驻档位透传。persistent=true 时 owner run 收尾不再收它。
                input.persistent,
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

/// 后台任务的 workdir 结构化围栏(D-174 静态第一道)。
///
/// 它校验的是**参数**而不是命令文本——不存在解析歧义,与 D-173 已经证否的
/// "猜命令文本"是两回事(命令文本层面的路径匹配挡不住 WriteAllText/重定向/
/// 解释器一行流,那条路本文件顶部已经放弃)。因此它只是第一道:挡掉
/// "cd 进托管目录再用相对路径写"的一整类,绝对路径写入仍由后台守卫的
/// 结果侧对账兜住。
///
/// # 包含根是**代码树**,不是主根(R-177 内容②)
///
/// D-174 的两条语义原样保留:①不得跑出可归因范围;②不得扎进托管树。变的只是
/// 「可归因范围」的正名——线上线后 agent 的代码树是 worktree,主根只承担
/// `.kanzei/**`。仍按主根做包含判定的话,线里每一条 `background: true` 的 bash
/// 都会被无条件拒(worktree 不在主根之下)。
///
/// 托管树的排除同时查两处:代码树自己的 `.kanzei`(worktree 里那份是 git checkout
/// 出来的分支副本,一样不许当工作目录)与主根的 `.kanzei`(真正的托管资产)。
fn background_workdir_breach(
    code_root: &Path,
    project_root: &Path,
    workdir: &Path,
) -> Option<String> {
    let (root, dir) = match (
        std::fs::canonicalize(code_root),
        std::fs::canonicalize(workdir),
    ) {
        (Ok(root), Ok(dir)) => (root, dir),
        // 任一侧无法规范化时两侧都用原样路径比,避免单边规范化造成假阳性拒绝。
        _ => (code_root.to_path_buf(), workdir.to_path_buf()),
    };
    let Ok(relative) = dir.strip_prefix(&root) else {
        return Some(format!(
            "background workdir must stay inside the code tree: {} is outside {}",
            workdir.display(),
            code_root.display()
        ));
    };
    let relative = relative.display().to_string().replace('\\', "/");
    if relative == ".kanzei" || relative.starts_with(".kanzei/") {
        return Some(format!(
            "background workdir must not sit inside `.kanzei/`: {}",
            workdir.display()
        ));
    }
    if in_managed_dir(project_root, workdir) {
        return Some(format!(
            "background workdir must not sit inside `.kanzei/`: {}",
            workdir.display()
        ));
    }
    None
}

/// workdir 是否落在 `root/.kanzei` 之下。与上面的相对路径判定同源,单独抽出来
/// 是为了让「代码树」与「主根」两个根各查一次。
fn in_managed_dir(root: &Path, workdir: &Path) -> bool {
    let (root, dir) = match (std::fs::canonicalize(root), std::fs::canonicalize(workdir)) {
        (Ok(root), Ok(dir)) => (root, dir),
        _ => (root.to_path_buf(), workdir.to_path_buf()),
    };
    let Ok(relative) = dir.strip_prefix(&root) else {
        return false;
    };
    let relative = relative.display().to_string().replace('\\', "/");
    relative == ".kanzei" || relative.starts_with(".kanzei/")
}

fn background_owner(ctx: &ToolCtx) -> crate::background::BackgroundOwner {
    crate::background::BackgroundOwner {
        // CLI 路径不调 with_identity,run_id 为 None:登记为 unowned 而不是伪造
        // 一个 id——归因要么真实,要么如实说不知道。
        run_id: ctx.run_id.clone().unwrap_or_else(|| "unowned".into()),
        process_id: ctx.process_id.clone().unwrap_or_else(|| "unowned".into()),
        write_key: ctx.project_write_key(),
    }
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
    use super::{full_file_write_cmdlet, git_mutation_form, in_managed_dir, BashTool};
    use crate::managed::ManagedSnapshot;
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
            ..Default::default()
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
            ..Default::default()
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

    /// D-174 静态第一道:后台 workdir 不得扎进托管树,也不得跑出项目根。
    #[tokio::test]
    async fn background_workdir_must_stay_outside_managed_tree_and_inside_project() {
        let root = temp_project("bg-workdir");
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        // 扎进 .kanzei/ 内:拒绝,且理由点名 .kanzei 而不是笼统的"后台不可用"。
        let out = BashTool
            .execute(
                serde_json::json!({
                    "command": "echo ok",
                    "background": true,
                    "workdir": ".kanzei/project",
                }),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(
            out.content.contains("must not sit inside `.kanzei/`"),
            "{}",
            out.content
        );

        // 跑出代码树:同样拒绝(跑出去就无从归因,守卫的对账范围也失去意义)。
        // 主树运行时 cwd == project_root,这一条与改前逐字节同义。
        let outside = root.parent().unwrap().join(format!(
            "kz-bash-outside-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&outside).unwrap();
        let out = BashTool
            .execute(
                serde_json::json!({
                    "command": "echo ok",
                    "background": true,
                    "workdir": outside.display().to_string(),
                }),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(
            out.content.contains("must stay inside the code tree"),
            "{}",
            out.content
        );
        std::fs::remove_dir_all(&outside).ok();
        std::fs::remove_dir_all(root).ok();
    }

    /// R-177 内容②:线上运行时 cwd = worktree、project_root = 主根,
    /// 后台围栏必须以**代码树**为界——否则线里每一条 background bash 都被无条件拒。
    #[tokio::test]
    async fn 后台workdir以代码树为界_worktree子目录放行_树外与两处kanzei仍拒() {
        let main_root = temp_project("bg-main");
        let worktree = temp_project("bg-worktree");
        let sub = worktree.join("crates");
        std::fs::create_dir_all(&sub).unwrap();
        let ctx = ToolCtx {
            cwd: worktree.clone(),
            project_root: main_root.clone(),
            ..Default::default()
        };
        // ① worktree 的子目录:放行(改前会因为「不在主根之下」被拒)。
        let out = BashTool
            .execute(
                serde_json::json!({
                    "command": "echo ok",
                    "background": true,
                    "workdir": "crates",
                }),
                &ctx,
            )
            .await;
        assert!(
            !out.is_error,
            "worktree 子目录必须放行,实际: {}",
            out.content
        );
        // ② worktree 内的 .kanzei 副本:仍拒。
        let out = BashTool
            .execute(
                serde_json::json!({
                    "command": "echo ok",
                    "background": true,
                    "workdir": ".kanzei/project",
                }),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(
            out.content.contains("must not sit inside `.kanzei/`"),
            "{}",
            out.content
        );
        // ③ 主根(代码树之外):仍拒。
        let out = BashTool
            .execute(
                serde_json::json!({
                    "command": "echo ok",
                    "background": true,
                    "workdir": main_root.display().to_string(),
                }),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(
            out.content.contains("must stay inside the code tree"),
            "{}",
            out.content
        );
        // ④ 主根的托管目录:即使换个走法也拒(两处 .kanzei 都在排除表里)。
        assert!(
            in_managed_dir(&main_root, &main_root.join(".kanzei/project")),
            "主根的 .kanzei 必须仍在排除表里"
        );
        std::fs::remove_dir_all(&worktree).ok();
        std::fs::remove_dir_all(&main_root).ok();
    }

    /// 固化事实(不是遗漏):托管快照恒取**主根**——托管文档只有主根一份,
    /// worktree 里的 `.kanzei` 副本不在围栏辖区,由 git 分支自己承担。
    #[test]
    fn 托管快照仍取主根_worktree内kanzei副本不在辖区() {
        let main_root = temp_project("snap-main");
        let worktree = temp_project("snap-worktree");
        std::fs::write(main_root.join(".kanzei/project/x.md"), "main").unwrap();
        std::fs::write(worktree.join(".kanzei/project/x.md"), "worktree").unwrap();
        let ctx = ToolCtx {
            cwd: worktree.clone(),
            project_root: main_root.clone(),
            ..Default::default()
        };
        let snapshot = ManagedSnapshot::capture(&ctx.project_root);
        assert_eq!(
            snapshot,
            ManagedSnapshot::capture(&main_root),
            "快照必须取主根那一份"
        );
        assert_ne!(
            snapshot,
            ManagedSnapshot::capture(&worktree),
            "两棵树的托管副本内容不同,快照不该取到 worktree 那份"
        );
        // 改 worktree 里的副本不影响主根快照——它压根不在辖区内。
        std::fs::write(worktree.join(".kanzei/project/x.md"), "worktree-changed").unwrap();
        assert_eq!(
            ManagedSnapshot::capture(&ctx.project_root),
            snapshot,
            "worktree 内的 .kanzei 副本改动不得进入托管围栏"
        );
        std::fs::remove_dir_all(&worktree).ok();
        std::fs::remove_dir_all(&main_root).ok();
    }

    /// D-174:托管项目里的后台启动从"一律拒绝"恢复为"受管启动"。
    /// R-097 的后台能力因此回来了——边界是单 run 内可用、跨 run 被收尾。
    #[tokio::test]
    async fn background_shell_is_managed_not_refused_in_managed_projects() {
        let root = temp_project("background-fence");
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            run_id: Some("run-a".into()),
            process_id: Some("proc-a".into()),
            ..Default::default()
        };
        let out = BashTool
            .execute(
                serde_json::json!({"command":"echo ok","background":true}),
                &ctx,
            )
            .await;
        assert!(
            !out.is_error,
            "托管项目里的后台启动不该再被拒绝: {}",
            out.content
        );
        assert!(out.content.contains("process_id:"), "{}", out.content);
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
                    ..Default::default()
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
                    ..Default::default()
                },
            )
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("timeout: true"), "{}", out.content);
        // D-262:超时路径现在要**等到进程树确认消失**才返回(见 shell::kill_tree),
        // 而本机 taskkill 光启动就是秒级(实测 1.0–4.2 秒,机器一忙更久)。原先 4 秒的
        // 上限是按"击杀不生效、超时只让调用返回"那套行为定的,不能再当基准。
        // 上限仍然要有——只是按真实击杀成本重新定档。
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "超时路径耗时 {:?}",
            started.elapsed()
        );
    }

    /// D-262 验收③(bash 超时这条路径):超时不只是让工具调用返回,
    /// 被超时的**进程树必须真的退出**。
    ///
    /// 缺陷影响②原文:"超时只是让工具调用返回,被击杀的进程继续持有文件与端口"。
    /// 所以断言的对象是 pid 的活性,而不是返回文本里有没有 "timeout: true"——
    /// 后者在击杀完全失效的旧实现下同样是绿的。
    ///
    /// 形态要点:①命令活 300 秒,自然退出冒充不了击杀;②带孙进程,`taskkill /t`
    /// 不生效就会留残留;③两个 pid 都由命令自己写进临时文件回传,不经 stdout
    /// (stdout 被 bash 工具自己抽着)。
    #[tokio::test]
    async fn timeout_actually_terminates_the_process_tree() {
        let shell = super::detected_shell();
        if !matches!(shell.name, "pwsh" | "powershell") {
            eprintln!("跳过:本测试需要 pwsh/powershell 才能回传孙进程 pid");
            return;
        }
        let root = temp_project("timeout-tree");
        let marker = root.join("pids.txt");
        let marker_str = marker.display().to_string().replace('\\', "/");
        let command = format!(
            "$c = Start-Process -NoNewWindow -PassThru -FilePath (Get-Process -Id $PID).Path \
             -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 300'; \
             [System.IO.File]::WriteAllText('{marker_str}', \"$PID,$($c.Id)\"); \
             Start-Sleep -Seconds 300"
        );
        let out = BashTool
            .execute(
                // 超时要给足:命令得先把孙进程起来并写完 pid,不然测试拿不到断言对象。
                serde_json::json!({"command": command, "timeout_ms": 8000}),
                &ToolCtx {
                    cwd: root.clone(),
                    project_root: root.clone(),
                    ..Default::default()
                },
            )
            .await;
        assert!(out.content.contains("timeout: true"), "{}", out.content);

        let text = std::fs::read_to_string(&marker).expect("命令应已回传 pid");
        let pids: Vec<u32> = text
            .trim()
            .split(',')
            .map(|p| p.trim().parse::<u32>().expect("pid"))
            .collect();
        assert_eq!(pids.len(), 2, "应回传 shell 与孙进程两个 pid: {text}");

        // 终止是异步的,所以"等到没"而不是"看一眼";等不到就如实红。
        let mut alive: Vec<u32> = Vec::new();
        for _ in 0..100 {
            alive = pids
                .iter()
                .copied()
                .filter(|p| crate::shell::process_alive(*p))
                .collect();
            if alive.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        for pid in &alive {
            crate::shell::kill_tree(*pid).await;
        }
        std::fs::remove_dir_all(&root).ok();
        assert!(
            alive.is_empty(),
            "D-262 验收③:bash 超时后进程树必须真的退出,残留 pid {alive:?}"
        );
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
                    ..Default::default()
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
                    ..Default::default()
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
