//! 在外部程序里打开会话所在的位置:资源管理器、VS Code、Windows 终端、PowerShell、Git Bash,
//! 以及设置→打开方式里自配的工具(UX 整改 §3:`reveal_path` / `open_tools_list` / `open_with` /
//! `open_tools_save`)。
//!
//! 目标目录:带工作树的独立任务 → 其工作树;其余(主对话、讨论)→ 项目根。
//!
//! Windows 细节(纯逻辑部分都有单测钉住,测试不启动任何外部进程):
//! - `explorer.exe` 成功时退出码也可能非 0 → 只 spawn、不等待,**只在启动失败时报错**。
//! - 交给外部程序的目录先去掉 `\\?\` 前缀、统一成反斜杠、去掉末尾分隔符:explorer 对
//!   `\\?\` 与正斜杠的处理不可靠。
//! - 路径含空格(「kanzei code」)靠逐个参数传给 `CreateProcess`,由标准库加引号。
//! - `code` 是 `.cmd` 垫片:优先用它旁边的 `Code.exe` 直接起(不过 cmd,任何字符都安全);
//!   找不到才走 `cmd /d /s /c`,整行自己拼好,并拒绝 `%` `"` 这类 cmd 会再解析的字符。
//! - Windows 终端的 `wt.exe` 是 App Execution Alias(重解析点),探测用 `symlink_metadata`。
//! - 不等待、不继承 stdio、不弹控制台黑框(PowerShell 例外:它就是要一个新控制台)。

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use crate::prefs::{load_prefs, save_prefs, write_guard, OpenToolConfig};
use crate::state::{default_process_id, AppState};
use crate::{normalized_project_root, MutexPoisonExt};

/// 自配工具的数量上限(设置页一屏放得下,也防止误把几百行贴进来)。
const MAX_CUSTOM_TOOLS: usize = 20;
const PATH_PLACEHOLDER: &str = "{path}";
const MAX_ID_CHARS: usize = 40;

/// 内置工具:(id, 界面名)。顺序即界面顺序;自配工具的 id 不得与它们重名。
const BUILTIN_TOOLS: [(&str, &str); 4] = [
    ("vscode", "VS Code"),
    ("wt", "Windows 终端"),
    ("powershell", "PowerShell"),
    ("git-bash", "Git Bash"),
];

// ---------- 程序探测 ----------

/// 在一份 PATH 里找可执行文件的探测器。字段可注入,单测用临时目录当 PATH,不依赖本机装了什么。
pub(crate) struct Locator {
    path: OsString,
    /// 小写、带点的可执行扩展名(`.exe` `.cmd`…)。非 Windows 为空。
    pathext: Vec<String>,
}

impl Locator {
    /// 本进程视角的 PATH ∪ 注册表里新增的目录(装完 VS Code 不必重启 kanzei 就能认到)。
    pub(crate) fn system() -> Self {
        let pathext = if cfg!(windows) {
            std::env::var("PATHEXT")
                .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
                .split(';')
                .map(|ext| ext.trim().to_ascii_lowercase())
                .filter(|ext| !ext.is_empty())
                .collect()
        } else {
            Vec::new()
        };
        Self {
            path: kanzei_tools::fresh_path(),
            pathext,
        }
    }

    pub(crate) fn locate(&self, name: &str) -> Option<PathBuf> {
        locate_in(name, &self.path, &self.pathext)
    }
}

/// 在 PATH 目录里找可执行文件(Windows 按 `pathext` 补扩展名)。
///
/// 没有扩展名的同名文件不算:VS Code 的 `bin\` 里同时有 bash 脚本 `code` 与 `code.cmd`,
/// 前者 `CreateProcess` 起不来。用 `symlink_metadata` 而不是 `is_file`:`wt.exe` 是 App
/// Execution Alias(重解析点),`metadata` 对它会失败。
pub(crate) fn locate_in(name: &str, path_var: &OsStr, pathext: &[String]) -> Option<PathBuf> {
    let is_file = |candidate: &Path| {
        candidate
            .symlink_metadata()
            .is_ok_and(|meta| !meta.is_dir())
    };
    let name_has_known_ext = pathext.is_empty()
        || Path::new(name).extension().is_some_and(|ext| {
            let ext = format!(".{}", ext.to_string_lossy().to_ascii_lowercase());
            pathext.contains(&ext)
        });
    let probe = |base: PathBuf| -> Option<PathBuf> {
        if name_has_known_ext && is_file(base.as_path()) {
            return Some(base);
        }
        pathext
            .iter()
            .map(|ext| PathBuf::from(format!("{}{ext}", base.display())))
            .find(|candidate| is_file(candidate.as_path()))
    };
    let named = Path::new(name);
    if named.is_absolute() {
        return probe(named.to_path_buf());
    }
    std::env::split_paths(path_var)
        .filter(|dir| !dir.as_os_str().is_empty())
        .find_map(|dir| probe(dir.join(name)))
}

