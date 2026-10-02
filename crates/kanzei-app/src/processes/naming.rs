//! 对话命名、类型与排序(UX-009 / UX-162)。
//!
//! 界面上的会话原来只叫 `p26` / 「默认」,既没法认也没法改名。现在的名字规则:
//! 用户命名(`sessions.title`)‖ 首条消息前 48 字(零 token,与 conversation.rs 的历史对话
//! 标题同口径)‖ 类型 + 序号(「主对话」「讨论 3」「独立任务 5」)。
//!
//! 纯函数全在这里(可单测);读 state.db 只有 [`load_naming`] 一处,调用方负责开库——
//! `process_list` 一次开库给全部线用,不为每条线各开一次。

use std::path::Path;

use kanzei_core::SessionStore;

/// 用户命名的字数上限(侧栏一行放得下,标题不是文章)。
pub(crate) const TITLE_MAX_CHARS: usize = 60;
/// 自动标题取首条消息的前多少字(conversation.rs 的历史对话标题同为 48)。
pub(crate) const AUTO_TITLE_CHARS: usize = 48;

/// 对话类型。判据只看进程 id 与持久 profile,与前端 `profile === "readonly"` 即「讨论」同一口径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessKind {
    Main,
    Discussion,
    Task,
    Research,
}

impl ProcessKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ProcessKind::Main => "main",
            ProcessKind::Discussion => "discussion",
            ProcessKind::Task => "task",
            ProcessKind::Research => "research",
        }
    }

    /// 界面用词(全站命名词表:主对话 / 讨论 / 独立任务;研究会话单独叫「研究」)。
    pub(crate) fn word(self) -> &'static str {
        match self {
            ProcessKind::Main => "主对话",
            ProcessKind::Discussion => "讨论",
            ProcessKind::Task => "独立任务",
            ProcessKind::Research => "研究",
        }
    }
}

pub(crate) fn process_kind(process_id: &str, profile: Option<&str>) -> ProcessKind {
    if process_id.starts_with("d|") {
        return ProcessKind::Main;
    }
    match profile {
        Some("research") => ProcessKind::Research,
        Some("readonly") => ProcessKind::Discussion,
        _ => ProcessKind::Task,
    }
}

/// 编号:`p12|<项目>` 里的 12。主对话(`d|…`)与其它形态没有。
pub(crate) fn process_ordinal(process_id: &str) -> Option<u64> {
    process_id
        .split('|')
        .next()?
        .strip_prefix('p')?
        .parse::<u64>()
        .ok()
}

/// 没有任何标题可用时的展示名:「主对话」「讨论 3」「独立任务 5」。
pub(crate) fn fallback_label(kind: ProcessKind, ordinal: Option<u64>) -> String {
    match (kind, ordinal) {
        (ProcessKind::Main, _) | (_, None) => kind.word().to_string(),
        (_, Some(ordinal)) => format!("{} {ordinal}", kind.word()),
    }
}

/// 线路顺序键:主对话在最前,其后 `pN` 按**数字**升序(p2 在 p10 之前),其它形态垫底、
/// 彼此按 id 字面序。原先按 id 字符串排,p10 会挤到 p2 前面(UX-162)。
pub(crate) fn order_key(process_id: &str) -> (u8, u64, &str) {
    if process_id.starts_with("d|") {
        return (0, 0, process_id);
    }
    match process_ordinal(process_id) {
        Some(ordinal) => (1, ordinal, process_id),
        None => (2, 0, process_id),
    }
}

/// 自动标题:首条消息里第一行有内容的文字,取前 [`AUTO_TITLE_CHARS`] 个字。
pub(crate) fn auto_title(prompt: &str) -> Option<String> {
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    Some(line.chars().take(AUTO_TITLE_CHARS).collect())
}

/// 校验并整理用户输入的名字:换行/制表折成空格;空白 = `None`(清除命名,回到自动名)。
pub(crate) fn clean_user_title(raw: &str) -> Result<Option<String>, String> {
    let flat = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return Ok(None);
    }
    if flat.chars().any(char::is_control) {
        return Err("名称不能包含控制字符".into());
    }
    if flat.chars().count() > TITLE_MAX_CHARS {
        return Err(format!("名称太长(最多 {TITLE_MAX_CHARS} 个字)"));
    }
    Ok(Some(flat))
}

