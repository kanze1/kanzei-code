//! 冗余机械门禁域(R-155 B3):RedundancyWatch 就地提醒(R-100 模式 1 git 重复 /
//! 模式 2 全量测试白跑 / 模式 3 已知缺陷路径)。is_full_test_command /
//! defect_known_path_hint / trim_path_token / is_path_like 随域迁移;
//! is_git_query 双归属提 pub(crate) 于 metrics,此处显式导入。

use crate::runner::metrics::is_git_query;
use kanzei_llm::Part;

/// R-100 机械门禁:对可机械识别的冗余模式在工具结果中就地处提醒(不阻断,
/// 先观察后升级)。状态按单次运行持有——轮与轮之间不复用,避免跨轮误报。
///
/// 三种模式(全在工具结果文本追加 `[冗余提醒]` 前缀,summarize_metrics 按前缀计数):
/// 1. 同一工作树无变化时的重复 git status/diff:以上一次同类的工具结果内容为
///    工作树指纹,内容一致即判无变化;
/// 2. 无文件变更的重复全量测试:以上一次 git status/diff 的结果内容为指纹,
///    全量测试之间指纹未变即判白跑;
/// 3. 缺陷记录已含文件路径仍调 task:task prompt 里引用 D-xxx 且该缺陷条目
///    字段已含的路径也出现在 prompt 里,说明是在让子代理重新探索已知位置。
#[derive(Default)]
pub(crate) struct RedundancyWatch {
    /// 上一次 git status/diff 的结果内容(工作树指纹,None = 尚未见过)。
    last_git_content: Option<String>,
    /// 最近一次全量测试时的指纹。
    last_full_test_tree: Option<String>,
    /// 本轮是否已跑过全量测试。
    full_test_ran: bool,
}

impl RedundancyWatch {
    /// 在整步工具结果回喂前调用:`results` 与 `calls` 按下标一一对应
    /// (并行 wave 与串行路径都保持该对齐)。只追加、不改 is_error。
    pub(crate) fn note_step(
        &mut self,
        project_root: &std::path::Path,
        calls: &[(String, String, serde_json::Value, String)],
        results: &mut [Part],
    ) {
        // calls[i]↔results[i] 下标对齐不变式(R-155 设计要点 3):
        // 不变式跨 tool_exec/redundancy/drive 三文件,这里锁住调用方必须保持对齐,
        // 否则 results.get_mut(index) 会静默配错工具结果。
        debug_assert_eq!(calls.len(), results.len(), "工具调用与结果按下标一一对应");
        for (index, (_, name, input, _)) in calls.iter().enumerate() {
            let Some(Part::ToolResult { content, is_error, .. }) = results.get_mut(index) else {
                continue;
            };
            if *is_error || content.is_empty() {
                continue;
            }
            match name.as_str() {
                "bash" => {
                    // 先取原始内容再比较:提醒文本不能污染指纹,否则下次比较恒不相等。
                    let original = content.clone();
                    if is_git_query(input) {
                        if let Some(prev) = &self.last_git_content {
                            if prev == &original {
                                content.push_str(
                                    "\n[冗余提醒] 工作树与上次 git status/diff 无变化,这次查询可省",
                                );
                            }
                        }
                        self.last_git_content = Some(original);
                    } else {
                        let command = input
                            .get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_lowercase();
                        if is_full_test_command(&command) {
                            if self.full_test_ran {
                                if let (Some(cur), Some(prev)) =
                                    (&self.last_git_content, &self.last_full_test_tree)
                                {
                                    if cur == prev {
                                        content.push_str(
                                            "\n[冗余提醒] 自上次全量测试以来工作树无变更,这次测试可省",
                                        );
                                    }
                                }
                            }
                            self.last_full_test_tree = self.last_git_content.clone();
                            self.full_test_ran = true;
                        }
                    }
                }
                "task" => {
                    let prompt = input
                        .get("prompt")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if let Some(note) = defect_known_path_hint(project_root, prompt) {
                        content.push_str(&format!("\n[冗余提醒] {note}"));
                    }
                }
                _ => {}
            }
        }
    }
}