/// 常见的安装位置(PATH 里没有时的兜底):`%LOCALAPPDATA%\Programs\<dir>\<exe>` 与
/// `%ProgramFiles%\<dir>\<exe>`。
fn known_install(dir: &str, exe: &str) -> Option<PathBuf> {
    let user = std::env::var_os("LOCALAPPDATA")
        .map(|base| PathBuf::from(base).join("Programs").join(dir).join(exe));
    let machine =
        std::env::var_os("ProgramFiles").map(|base| PathBuf::from(base).join(dir).join(exe));
    [user, machine]
        .into_iter()
        .flatten()
        .find(|candidate| candidate.is_file())
}

/// `<安装目录>\bin\code.cmd` 上一级的 `Code.exe`。
fn vscode_exe_beside(shim: &Path) -> Option<PathBuf> {
    let exe = shim.parent()?.parent()?.join("Code.exe");
    exe.is_file().then_some(exe)
}

fn resolve_vscode(locator: &Locator) -> Option<PathBuf> {
    match locator.locate("code") {
        Some(shim) => Some(vscode_exe_beside(&shim).unwrap_or(shim)),
        None => known_install("Microsoft VS Code", "Code.exe"),
    }
}

/// `git.exe` 在 `<Git>\cmd\`(或 `bin\`),`git-bash.exe` 在 `<Git>\` 根。
fn resolve_git_bash(locator: &Locator) -> Option<PathBuf> {
    locator
        .locate("git")
        .and_then(|git| Some(git.parent()?.parent()?.join("git-bash.exe")))
        .filter(|exe| exe.is_file())
        .or_else(|| known_install("Git", "git-bash.exe"))
}

fn resolve_builtin(id: &str, locator: &Locator) -> Option<PathBuf> {
    match id {
        "vscode" => resolve_vscode(locator),
        "wt" => locator.locate("wt"),
        "powershell" => locator
            .locate("pwsh")
            .or_else(|| locator.locate("powershell")),
        "git-bash" => resolve_git_bash(locator),
        _ => None,
    }
}

// ---------- 路径与启动计划 ----------

/// 交给外部程序的目录写法:去 `\\?\` 前缀、统一反斜杠、去末尾分隔符(盘符根 `C:\` 保留)。
pub(crate) fn external_path(path: &Path) -> String {
    let simplified = kanzei_tools::path_form::simplify(path);
    let mut text = simplified.display().to_string();
    if cfg!(windows) {
        text = text.replace('/', "\\");
    }
    let trimmed = text.trim_end_matches(['\\', '/']);
    if trimmed.is_empty() {
        text
    } else if trimmed.len() == 2 && trimmed.ends_with(':') {
        format!("{trimmed}\\")
    } else {
        trimmed.to_string()
    }
}

/// 内置工具的命令行参数(目标路径已是 [`external_path`] 的写法)。
fn builtin_args(id: &str, target: &str) -> Vec<String> {
    match id {
        "vscode" => vec![target.into()],
        "wt" => vec!["-d".into(), target.into()],
        // 单引号字面量里只有 `'` 需要转义(写两个)。
        "powershell" => vec![
            "-NoExit".into(),
            "-Command".into(),
            format!("Set-Location -LiteralPath '{}'", target.replace('\'', "''")),
        ],
        "git-bash" => vec![format!("--cd={target}")],
        _ => Vec::new(),
    }
}

/// 自配工具的参数:`{path}` 换成目标目录;一个 `{path}` 都没有就把目录补在最后
/// (打开方式不带目标肯定是配置漏了)。
pub(crate) fn substitute_args(template: &[String], target: &str) -> Vec<String> {
    let mut args: Vec<String> = template
        .iter()
        .map(|arg| arg.replace(PATH_PLACEHOLDER, target))
        .collect();
    if !template.iter().any(|arg| arg.contains(PATH_PLACEHOLDER)) {
        args.push(target.to_string());
    }
    args
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LaunchPlan {
    /// 直接起一个可执行文件。
    Direct {
        program: PathBuf,
        args: Vec<String>,
        /// 需要一个新的控制台窗口(PowerShell)。
        new_console: bool,
    },
    /// 经 `cmd /d /s /c` 起 `.cmd`/`.bat` 垫片。
    Shim { shim: PathBuf, args: Vec<String> },
}

/// `.cmd`/`.bat` 必须经 cmd 起,其余直接起。
pub(crate) fn plan_launch(program: PathBuf, args: Vec<String>, new_console: bool) -> LaunchPlan {
    let ext = program
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase());
    if matches!(ext.as_deref(), Some("cmd" | "bat")) {
        LaunchPlan::Shim {
            shim: program,
            args,
        }
    } else {
        LaunchPlan::Direct {
            program,
            args,
            new_console,
        }
    }
}

