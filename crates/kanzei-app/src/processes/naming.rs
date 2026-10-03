//! 对话命名、类型与排序(UX-009 / UX-162)。
//!
//! 界面上的会话原来只叫 `p26` / 「默认」,既没法认也没法改名。现在的名字规则:
//! 用户命名(`sessions.title`)‖ 首条消息前 48 字(零 token,与 conversation.rs 的历史对话
//! 标题同口径)‖ 类型 + 序号(「对话」「讨论 3」「对话 5」)。
//!
//! 纯函数全在这里(可单测);读 state.db 只有 [`load_naming`] 一处,调用方负责开库——
//! `process_list` 一次开库给全部线用,不为每条线各开一次。

use std::path::Path;

use kanzei_core::SessionStore;

/// 用户命名的字数上限(侧栏一行放得下,标题不是文章)。
pub(crate) const TITLE_MAX_CHARS: usize = 60;
/// 自动标题取首条消息的前多少字(conversation.rs 的历史对话标题同为 48)。
pub(crate) const AUTO_TITLE_CHARS: usize = 48;

/// 对话类型。判据只看持久 profile,与前端 `profile === "readonly"` 即「讨论」同一口径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessKind {
    Discussion,
    Conversation,
    Research,
}

impl ProcessKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ProcessKind::Discussion => "discussion",
            ProcessKind::Conversation => "conversation",
            ProcessKind::Research => "research",
        }
    }

    /// 界面用词(全站命名词表:对话 / 讨论;研究会话单独叫「研究」)。
    pub(crate) fn word(self) -> &'static str {
        match self {
            ProcessKind::Discussion => "讨论",
            ProcessKind::Conversation => "对话",
            ProcessKind::Research => "研究",
        }
    }
}

pub(crate) fn process_kind(_process_id: &str, profile: Option<&str>) -> ProcessKind {
    match profile {
        Some("research") => ProcessKind::Research,
        Some("readonly") => ProcessKind::Discussion,
        _ => ProcessKind::Conversation,
    }
}

/// 编号:`p12|<项目>` 里的 12。旧格式(`d|…`)与其它形态没有。
pub(crate) fn process_ordinal(process_id: &str) -> Option<u64> {
    process_id
        .split('|')
        .next()?
        .strip_prefix('p')?
        .parse::<u64>()
        .ok()
}

/// 没有任何标题可用时的展示名:「对话」「讨论 3」「对话 5」。
pub(crate) fn fallback_label(kind: ProcessKind, ordinal: Option<u64>) -> String {
    match (kind, ordinal) {
        (_, None) => kind.word().to_string(),
        (_, Some(ordinal)) => format!("{} {ordinal}", kind.word()),
    }
}