/// 全量测试判定:覆盖整个 workspace 的 cargo 测试命令。
/// `cargo test --workspace` 系列显式全量;不带 `-p` 的 `cargo test` 在工作区根
/// 跑的就是全量(把 -p 定向测试排除在外,定向不算全量)。
fn is_full_test_command(command: &str) -> bool {
    let c = command.to_lowercase();
    if !(c.contains("cargo test") || c.contains("cargo nextest")) {
        return false;
    }
    const FULL_FLAGS: &[&str] = &["--workspace", "--all", "--all-targets"];
    FULL_FLAGS.iter().any(|f| c.contains(f)) || !c.contains(" -p ")
}

/// R-100 模式 3:task prompt 引用缺陷 D-xxx 且该缺陷记录字段已含的路径也出现在
/// prompt 里 → 让子代理重新探索已知位置,就地提醒。纯文本解析,不依赖 docstore
/// (runner 层不能反向依赖 kanzei-tools,这是机械门禁的取舍)。
fn defect_known_path_hint(project_root: &std::path::Path, prompt: &str) -> Option<String> {
    if prompt.trim().is_empty() {
        return None;
    }
    let ids: Vec<&str> = prompt
        .split_whitespace()
        .filter(|w| {
            w.len() > 2
                && w.starts_with("D-")
                && w[2..].chars().all(|c| c.is_ascii_digit())
        })
        .collect();
    if ids.is_empty() {
        return None;
    }
    for name in ["defects.md", "defects-archive.md"] {
        let path = project_root.join(".kanzei/project").join(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for id in &ids {
            let marker = format!("## {id} ");
            let Some(start) = text.find(&marker) else {
                continue;
            };
            let rest = &text[start..];
            let end = rest
                .find("\n## ")
                .map(|i| start + i)
                .unwrap_or(text.len());
            let section = &text[start..end];
            let known: Vec<&str> = section
                .split_whitespace()
                .map(trim_path_token)
                .filter(|w| is_path_like(w))
                .collect();
            for known_path in known {
                if prompt.contains(known_path) {
                    return Some(format!(
                        "缺陷 {id} 记录已含文件路径 {known_path},直接 read 该文件即可,无需 task 重新探索"
                    ));
                }
            }
        }
    }
    None
}

/// 去掉路径 token 首尾的标点(截断、括号、分号等)。
fn trim_path_token(token: &str) -> &str {
    let mut s = token.trim();
    while let Some(last) = s.chars().last() {
        if ".,;:!?)]}、。；：」』》".contains(last) {
            s = &s[..s.len() - last.len_utf8()];
        } else {
            break;
        }
    }
    s
}

/// 路径样子:含目录分隔符且含点(代码文件/相对路径),排除纯 URL。
fn is_path_like(token: &str) -> bool {
    let has_sep = token.contains('/') || token.contains('\\');
    let has_dot = token.contains('.');
    let not_url = !token.contains("://");
    has_sep && has_dot && not_url
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::testutil::result_content;

    #[test]
    fn 重复_git_status_无变化时_就地提醒() {
        let mut watch = RedundancyWatch::default();
        let dir = std::env::temp_dir().join(format!("kz-red-git-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();

        let calls = vec![
            ("g1".into(), "bash".into(), serde_json::json!({"command": "git status --porcelain"}), "".into()),
            ("g2".into(), "bash".into(), serde_json::json!({"command": "git status --porcelain"}), "".into()),
        ];
        let mut results = vec![
            Part::ToolResult { call_id: "g1".into(), content: " M src/lib.rs".into(), is_error: false },
            Part::ToolResult { call_id: "g2".into(), content: " M src/lib.rs".into(), is_error: false },
        ];
        watch.note_step(&dir, &calls, &mut results);
        assert!(!result_content(&results[0]).contains("[冗余提醒]"));
        assert!(result_content(&results[1]).contains("[冗余提醒]"), "{}", result_content(&results[1]));
        // 内容变了(工作树有改动)就不再提醒。
        let mut watch2 = RedundancyWatch::default();
        let mut results2 = vec![
            Part::ToolResult { call_id: "g1".into(), content: " M src/lib.rs".into(), is_error: false },
            Part::ToolResult { call_id: "g2".into(), content: " M src/lib.rs\n M src/app.rs".into(), is_error: false },
        ];
        watch2.note_step(&dir, &calls, &mut results2);
        assert!(!result_content(&results2[1]).contains("[冗余提醒]"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn 全量测试_工作树未变时_就地提醒() {
        let mut watch = RedundancyWatch::default();
        let dir = std::env::temp_dir().join(format!("kz-red-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();

        let tree = " M crates/kanzei-app/ui/main.js";
        let calls1 = vec![
            ("g1".into(), "bash".into(), serde_json::json!({"command": "git status --porcelain"}), "".into()),
            ("b1".into(), "bash".into(), serde_json::json!({"command": "cargo test --workspace"}), "".into()),
        ];
        let mut results1 = vec![
            Part::ToolResult { call_id: "g1".into(), content: tree.into(), is_error: false },
            Part::ToolResult { call_id: "b1".into(), content: "test result: ok. 200 passed".into(), is_error: false },
        ];
        watch.note_step(&dir, &calls1, &mut results1);
        assert!(!result_content(&results1[1]).contains("[冗余提醒]"), "首次全量测试不该提醒");

        // 第二次:git status 内容一致,再跑全量 → 提醒。
        let calls2 = vec![
            ("g2".into(), "bash".into(), serde_json::json!({"command": "git status --porcelain"}), "".into()),
            ("b2".into(), "bash".into(), serde_json::json!({"command": "cargo test --workspace"}), "".into()),
        ];
        let mut results2 = vec![
            Part::ToolResult { call_id: "g2".into(), content: tree.into(), is_error: false },
            Part::ToolResult { call_id: "b2".into(), content: "test result: ok. 200 passed".into(), is_error: false },
        ];
        watch.note_step(&dir, &calls2, &mut results2);
        assert!(result_content(&results2[1]).contains("[冗余提醒]"), "{}", result_content(&results2[1]));
        // 定向测试不算全量,不触发。
        let mut watch3 = RedundancyWatch::default();
        let calls3 = vec![("b3".into(), "bash".into(), serde_json::json!({"command": "cargo test -p kanzei-core"}), "".into())];
        let mut results3 = vec![Part::ToolResult { call_id: "b3".into(), content: "ok".into(), is_error: false }];
        watch3.note_step(&dir, &calls3, &mut results3);
        assert!(!result_content(&results3[0]).contains("[冗余提醒]"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn task_引用已知缺陷路径时_就地提醒() {
        let dir = std::env::temp_dir().join(format!("kz-red-task-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::write(
            dir.join(".kanzei/project/defects.md"),
            "# Defects\n\n## D-001 启动黑屏 [open]\n- 复现: crates/kanzei-app/ui/main.js 初始化\n",
        )
        .unwrap();
        let mut watch = RedundancyWatch::default();
        let calls = vec![(
            "t1".into(),
            "task".into(),
            serde_json::json!({"prompt": "D-001 启动黑屏,找 crates/kanzei-app/ui/main.js 的初始化位置"}),
            "".into(),
        )];
        let mut results = vec![Part::ToolResult { call_id: "t1".into(), content: "done".into(), is_error: false }];
        watch.note_step(&dir, &calls, &mut results);
        assert!(result_content(&results[0]).contains("[冗余提醒] 缺陷 D-001"), "{}", result_content(&results[0]));
        // 路径不在缺陷记录里 → 不提醒。
        let calls2 = vec![(
            "t2".into(),
            "task".into(),
            serde_json::json!({"prompt": "D-001 启动黑屏,找 crates/kanzei-app/src/main.rs 的逻辑"}),
            "".into(),
        )];
        let mut results2 = vec![Part::ToolResult { call_id: "t2".into(), content: "done".into(), is_error: false }];
        watch.note_step(&dir, &calls2, &mut results2);
        assert!(!result_content(&results2[0]).contains("[冗余提醒]"));
        std::fs::remove_dir_all(dir).ok();
    }
}