/// `cmd` 的参数串:`/d /s /c ""<垫片>" "<参数>"…"`。`/s` 让 cmd 只剥最外一层引号,
/// 每个参数自己加引号才保得住空格。cmd 在引号里仍会展开 `%VAR%`、引号本身又无法转义,
/// 所以含这些字符(或换行)的一律拒绝,而不是赌它碰巧能行。
pub(crate) fn shim_command_line(shim: &Path, args: &[String]) -> Result<String, String> {
    let unsafe_text = |text: &str| text.chars().any(|c| matches!(c, '"' | '%' | '\r' | '\n'));
    let shim_text = shim.display().to_string();
    if unsafe_text(&shim_text) {
        return Err(format!(
            "程序路径含有 \" % 或换行,无法经 .cmd 垫片启动: {shim_text}"
        ));
    }
    let mut line = format!("/d /s /c \"\"{shim_text}\"");
    for arg in args {
        if unsafe_text(arg) {
            return Err(format!(
                "路径或参数含有 \" % 或换行,无法经 .cmd 垫片传递: {arg}(换用能直接启动的 .exe 工具)"
            ));
        }
        line.push_str(&format!(" \"{arg}\""));
    }
    line.push('"');
    Ok(line)
}

fn missing_tool_error(label: &str) -> String {
    format!(
        "没找到「{label}」:它不在 PATH 里,也不在常见安装位置。装好后重新打开 kanzei,\
         或在 设置→打开方式 里加一个指向它的自定义工具"
    )
}

/// 工具 id → 启动计划。`target` 已是 [`external_path`] 的写法。
pub(crate) fn plan_for_tool(
    tool_id: &str,
    target: &str,
    custom: &[OpenToolConfig],
    locator: &Locator,
) -> Result<LaunchPlan, String> {
    if let Some((id, label)) = BUILTIN_TOOLS.iter().find(|(id, _)| *id == tool_id) {
        let program = resolve_builtin(id, locator).ok_or_else(|| missing_tool_error(label))?;
        // wt 把 `;` 当子命令分隔符,目录名里带它无法安全传递。
        if *id == "wt" && target.contains(';') {
            return Err(format!(
                "路径含有「;」,Windows 终端会把它当命令分隔符: {target}。换用其它打开方式"
            ));
        }
        return Ok(plan_launch(
            program,
            builtin_args(id, target),
            *id == "powershell",
        ));
    }
    let tool = custom
        .iter()
        .find(|tool| tool.id == tool_id)
        .ok_or_else(|| format!("未知的打开方式「{tool_id}」,请在 设置→打开方式 里检查"))?;
    let program = locator.locate(&tool.command).ok_or_else(|| {
        format!(
            "没找到「{}」的程序 {}:确认它已安装且在 PATH 里,或在 设置→打开方式 里改成完整路径",
            tool.label, tool.command
        )
    })?;
    Ok(plan_launch(
        program,
        substitute_args(&tool.args, target),
        false,
    ))
}

// ---------- 启动 ----------

/// 起进程但不等待:调用方只关心「起不起得来」。
#[cfg(windows)]
fn spawn(plan: &LaunchPlan) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = match plan {
        LaunchPlan::Direct {
            program,
            args,
            new_console,
        } => {
            let mut command = Command::new(program);
            command.args(args);
            if *new_console {
                command.creation_flags(CREATE_NEW_CONSOLE);
            }
            command
        }
        LaunchPlan::Shim { shim, args } => {
            let mut command = Command::new("cmd");
            command.raw_arg(shim_command_line(shim, args)?);
            command.creation_flags(CREATE_NO_WINDOW);
            command
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("启动失败: {error}"))
}

#[cfg(not(windows))]
fn spawn(plan: &LaunchPlan) -> Result<(), String> {
    let LaunchPlan::Direct { program, args, .. } = plan else {
        return Err("当前系统不支持 .cmd 垫片".into());
    };
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("启动失败: {error}"))
}

fn spawn_explorer(path: &str) -> Result<(), String> {
    #[cfg(windows)]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let program = "xdg-open";
    // explorer 成功时退出码也可能非 0:只看能不能起来,不等不看退出码。
    Command::new(program)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法打开资源管理器: {error}"))
}

// ---------- 目标目录 ----------