/// 一条对话的命名事实(来自 state.db)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionNaming {
    /// 用户命名(`sessions.title`)。
    pub(crate) custom: Option<String>,
    /// 自动标题;已有用户命名、主对话、还没发过消息时都是 None。
    pub(crate) auto: Option<String>,
    /// `sessions.updated_at`(每次追加事件刷新);会话行还没建出来为 None。
    pub(crate) updated_at: Option<i64>,
}

impl SessionNaming {
    /// 有效标题:用户命名优先,其次自动标题。
    pub(crate) fn title(&self) -> Option<&str> {
        self.custom.as_deref().or(self.auto.as_deref())
    }

    /// 展示名:有标题用标题,否则类型 + 序号。永不为空。
    pub(crate) fn display(&self, kind: ProcessKind, ordinal: Option<u64>) -> String {
        match self.title() {
            Some(title) => title.to_string(),
            None => fallback_label(kind, ordinal),
        }
    }
}

/// 只读打开项目的 state.db 取命名事实;库不存在或打不开就是没有(调用方回落到类型 + 序号)。
pub(crate) fn open_naming_store(root: &Path) -> Option<SessionStore> {
    let path = kanzei_core::project_state_path(root);
    if !path.is_file() {
        return None;
    }
    SessionStore::open_read_only(&path).ok()
}

