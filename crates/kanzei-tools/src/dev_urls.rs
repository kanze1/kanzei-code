//! 本地开发服务地址发现(UI2-0926 #8,docs/design/preview_pane.md §3「空态」)。
//!
//! 预览面板的空态要列出 kanzei 自己起的 dev server(`bash` 后台模式 / `process`),
//! 点一下就打开。来源只有后台进程输出:Vite 的 `➜  Local:   http://localhost:5173/`、
//! Next 的 `- Local: http://localhost:3000`、`python -m http.server` 的
//! `http://0.0.0.0:8000/` 等。只认回环与 0.0.0.0(映射成 localhost),局域网地址不列。
//!
//! `background` 模块对外私有,这里是它开给预览面板的唯一窄接口。

use std::path::Path;
use std::sync::OnceLock;

use serde::Serialize;

/// 只读后台进程输出的末尾这么多字节:dev server 的地址行在启动时打印,
/// 之后的 HMR 日志可能很长,但最近的重启也会重新打印地址。
const OUTPUT_TAIL_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DevUrl {
    pub url: String,
    pub command: String,
    pub pid: Option<u32>,
}

/// 项目内还在运行的后台进程里检测到的本地开发服务地址(按 origin 去重,先到先得)。
pub fn dev_server_urls(project_root: &Path) -> Vec<DevUrl> {
    let processes = crate::background::list(project_root);
    collect(processes.iter().map(|process| ProcessView {
        running: process.is_running(),
        command: process.command.clone(),
        pid: process.pid(),
        output: if process.persistent {
            process.full_log()
        } else {
            process.output()
        },
    }))
}

/// 纯判定用的进程视图(单测直接造)。
pub(crate) struct ProcessView {
    pub(crate) running: bool,
    pub(crate) command: String,
    pub(crate) pid: Option<u32>,
    pub(crate) output: String,
}

pub(crate) fn collect(processes: impl Iterator<Item = ProcessView>) -> Vec<DevUrl> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for process in processes.filter(|process| process.running) {
        for url in extract_urls(tail(&process.output, OUTPUT_TAIL_BYTES)) {
            if seen.insert(origin_of(&url)) {
                out.push(DevUrl {
                    url,
                    command: process.command.clone(),
                    pid: process.pid,
                });
            }
        }
    }
    out
}

fn tail(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// 去掉 ANSI 转义(CSI 着色 / 光标控制与 OSC 超链接)。
pub fn strip_ansi(text: &str) -> String {
    static ANSI: OnceLock<regex::Regex> = OnceLock::new();
    let pattern = ANSI.get_or_init(|| {
        regex::Regex::new(
            r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]",
        )
        .expect("ANSI 正则")
    });
    pattern.replace_all(text, "").into_owned()
}

/// 从一段输出里抽回环开发服务地址:0.0.0.0 映射为 localhost,补全尾斜杠,
/// 去掉粘在末尾的标点。保持出现顺序,同一 URL 只留一次。
pub fn extract_urls(text: &str) -> Vec<String> {
    static URL: OnceLock<regex::Regex> = OnceLock::new();
    let pattern = URL.get_or_init(|| {
        regex::Regex::new(
            r"(https?)://(localhost|127\.0\.0\.1|\[::1\]|0\.0\.0\.0)(:\d{2,5})?(/[^\s'\x22<>`]*)?",
        )
        .expect("URL 正则")
    });
    let clean = strip_ansi(text);
    let mut out: Vec<String> = Vec::new();
    for captures in pattern.captures_iter(&clean) {
        let scheme = &captures[1];
        let host = match &captures[2] {
            "0.0.0.0" => "localhost",
            other => other,
        };
        let port = captures.get(3).map(|m| m.as_str()).unwrap_or("");
        if let Some(number) = port.strip_prefix(':') {
            if number.parse::<u32>().map_or(true, |n| n == 0 || n > 65535) {
                continue;
            }
        }
        let path = captures
            .get(4)
            .map(|m| {
                m.as_str()
                    .trim_end_matches([',', '.', ';', ')', ']', '}', '!', '?'])
            })
            .filter(|path| !path.is_empty())
            .unwrap_or("/");
        let url = format!("{scheme}://{host}{port}{path}");
        if !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

fn origin_of(url: &str) -> String {
    let after_scheme = url.find("://").map(|index| index + 3).unwrap_or(0);
    match url[after_scheme..].find('/') {
        Some(index) => url[..after_scheme + index].to_string(),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(running: bool, command: &str, output: &str) -> ProcessView {
        ProcessView {
            running,
            command: command.into(),
            pid: Some(42),
            output: output.into(),
        }
    }

    #[test]
    fn 常见开发服务的地址行都能抽出() {
        let vite = "\n  VITE v5.4.0  ready in 312 ms\n\n  ➜  Local:   http://localhost:5173/\n  ➜  Network: http://192.168.1.8:5173/\n";
        assert_eq!(extract_urls(vite), vec!["http://localhost:5173/"]);
        let next = "   ▲ Next.js 14.2.3\n   - Local:        http://localhost:3000\n";
        assert_eq!(extract_urls(next), vec!["http://localhost:3000/"]);
        let python = "Serving HTTP on 0.0.0.0 port 8000 (http://0.0.0.0:8000/) ...";
        assert_eq!(extract_urls(python), vec!["http://localhost:8000/"]);
        let ipv6 = "listening on http://[::1]:4173/app, ready.";
        assert_eq!(extract_urls(ipv6), vec!["http://[::1]:4173/app"]);
        assert!(extract_urls("http://localhost:99999/ bogus port").is_empty());
    }

    #[test]
    fn ansi着色被剥掉() {
        let colored = "  \x1b[32m➜\x1b[39m  \x1b[1mLocal\x1b[22m:   \x1b[36mhttp://localhost:\x1b[1m5173\x1b[22m/\x1b[39m\n";
        assert_eq!(extract_urls(colored), vec!["http://localhost:5173/"]);
        let osc = "\x1b]8;;http://localhost:3000\x07http://localhost:3000\x1b]8;;\x07";
        assert_eq!(extract_urls(osc), vec!["http://localhost:3000/"]);
    }

    #[test]
    fn 按origin去重且已退出进程忽略() {
        let urls = collect(
            vec![
                view(false, "npm run dev -- --port 4000", "http://localhost:4000/"),
                view(
                    true,
                    "npm run dev",
                    "Local: http://localhost:5173/\nagain http://localhost:5173/about\nhttp://127.0.0.1:5173/",
                ),
                view(true, "python -m http.server", "http://0.0.0.0:5173/"),
            ]
            .into_iter(),
        );
        assert_eq!(
            urls,
            vec![
                DevUrl {
                    url: "http://localhost:5173/".into(),
                    command: "npm run dev".into(),
                    pid: Some(42),
                },
                DevUrl {
                    url: "http://127.0.0.1:5173/".into(),
                    command: "npm run dev".into(),
                    pid: Some(42),
                },
            ]
        );
    }

    #[test]
    fn 只看输出尾部且按字符边界切() {
        let long = format!("{}http://localhost:1111/\n{}", "中".repeat(40_000), "x");
        assert!(tail(&long, OUTPUT_TAIL_BYTES).len() <= OUTPUT_TAIL_BYTES);
        assert_eq!(collect(vec![view(true, "a", &long)].into_iter()).len(), 1);
    }
}