/// 并行线开工作树,其余开项目根。
pub(crate) fn pick_target(root: &Path, worktree: Option<&Path>) -> PathBuf {
    worktree.unwrap_or(root).to_path_buf()
}

fn missing_dir_message(target: &Path, is_worktree: bool) -> String {
    if is_worktree {
        format!(
            "独立任务的工作树已不在磁盘上({}):它可能已被合并回收,或被手动删除了",
            target.display()
        )
    } else {
        format!(
            "项目目录不存在或暂时不可访问({}):磁盘是否已断开、文件夹是否被移动过?",
            target.display()
        )
    }
}

/// 会话 → 要打开的目录。内存进程表优先;重启后还没恢复到内存的线从 state.db 取
/// (只读,不触发恢复的副作用)。找不到这条会话就明说,不静默退回项目根。
fn process_worktree(
    state: &AppState,
    root: &Path,
    process_id: &str,
) -> Result<Option<PathBuf>, String> {
    if let Some(process) = state.processes.lock_or_recover().get(process_id) {
        return Ok(process.worktree_path.as_ref().map(|path| path.0.clone()));
    }
    let state_path = kanzei_core::project_state_path(root);
    let store = state_path
        .is_file()
        .then(|| kanzei_core::SessionStore::open_read_only(&state_path).ok())
        .flatten();
    match store.and_then(|store| store.get_process(process_id).ok().flatten()) {
        Some(record) => Ok(record.worktree_path.map(PathBuf::from)),
        None => Err("找不到这个对话(它可能已被删除)".into()),
    }
}

pub(crate) fn resolve_target(
    state: &AppState,
    project_dir: &str,
    process_id: Option<&str>,
) -> Result<PathBuf, String> {
    let root = normalized_project_root(Path::new(project_dir));
    let default_id = default_process_id(&root);
    let worktree = match process_id.filter(|id| !id.is_empty() && *id != default_id) {
        Some(id) => process_worktree(state, &root, id)?,
        None => None,
    };
    let target = pick_target(&root, worktree.as_deref());
    if !target.is_dir() {
        return Err(missing_dir_message(&target, worktree.is_some()));
    }
    Ok(target)
}

// ---------- 自配工具 ----------

/// 校验并整理设置页提交的自配工具。id 为空的补一个不重名的;参数里没有 `{path}` 的补上。
pub(crate) fn validate_tools(tools: Vec<OpenToolConfig>) -> Result<Vec<OpenToolConfig>, String> {
    if tools.len() > MAX_CUSTOM_TOOLS {
        return Err(format!("自配的打开方式最多 {MAX_CUSTOM_TOOLS} 个"));
    }
    let has_control = |text: &str| text.chars().any(char::is_control);
    let mut cleaned = Vec::with_capacity(tools.len());
    let mut taken: HashSet<String> = BUILTIN_TOOLS.iter().map(|(id, _)| id.to_string()).collect();
    for (index, tool) in tools.into_iter().enumerate() {
        let label = tool.label.trim().to_string();
        let command = tool.command.trim().to_string();
        if label.is_empty() {
            return Err(format!("第 {} 个打开方式缺少名称", index + 1));
        }
        if command.is_empty() {
            return Err(format!("「{label}」缺少要运行的命令"));
        }
        if has_control(&label)
            || has_control(&command)
            || tool.args.iter().any(|arg| has_control(arg.as_str()))
        {
            return Err(format!("「{label}」的名称、命令或参数里有控制字符"));
        }
        let id = tool.id.trim().to_string();
        if !id.is_empty() {
            let well_formed = id.chars().count() <= MAX_ID_CHARS
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if !well_formed {
                return Err(format!(
                    "「{label}」的标识 {id} 只能用字母、数字、- 和 _(最多 {MAX_ID_CHARS} 个字符)"
                ));
            }
            if !taken.insert(id.to_ascii_lowercase()) {
                return Err(format!("打开方式的标识 {id} 重复了(或与内置工具重名)"));
            }
        }
        cleaned.push((id, label, command, tool.args));
    }
    let mut out = Vec::with_capacity(cleaned.len());
    for (id, label, command, mut args) in cleaned {
        let id = if id.is_empty() {
            let mut n = out.len() + 1;
            while !taken.insert(format!("custom-{n}")) {
                n += 1;
            }
            format!("custom-{n}")
        } else {
            id
        };
        if !args.iter().any(|arg| arg.contains(PATH_PLACEHOLDER)) {
            args.push(PATH_PLACEHOLDER.to_string());
        }
        out.push(OpenToolConfig {
            id,
            label,
            command,
            args,
        });
    }
    Ok(out)
}