/// 读一条会话的命名事实。主对话不用首条消息当名字(它是「主对话」,不是某个历史话题)。
pub(crate) fn load_naming(
    store: &SessionStore,
    session_id: &str,
    kind: ProcessKind,
) -> SessionNaming {
    let session = store.get_session(session_id).ok().flatten();
    let custom = session
        .as_ref()
        .and_then(|session| session.title.clone())
        .filter(|title| !title.trim().is_empty());
    let auto = if custom.is_none() && kind != ProcessKind::Main {
        store
            .first_input_prompt(session_id)
            .ok()
            .flatten()
            .as_deref()
            .and_then(auto_title)
    } else {
        None
    };
    SessionNaming {
        custom,
        auto,
        updated_at: session.map(|session| session.updated_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 类型只看进程id与profile() {
        assert_eq!(process_kind("d|C:\\proj", None), ProcessKind::Main);
        // 主对话即便被设成只读 profile 也是主对话。
        assert_eq!(
            process_kind("d|C:\\proj", Some("readonly")),
            ProcessKind::Main
        );
        assert_eq!(
            process_kind("p3|C:\\proj", Some("readonly")),
            ProcessKind::Discussion
        );
        assert_eq!(
            process_kind("p4|C:\\proj", Some("research")),
            ProcessKind::Research
        );
        assert_eq!(process_kind("p5|C:\\proj", Some("dev")), ProcessKind::Task);
        assert_eq!(process_kind("p6|C:\\proj", None), ProcessKind::Task);
    }

    #[test]
    fn 编号取pn里的数字() {
        assert_eq!(process_ordinal("p12|C:\\a|b"), Some(12));
        assert_eq!(process_ordinal("d|C:\\proj"), None);
        assert_eq!(process_ordinal("pX|C:\\proj"), None);
        assert_eq!(process_ordinal("proj"), None);
    }

    #[test]
    fn 没有标题时回落到类型加序号且不再出现pn与默认() {
        assert_eq!(fallback_label(ProcessKind::Main, None), "主对话");
        assert_eq!(fallback_label(ProcessKind::Discussion, Some(3)), "讨论 3");
        assert_eq!(fallback_label(ProcessKind::Task, Some(26)), "独立任务 26");
        assert_eq!(fallback_label(ProcessKind::Task, None), "独立任务");
        for label in [
            fallback_label(ProcessKind::Main, None),
            fallback_label(ProcessKind::Discussion, Some(3)),
            fallback_label(ProcessKind::Task, Some(26)),
        ] {
            assert!(!label.contains('p') && !label.contains("默认"), "{label}");
        }
    }

    #[test]
    fn 线路按数字序排_主对话在最前() {
        let mut ids = vec![
            "p10|C:\\proj".to_string(),
            "p2|C:\\proj".to_string(),
            "x|odd".to_string(),
            "d|C:\\proj".to_string(),
            "p1|C:\\proj".to_string(),
            "p11|C:\\proj".to_string(),
        ];
        ids.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
        assert_eq!(
            ids,
            [
                "d|C:\\proj",
                "p1|C:\\proj",
                "p2|C:\\proj",
                "p10|C:\\proj",
                "p11|C:\\proj",
                "x|odd"
            ]
        );
    }

    #[test]
    fn 自动标题取首个有内容的行的前48字() {
        assert_eq!(
            auto_title("   \n\n  修一下登录页  \n第二行"),
            Some("修一下登录页".into())
        );
        assert_eq!(auto_title(" \n \t"), None);
        assert_eq!(auto_title(""), None);
        let long = "字".repeat(80);
        let title = auto_title(&long).unwrap();
        assert_eq!(title.chars().count(), AUTO_TITLE_CHARS);
    }

    #[test]
    fn 用户命名整理空白_空即清除_超长与控制字符被拒() {
        assert_eq!(
            clean_user_title("  方案  对照\n二 ").unwrap(),
            Some("方案 对照 二".into())
        );
        assert_eq!(clean_user_title("   ").unwrap(), None);
        assert_eq!(clean_user_title("").unwrap(), None);
        assert!(clean_user_title("a\u{7}b").is_err());
        assert!(clean_user_title(&"长".repeat(TITLE_MAX_CHARS + 1)).is_err());
        assert!(clean_user_title(&"长".repeat(TITLE_MAX_CHARS)).is_ok());
    }

    #[test]
    fn 展示名优先级_用户命名_自动标题_类型序号() {
        let custom = SessionNaming {
            custom: Some("方案对照".into()),
            auto: Some("不会用到".into()),
            updated_at: Some(1),
        };
        assert_eq!(custom.display(ProcessKind::Task, Some(7)), "方案对照");
        let auto = SessionNaming {
            custom: None,
            auto: Some("修登录页".into()),
            updated_at: None,
        };
        assert_eq!(auto.display(ProcessKind::Discussion, Some(2)), "修登录页");
        let none = SessionNaming::default();
        assert_eq!(none.display(ProcessKind::Discussion, Some(2)), "讨论 2");
        assert_eq!(none.title(), None);
    }

    #[test]
    fn 命名读取_主对话不用首条消息_讨论用首条消息_改名优先() {
        let store = SessionStore::open_in_memory().unwrap();
        store.create_session("ses_main", "C:/p", None).unwrap();
        store.create_session("ses_disc", "C:/p", None).unwrap();
        for session in ["ses_main", "ses_disc"] {
            store
                .admit_input(
                    session,
                    &format!("in-{session}"),
                    "帮我梳理一下需求\n再补充一行",
                    kanzei_core::Delivery::Queue,
                )
                .unwrap();
        }
        let main = load_naming(&store, "ses_main", ProcessKind::Main);
        assert_eq!(main.title(), None, "主对话叫「主对话」,不拿首条消息当名字");
        assert!(main.updated_at.is_some());
        let discussion = load_naming(&store, "ses_disc", ProcessKind::Discussion);
        assert_eq!(discussion.auto.as_deref(), Some("帮我梳理一下需求"));
        store
            .set_session_title("ses_disc", "C:/p", Some("需求梳理"))
            .unwrap();
        let renamed = load_naming(&store, "ses_disc", ProcessKind::Discussion);
        assert_eq!(renamed.custom.as_deref(), Some("需求梳理"));
        assert_eq!(renamed.auto, None, "有用户命名就不再读首条消息");
        // 会话行不存在:全空,调用方回落到类型 + 序号。
        assert_eq!(
            load_naming(&store, "ses_none", ProcessKind::Task),
            SessionNaming::default()
        );
    }

    /// `ProcessInfo`(`process_list` 的元素、`process_create`/`process_update` 的返回)的 IPC 形状。
    /// 新增的 label/title/title_custom/kind/ordinal/updated_at 前端侧栏与历史弹层直接读它们。
    #[test]
    fn process_info_形状与ipc契约一致() {
        let dir = std::env::temp_dir().join(format!(
            "kz-process-info-contract-{}-{}",
            std::process::id(),
            crate::run::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let root = crate::normalized_project_root(&dir);
        let state = crate::AppState::default();
        let process = crate::ensure_default_process(&state, &root);
        let info = crate::process_info(&state, &process);
        crate::ipc_contract::tests::check_contract(
            "process_info",
            crate::ipc_contract::shape(&serde_json::to_value(&info).unwrap()),
            "ProcessInfo 的 IPC 形状变了",
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