/// 线路顺序键:`pN` 按**数字**升序(p2 在 p10 之前),其它形态垫底、
/// 彼此按 id 字面序。原先按 id 字符串排,p10 会挤到 p2 前面(UX-162)。
pub(crate) fn order_key(process_id: &str) -> (u8, u64, &str) {
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
    /// 自动标题;已有用户命名或还没发过消息时为 None。
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

/// 所有对话都优先展示用户实际发出的内容，执行角色不再覆盖话题名称。
pub(crate) fn load_naming(
    store: &SessionStore,
    session_id: &str,
    _kind: ProcessKind,
) -> SessionNaming {
    let session = store.get_session(session_id).ok().flatten();
    let custom = session
        .as_ref()
        .and_then(|session| session.title.clone())
        .map(|title| title.trim().to_string())
        .filter(|title| !title.trim().is_empty());
    let auto = if custom.is_none() {
        let input = store
            .first_input_prompt_with_sequence(session_id)
            .ok()
            .flatten()
            .and_then(|(sequence, prompt)| auto_title(&prompt).map(|title| (sequence, title)));
        input
            .into_iter()
            .chain(history_auto_title(store, session_id))
            .min_by_key(|(sequence, _)| *sequence)
            .map(|(_, title)| title)
    } else {
        None
    };
    SessionNaming {
        custom,
        auto,
        updated_at: session.map(|session| session.updated_at),
    }
}

/// 旧桌面快照和手机消息没有 inbox 行，仍需从已有用户消息回填话题。
/// 只读当前段的相关事件，避免 reset/删除当前段后重新显示旧标题。
fn history_auto_title(store: &SessionStore, session_id: &str) -> Option<(i64, String)> {
    let floor = store
        .conversation_floor(session_id)
        .ok()
        .flatten()
        .unwrap_or(0);
    [
        "conversation.updated",
        "session.user_message_committed",
        "session.steering_message_committed",
    ]
    .into_iter()
    .filter_map(|event_type| {
        let mut after = floor;
        while let Some(event) = store
            .first_event_by_type_after(session_id, after, event_type)
            .ok()
            .flatten()
        {
            let title = if event_type == "conversation.updated" {
                serde_json::from_value::<Vec<kanzei_llm::Message>>(
                    event.payload["messages"].clone(),
                )
                .ok()
                .and_then(|messages| messages_auto_title(&messages))
            } else {
                serde_json::from_value::<kanzei_llm::Message>(
                    event.payload["fact"]["message"].clone(),
                )
                .ok()
                .and_then(|message| messages_auto_title(&[message]))
            };
            if let Some(title) = title {
                return Some((event.sequence, title));
            }
            after = event.sequence;
        }
        None
    })
    .min_by_key(|(sequence, _)| *sequence)
}

fn messages_auto_title(messages: &[kanzei_llm::Message]) -> Option<String> {
    messages
        .iter()
        .filter(|message| message.role == kanzei_llm::Role::User)
        .find_map(|message| {
            message.parts.iter().find_map(|part| match part {
                kanzei_llm::Part::Text { text } => auto_title(text),
                _ => None,
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 类型只看进程id与profile() {
        assert_eq!(process_kind("d|C:\\proj", None), ProcessKind::Conversation);
        // 旧格式与新格式都按显式 profile 确定模式。
        assert_eq!(
            process_kind("d|C:\\proj", Some("readonly")),
            ProcessKind::Discussion
        );
        assert_eq!(
            process_kind("p3|C:\\proj", Some("readonly")),
            ProcessKind::Discussion
        );
        assert_eq!(
            process_kind("p4|C:\\proj", Some("research")),
            ProcessKind::Research
        );
        assert_eq!(
            process_kind("p5|C:\\proj", Some("dev")),
            ProcessKind::Conversation
        );
        assert_eq!(process_kind("p6|C:\\proj", None), ProcessKind::Conversation);
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
        assert_eq!(fallback_label(ProcessKind::Conversation, None), "对话");
        assert_eq!(fallback_label(ProcessKind::Discussion, Some(3)), "讨论 3");
        assert_eq!(
            fallback_label(ProcessKind::Conversation, Some(26)),
            "对话 26"
        );
        assert_eq!(fallback_label(ProcessKind::Conversation, None), "对话");
        for label in [
            fallback_label(ProcessKind::Conversation, None),
            fallback_label(ProcessKind::Discussion, Some(3)),
            fallback_label(ProcessKind::Conversation, Some(26)),
        ] {
            assert!(!label.contains('p') && !label.contains("默认"), "{label}");
        }
    }

    #[test]
    fn 线路按数字序排_旧格式没有特殊顺位() {
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
                "p1|C:\\proj",
                "p2|C:\\proj",
                "p10|C:\\proj",
                "p11|C:\\proj",
                "d|C:\\proj",
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
        assert_eq!(
            custom.display(ProcessKind::Conversation, Some(7)),
            "方案对照"
        );
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
    fn 命名读取_所有对话用首条消息_改名优先() {
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
        let main = load_naming(&store, "ses_main", ProcessKind::Conversation);
        assert_eq!(main.title(), Some("帮我梳理一下需求"));
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
            load_naming(&store, "ses_none", ProcessKind::Conversation),
            SessionNaming::default()
        );
    }

    #[test]
    fn 存量快照与手机事实也回填标题_清空后不复用旧话题() {
        use kanzei_llm::Message;
        let store = SessionStore::open_in_memory().unwrap();
        store.create_session("ses_legacy", "C:/p", None).unwrap();
        store
            .create_session("ses_mobile", "C:/general", None)
            .unwrap();
        store
            .append_event(
                "ses_legacy",
                "conversation.updated",
                &serde_json::json!({
                    "messages": [Message::user_text("\n  存量对话的话题\n还有一些细节")]
                }),
            )
            .unwrap();
        store
            .append_mobile_message(
                "ses_mobile",
                "phone",
                Message::user_text("手机发来的实际问题"),
                &serde_json::json!({}),
            )
            .unwrap();
        assert_eq!(
            load_naming(&store, "ses_legacy", ProcessKind::Conversation).title(),
            Some("存量对话的话题")
        );
        assert_eq!(
            load_naming(&store, "ses_mobile", ProcessKind::Discussion).title(),
            Some("手机发来的实际问题")
        );
        for (session, title) in [
            ("ses_legacy", "存量对话的话题"),
            ("ses_mobile", "手机发来的实际问题"),
        ] {
            store
                .admit_input(
                    session,
                    &format!("desktop-{session}"),
                    "桌面继续补充",
                    kanzei_core::Delivery::Queue,
                )
                .unwrap();
            assert_eq!(
                load_naming(&store, session, ProcessKind::Conversation).title(),
                Some(title),
                "后续 inbox 输入不能覆盖更早的实际用户消息"
            );
        }
        for session in ["ses_legacy", "ses_mobile"] {
            store
                .append_event(session, "conversation.reset", &serde_json::json!({}))
                .unwrap();
            assert_eq!(
                load_naming(&store, session, ProcessKind::Conversation).title(),
                None
            );
            store
                .admit_input(
                    session,
                    &format!("new-{session}"),
                    "清空后的新话题",
                    kanzei_core::Delivery::Queue,
                )
                .unwrap();
            assert_eq!(
                load_naming(&store, session, ProcessKind::Conversation).title(),
                Some("清空后的新话题")
            );
        }
        store
            .set_session_title("ses_mobile", "C:/general", Some("我起的名字"))
            .unwrap();
        assert_eq!(
            load_naming(&store, "ses_mobile", ProcessKind::Conversation).title(),
            Some("我起的名字")
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