/// 「用…打开」的工具清单:内置在前,自配在后。内置项 `command` 为 null、`args` 为空。
pub(crate) fn tool_entries(custom: &[OpenToolConfig], locator: &Locator) -> Vec<Value> {
    let mut entries = BUILTIN_TOOLS
        .iter()
        .map(|(id, label)| {
            json!({
                "id": id, "label": label, "builtin": true,
                "available": resolve_builtin(id, locator).is_some(),
                "command": null, "args": [],
            })
        })
        .collect::<Vec<_>>();
    entries.extend(custom.iter().map(|tool| {
        json!({
            "id": tool.id, "label": tool.label, "builtin": false,
            "available": locator.locate(&tool.command).is_some(),
            "command": tool.command, "args": tool.args,
        })
    }));
    entries
}

// ---------- Tauri 命令 ----------

/// 在资源管理器中打开该会话的位置:独立任务 → 其工作树,其余 → 项目根。
#[tauri::command(async)]
pub fn reveal_path(
    state: tauri::State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
) -> Result<(), String> {
    let target = resolve_target(&state, &project_dir, process_id.as_deref())?;
    spawn_explorer(&external_path(&target))
}

#[tauri::command(async)]
pub fn open_tools_list() -> Result<Vec<Value>, String> {
    Ok(tool_entries(&load_prefs().open_tools, &Locator::system()))
}

/// 用指定工具打开该会话的位置(目标同 [`reveal_path`])。
#[tauri::command(async)]
pub fn open_with(
    state: tauri::State<'_, AppState>,
    tool_id: String,
    project_dir: String,
    process_id: Option<String>,
) -> Result<(), String> {
    let target = resolve_target(&state, &project_dir, process_id.as_deref())?;
    let plan = plan_for_tool(
        &tool_id,
        &external_path(&target),
        &load_prefs().open_tools,
        &Locator::system(),
    )?;
    spawn(&plan)
}

