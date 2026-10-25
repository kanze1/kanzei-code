//! 完成声明保留范围与目标;局部交付不能扩大成整次委托结束。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HandoffScope {
    /// 指定工作项的局部交付。
    WorkItem,
    /// 指定阶段的局部交付，不要求固定批次数量。
    Batch,
    /// 本次请求完成，独立目标和已授权队列分别判断。
    Request,
    /// 当前目标完成。
    Goal,
}

/// 本次运行实际使用的工作来源，与是否存在独立目标分开表达。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionMode {
    Request,
    ProjectQueue,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct HandoffDeclaration {
    #[serde(rename = "handoff_scope")]
    pub scope: HandoffScope,
    #[serde(rename = "handoff_target")]
    pub target: String,
    pub criterion: String,
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub summary: Option<String>,
}

impl HandoffDeclaration {
    pub fn from_input(input: &serde_json::Value) -> Option<Self> {
        let declaration: Self = serde_json::from_value(input.clone()).ok()?;
        if declaration.target.trim().is_empty()
            || declaration.criterion.trim().is_empty()
            || declaration.evidence_refs.is_empty()
            || declaration
                .evidence_refs
                .iter()
                .any(|item| item.trim().is_empty())
        {
            return None;
        }
        Some(declaration)
    }

    /// 轮末再核对身份,防止模型把子事项、旧请求或已被用户改写的目标当成整体完成。
    pub fn bound_scope(&self, request_id: &str, goal: Option<&str>) -> Option<HandoffScope> {
        match self.scope {
            HandoffScope::WorkItem | HandoffScope::Batch => Some(self.scope),
            HandoffScope::Request if self.target == request_id => Some(self.scope),
            HandoffScope::Goal if goal.is_some_and(|goal| self.target == goal) => Some(self.scope),
            _ => None,
        }
    }
}

/// 回执与上下文共用的表达说明，不妨碍用户主动查询内部记录。
pub const INTERNAL_RECORD_NOTICE: &str =
    "完成声明用于内部记录。向用户说明实际成果、验证情况和剩余工作，不例行复述内部字段或输入标识。";

/// 仅向具备 work 工具的开发档提供完成记录说明。
/// 调用方按真实的推进开关和工作来源传入模式，不从 Agent 名称推断队列授权。
pub fn context_prompt_for(
    profile: crate::ProfileKind,
    request_id: &str,
    goal: Option<&str>,
    mode: CompletionMode,
) -> Option<String> {
    if !profile.is_development() {
        return None;
    }
    let continuation = match mode {
        CompletionMode::Request => "本次围绕当前请求及已设置的目标推进。",
        CompletionMode::ProjectQueue => {
            "本次已启用项目队列推进，请求级完成不结束其余已授权的队列工作。"
        }
    };
    let identity = serde_json::json!({"request_id":request_id,"goal":goal});
    Some(format!(
        "\n\n完成记录：用 work handoff 记录实际完成的范围、条件和证据。work_item/batch 记录局部交付；request 的目标为下列 request_id，goal 的目标为下列目标原文。请求完成与独立目标完成分别判断。\n{continuation}\n按用户要求判断完成，包括已要求的发布或安装；阶段成果使用局部范围。\n{identity}\n{INTERNAL_RECORD_NOTICE}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_scope_and_empty_evidence_cannot_become_completion_signals() {
        let mut input =
            json!({"handoff_target":"R-379","criterion":"source ready","evidence_refs":["test:1"]});
        assert!(HandoffDeclaration::from_input(&input).is_none());
        input["handoff_scope"] = json!("work_item");
        assert!(HandoffDeclaration::from_input(&input).is_some());
        input["evidence_refs"] = json!([" "]);
        assert!(HandoffDeclaration::from_input(&input).is_none());
    }

    /// 完成身份保持可见，表达说明不要求例行复述内部记录。
    #[test]
    fn 契约提示要求不向用户复述内部字段() {
        let prompt = context_prompt_for(
            crate::ProfileKind::DevPair,
            "input-a",
            Some("publish"),
            CompletionMode::Request,
        )
        .unwrap();
        assert!(prompt.contains(INTERNAL_RECORD_NOTICE));
        // 约束不能吃掉契约本体:输入 id 与目标原文仍要交给模型。
        assert!(prompt.contains("input-a"));
        assert!(prompt.contains("\"publish\""));
    }

    /// UX-008:只读讨论与研究档没有 work 工具、轮末也不消费 handoff,不注入契约。
    #[test]
    fn 契约只注入开发档() {
        use crate::ProfileKind;
        let dev = context_prompt_for(
            ProfileKind::DevAuto,
            "input-a",
            None,
            CompletionMode::ProjectQueue,
        )
        .unwrap();
        assert!(dev.contains("已启用项目队列"));
        assert!(dev.contains(INTERNAL_RECORD_NOTICE));
        for profile in [
            ProfileKind::Readonly,
            ProfileKind::Research,
            ProfileKind::Isolated,
        ] {
            assert_eq!(
                context_prompt_for(profile, "input-a", None, CompletionMode::Request),
                None,
                "{profile:?} 不应带完成声明契约"
            );
        }
        for profile in [ProfileKind::DevAuto, ProfileKind::DevPair] {
            let request =
                context_prompt_for(profile, "input-a", None, CompletionMode::Request).unwrap();
            assert!(!request.contains("已启用项目队列"));
            assert!(request.contains("当前请求"));
            assert!(request.contains("input-a"));
        }
    }

    #[test]
    fn completion_is_bound_to_current_request_or_unchanged_goal() {
        let mut declaration = HandoffDeclaration::from_input(&json!({"handoff_scope":"request","handoff_target":"input-a","criterion":"done","evidence_refs":["test:1"]})).unwrap();
        assert_eq!(
            declaration.bound_scope("input-a", None),
            Some(HandoffScope::Request)
        );
        assert_eq!(declaration.bound_scope("input-b", None), None);
        assert_eq!(
            declaration.bound_scope("input-a", Some("publish")),
            Some(HandoffScope::Request)
        );
        declaration.scope = HandoffScope::Goal;
        declaration.target = "publish".into();
        assert_eq!(declaration.bound_scope("input-a", None), None);
        assert_eq!(
            declaration.bound_scope("input-a", Some("publish")),
            Some(HandoffScope::Goal)
        );
        assert_eq!(
            declaration.bound_scope("input-a", Some("publish and install")),
            None
        );
    }
}
