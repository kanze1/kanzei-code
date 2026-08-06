//! shell 检测(移植 opencode core/src/shell.ts 的精简版)。
//! Windows 优先级:pwsh → powershell → cmd(git-bash 留到 M5,避免 MSYS 环境歧义)。

use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct DetectedShell {
    /// 展示名,如 "pwsh" / "powershell" / "cmd" / "sh"。
    pub name: &'static str,
    /// 可执行文件。
    pub program: PathBuf,
    /// 执行一条命令所需的前置参数,命令文本追加在最后。
    pub args: Vec<&'static str>,
}

pub fn detected_shell() -> &'static DetectedShell {
    static SHELL: OnceLock<DetectedShell> = OnceLock::new();
    SHELL.get_or_init(detect)
}

#[cfg(windows)]
fn detect() -> DetectedShell {
    if let Some(p) = which("pwsh.exe") {
        return DetectedShell {
            name: "pwsh",
            program: p,
            args: vec!["-NoProfile", "-NonInteractive", "-Command"],
        };
    }
    if let Some(p) = which("powershell.exe") {
        return DetectedShell {
            name: "powershell",
            program: p,
            args: vec!["-NoProfile", "-NonInteractive", "-Command"],
        };
    }
    let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
    DetectedShell {
        name: "cmd",
        program: comspec.into(),
        args: vec!["/d", "/s", "/c"],
    }
}

#[cfg(not(windows))]
fn detect() -> DetectedShell {
    for candidate in ["/bin/bash", "/bin/zsh", "/bin/sh"] {
        if std::path::Path::new(candidate).exists() {
            let name: &'static str = match candidate {
                "/bin/bash" => "bash",
                "/bin/zsh" => "zsh",
                _ => "sh",
            };
            return DetectedShell {
                name,
                program: candidate.into(),
                args: vec!["-c"],
            };
        }
    }
    DetectedShell {
        name: "sh",
        program: "/bin/sh".into(),
        args: vec!["-c"],
    }
}

#[cfg(windows)]
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 进程树击杀(Windows 下 tokio kill 只杀直接子进程)。
pub async fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = tokio::process::Command::new("taskkill")
            .args(["/pid", &pid.to_string(), "/t", "/f"])
            .output()
            .await;
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
    }
}