/// 设置页整表保存自配工具。
#[tauri::command(async)]
pub fn open_tools_save(tools: Vec<OpenToolConfig>) -> Result<(), String> {
    let tools = validate_tools(tools)?;
    let _guard = write_guard()?;
    let mut prefs = load_prefs();
    prefs.open_tools = tools;
    save_prefs(&prefs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::sync::Mutex;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-os-open-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    fn exts() -> Vec<String> {
        [".com", ".exe", ".bat", ".cmd"].map(String::from).to_vec()
    }

    /// 只含给定目录的探测器(不碰本机真实 PATH)。
    fn locator(dirs: &[&Path]) -> Locator {
        Locator {
            path: std::env::join_paths(dirs).unwrap(),
            pathext: exts(),
        }
    }

    #[test]
    fn 探测按pathext补扩展名_无扩展名的同名脚本不算() {
        let bin = temp_dir("locate").join("bin");
        touch(&bin.join("code")); // VS Code bin 里的 bash 脚本,CreateProcess 起不来
        let none = locate_in("code", bin.as_os_str(), &exts());
        assert_eq!(none, None, "只有无扩展名的脚本时找不到");
        touch(&bin.join("code.cmd"));
        assert_eq!(
            locate_in("code", bin.as_os_str(), &exts()),
            Some(bin.join("code.cmd"))
        );
        // 名字自带已知扩展名也认。
        assert_eq!(
            locate_in("code.cmd", bin.as_os_str(), &exts()),
            Some(bin.join("code.cmd"))
        );
        // 绝对路径直接认(自配工具常这么写),同样会补扩展名。
        let abs = bin.join("code").display().to_string();
        assert_eq!(
            locate_in(&abs, OsStr::new(""), &exts()),
            Some(bin.join("code.cmd"))
        );
        // 目录不算文件。
        std::fs::create_dir_all(bin.join("tool.exe")).unwrap();
        assert_eq!(locate_in("tool", bin.as_os_str(), &exts()), None);
        std::fs::remove_dir_all(bin.parent().unwrap()).ok();
    }

    #[test]
    fn 外部路径去前缀统一反斜杠去末尾分隔符() {
        assert_eq!(
            external_path(Path::new(r"\\?\C:\Users\kanzei\Documents\kanzei code\")),
            r"C:\Users\kanzei\Documents\kanzei code"
        );
        assert_eq!(external_path(Path::new(r"C:\a b\c")), r"C:\a b\c");
        // 盘符根保留末尾反斜杠,否则 `C:` 指的是该盘当前目录。
        assert_eq!(external_path(Path::new(r"C:\")), r"C:\");
        assert_eq!(external_path(Path::new(r"\\?\D:\")), r"D:\");
        #[cfg(windows)]
        assert_eq!(external_path(Path::new("C:/x/y/")), r"C:\x\y");
    }

    #[test]
    fn 自配参数替换占位符_缺占位符时补目录() {
        let target = r"C:\a b";
        assert_eq!(
            substitute_args(&["--new-window".into(), "{path}".into()], target),
            ["--new-window", r"C:\a b"]
        );
        assert_eq!(
            substitute_args(&["--cd={path}".into()], target),
            [r"--cd=C:\a b"]
        );
        assert_eq!(substitute_args(&["-x".into()], target), ["-x", r"C:\a b"]);
        assert_eq!(substitute_args(&[], target), [r"C:\a b"]);
    }

    #[test]
    fn 垫片命令行逐参数加引号_含百分号引号换行的拒绝() {
        let shim = Path::new(r"C:\Program Files\Tool\bin\tool.cmd");
        let line = shim_command_line(shim, &[r"C:\a b\c".into(), "-x".into()]).unwrap();
        assert_eq!(
            line,
            r#"/d /s /c ""C:\Program Files\Tool\bin\tool.cmd" "C:\a b\c" "-x"""#
        );
        for bad in ["C:\\50%\\x", "C:\\a\"b", "a\nb"] {
            assert!(shim_command_line(shim, &[bad.into()]).is_err(), "{bad:?}");
        }
        assert!(
            shim_command_line(Path::new(r"C:\100%\tool.cmd"), &[]).is_err(),
            "垫片路径本身含 % 也拒绝"
        );
    }

    #[test]
    fn 启动计划按扩展名分直接与垫片() {
        assert!(matches!(
            plan_launch(PathBuf::from(r"C:\x\a.CMD"), vec![], false),
            LaunchPlan::Shim { .. }
        ));
        assert!(matches!(
            plan_launch(PathBuf::from(r"C:\x\a.bat"), vec![], false),
            LaunchPlan::Shim { .. }
        ));
        assert_eq!(
            plan_launch(PathBuf::from(r"C:\x\a.exe"), vec!["1".into()], true),
            LaunchPlan::Direct {
                program: PathBuf::from(r"C:\x\a.exe"),
                args: vec!["1".into()],
                new_console: true,
            }
        );
    }

    #[test]
    fn vscode优先用旁边的code_exe_没有才走cmd垫片() {
        let root = temp_dir("vscode");
        let install = root.join("Microsoft VS Code");
        touch(&install.join("bin").join("code.cmd"));
        let path = install.join("bin");
        let target = r"C:\Users\kanzei\Documents\kanzei code";
        // 只有垫片:经 cmd,路径含空格由整行引号保住。
        let plan = plan_for_tool("vscode", target, &[], &locator(&[path.as_path()])).unwrap();
        assert_eq!(
            plan,
            LaunchPlan::Shim {
                shim: path.join("code.cmd"),
                args: vec![target.into()],
            }
        );
        // 旁边有 Code.exe:直接起它,不过 cmd。
        touch(&install.join("Code.exe"));
        let plan = plan_for_tool("vscode", target, &[], &locator(&[path.as_path()])).unwrap();
        assert_eq!(
            plan,
            LaunchPlan::Direct {
                program: install.join("Code.exe"),
                args: vec![target.into()],
                new_console: false,
            }
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 工具缺失给可行动错误_未知工具也明说() {
        let empty = temp_dir("missing");
        let none = locator(&[empty.as_path()]);
        let error = plan_for_tool("wt", r"C:\p", &[], &none).unwrap_err();
        assert!(
            error.contains("Windows 终端") && error.contains("设置→打开方式"),
            "{error}"
        );
        let error = plan_for_tool("nope", r"C:\p", &[], &none).unwrap_err();
        assert!(
            error.contains("未知的打开方式") && error.contains("nope"),
            "{error}"
        );
        let custom = [OpenToolConfig {
            id: "zed".into(),
            label: "Zed".into(),
            command: "zed-not-installed".into(),
            args: vec!["{path}".into()],
        }];
        let error = plan_for_tool("zed", r"C:\p", &custom, &none).unwrap_err();
        assert!(
            error.contains("Zed") && error.contains("zed-not-installed"),
            "{error}"
        );
        std::fs::remove_dir_all(&empty).ok();
    }

    #[test]
    fn windows终端与powershell的参数() {
        let root = temp_dir("shells");
        touch(&root.join("wt.exe"));
        touch(&root.join("powershell.exe"));
        let found = locator(&[root.as_path()]);
        assert_eq!(
            plan_for_tool("wt", r"C:\a b", &[], &found).unwrap(),
            LaunchPlan::Direct {
                program: root.join("wt.exe"),
                args: vec!["-d".into(), r"C:\a b".into()],
                new_console: false,
            }
        );
        // wt 把 `;` 当子命令分隔符:带分号的目录拒绝,不赌能行。
        assert!(plan_for_tool("wt", r"C:\a;b", &[], &found).is_err());
        // PowerShell:单引号字面量里 `'` 写两个;要新控制台;pwsh 缺席时回落 powershell.exe。
        let plan = plan_for_tool("powershell", r"C:\O'Brien\x", &[], &found).unwrap();
        assert_eq!(
            plan,
            LaunchPlan::Direct {
                program: root.join("powershell.exe"),
                args: vec![
                    "-NoExit".into(),
                    "-Command".into(),
                    r"Set-Location -LiteralPath 'C:\O''Brien\x'".into(),
                ],
                new_console: true,
            }
        );
        touch(&root.join("pwsh.exe"));
        let plan = plan_for_tool("powershell", r"C:\x", &[], &found).unwrap();
        assert!(
            matches!(&plan, LaunchPlan::Direct { program, .. } if *program == root.join("pwsh.exe"))
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn gitbash从git旁边的根目录找() {
        let root = temp_dir("gitbash");
        touch(&root.join("Git").join("cmd").join("git.exe"));
        touch(&root.join("Git").join("git-bash.exe"));
        let cmd_dir = root.join("Git").join("cmd");
        let found = locator(&[cmd_dir.as_path()]);
        assert_eq!(
            plan_for_tool("git-bash", r"C:\a b", &[], &found).unwrap(),
            LaunchPlan::Direct {
                program: root.join("Git").join("git-bash.exe"),
                args: vec![r"--cd=C:\a b".into()],
                new_console: false,
            }
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 自配工具按命令探测并替换占位符() {
        let root = temp_dir("custom");
        touch(&root.join("zed.exe"));
        let custom = [OpenToolConfig {
            id: "zed".into(),
            label: "Zed".into(),
            command: "zed".into(),
            args: vec!["--new".into(), "{path}".into()],
        }];
        assert_eq!(
            plan_for_tool("zed", r"C:\a b", &custom, &locator(&[root.as_path()])).unwrap(),
            LaunchPlan::Direct {
                program: root.join("zed.exe"),
                args: vec!["--new".into(), r"C:\a b".into()],
                new_console: false,
            }
        );
        std::fs::remove_dir_all(&root).ok();
    }

    fn tool(id: &str, label: &str, command: &str, args: &[&str]) -> OpenToolConfig {
        OpenToolConfig {
            id: id.into(),
            label: label.into(),
            command: command.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }

    #[test]
    fn 保存校验_补id补占位符_拒绝重名与坏字符() {
        let saved = validate_tools(vec![
            tool("zed", " Zed ", " zed ", &["--new"]),
            tool("", "Notepad++", "notepad++.exe", &[]),
            tool("", "Sublime", "subl", &["{path}"]),
        ])
        .unwrap();
        assert_eq!(saved[0], tool("zed", "Zed", "zed", &["--new", "{path}"]));
        assert_eq!(saved[1].id, "custom-2", "空 id 补成不重名的");
        assert_eq!(saved[1].args, ["{path}"]);
        assert_eq!(saved[2].id, "custom-3");
        // 显式 id 与生成 id 撞车时生成的让位。
        let saved = validate_tools(vec![
            tool("", "A", "a", &[]),
            tool("custom-1", "B", "b", &[]),
        ])
        .unwrap();
        assert_eq!(saved[1].id, "custom-1");
        assert_eq!(saved[0].id, "custom-2");

        let bad = |tools: Vec<OpenToolConfig>| validate_tools(tools).unwrap_err();
        assert!(bad(vec![tool("vscode", "我的", "x", &[])]).contains("重名"));
        assert!(bad(vec![tool("VSCode", "我的", "x", &[])]).contains("重复"));
        assert!(bad(vec![tool("a", "A", "a", &[]), tool("A", "B", "b", &[])]).contains("重复"));
        assert!(bad(vec![tool("a b", "A", "a", &[])]).contains("只能用"));
        assert!(bad(vec![tool("a", "  ", "a", &[])]).contains("缺少名称"));
        assert!(bad(vec![tool("a", "A", "  ", &[])]).contains("缺少要运行的命令"));
        assert!(bad(vec![tool("a", "A", "a\nb", &[])]).contains("控制字符"));
        assert!(bad(vec![tool("a", "A", "a", &["x\ty\u{7}"])]).contains("控制字符"));
        let many = (0..=MAX_CUSTOM_TOOLS)
            .map(|i| tool(&format!("t{i}"), "T", "t", &[]))
            .collect();
        assert!(bad(many).contains("最多"));
        assert!(validate_tools(vec![]).unwrap().is_empty(), "清空列表合法");
    }

    #[test]
    fn 工具清单内置在前自配在后_内置项没有命令() {
        let root = temp_dir("entries");
        touch(&root.join("wt.exe"));
        let custom = [tool("zed", "Zed", "zed-missing", &["{path}"])];
        let entries = tool_entries(&custom, &locator(&[root.as_path()]));
        let ids: Vec<_> = entries.iter().map(|e| e["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["vscode", "wt", "powershell", "git-bash", "zed"]);
        assert_eq!(entries[1]["available"], true, "探测到 wt");
        // PowerShell 没有安装位置兜底:探测器里没有 pwsh/powershell 就是不可用。
        // (VS Code / Git Bash 有常见安装目录兜底,会随本机是否装了它们而变,这里不断言。)
        assert_eq!(entries[2]["available"], false);
        assert_eq!(entries[0]["builtin"], true);
        assert!(entries[0]["command"].is_null());
        assert_eq!(entries[4]["builtin"], false);
        assert_eq!(entries[4]["command"], "zed-missing");
        assert_eq!(entries[4]["available"], false, "自配工具的命令也要探测");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn open_tools_list_形状与ipc契约一致() {
        let nowhere = temp_dir("contract");
        let entries = tool_entries(&[], &locator(&[nowhere.as_path()]));
        crate::ipc_contract::tests::check_contract(
            "open_tools_list",
            crate::ipc_contract::shape(&Value::Array(entries)),
            "open_tools_list 的 IPC 形状变了",
        );
        std::fs::remove_dir_all(&nowhere).ok();
    }

    /// 并行线 → 工作树,其余 → 项目根;目录没了给可行动的错误;找不到的会话明说。
    #[test]
    fn 目标目录并行线开工作树_其余开项目根() {
        let base = temp_dir("target");
        let root = normalized_project_root(&base);
        let worktree = root.join("wt-line");
        std::fs::create_dir_all(&worktree).unwrap();
        let state = AppState::default();
        let main = crate::ensure_default_process(&state, &root);
        let project = root.display().to_string();

        assert_eq!(resolve_target(&state, &project, None).unwrap(), root);
        assert_eq!(resolve_target(&state, &project, Some("")).unwrap(), root);
        assert_eq!(
            resolve_target(&state, &project, Some(main.id.as_str())).unwrap(),
            root
        );

        let id = format!("p2|{project}");
        let line = |worktree_path: Option<PathBuf>| crate::ProcessHandle {
            id: id.clone(),
            origin_project: crate::ProjectRoot(root.clone()),
            project_dir: crate::ProjectRoot(root.clone()),
            worktree_path: worktree_path.map(crate::WorktreeRoot),
            branch: None,
            model: Arc::new(Mutex::new(None)),
            profile: Arc::new(Mutex::new(None)),
            research_topic: Arc::new(Mutex::new(None)),
            reasoning: Arc::new(Mutex::new(None)),
            manual_models: Arc::new(Mutex::new(Vec::new())),
            phase_pipeline_enabled: Arc::new(AtomicBool::new(false)),
            subagents_enabled: Arc::new(AtomicBool::new(true)),
            tracker_writes_enabled: Arc::new(AtomicBool::new(false)),
        };
        state
            .processes
            .lock_or_recover()
            .insert(id.clone(), line(Some(worktree.clone())));
        assert_eq!(
            resolve_target(&state, &project, Some(id.as_str())).unwrap(),
            worktree
        );
        // 不带工作树的会话(讨论)开项目根。
        state
            .processes
            .lock_or_recover()
            .insert(id.clone(), line(None));
        assert_eq!(
            resolve_target(&state, &project, Some(id.as_str())).unwrap(),
            root
        );
        // 工作树被删:给「工作树已不在磁盘上」,而不是悄悄退回项目根。
        state
            .processes
            .lock_or_recover()
            .insert(id.clone(), line(Some(worktree.clone())));
        std::fs::remove_dir_all(&worktree).unwrap();
        let error = resolve_target(&state, &project, Some(id.as_str())).unwrap_err();
        assert!(error.contains("工作树已不在磁盘上"), "{error}");
        // 不存在的会话。
        let error = resolve_target(&state, &project, Some("p9|nowhere")).unwrap_err();
        assert!(error.contains("找不到这个对话"), "{error}");
        // 项目根没了。
        std::fs::remove_dir_all(&root).unwrap();
        let error = resolve_target(&state, &project, None).unwrap_err();
        assert!(error.contains("项目目录不存在"), "{error}");
    }
}
