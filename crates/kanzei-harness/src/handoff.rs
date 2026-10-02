//! 完成声明保留范围与目标;局部交付不能扩大成整次委托结束。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HandoffScope {
    WorkItem,
    Batch,
    Request,
    Goal,
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
            HandoffScope::Request if goal.is_none() && self.target == request_id => {
                Some(self.scope)
            }
            HandoffScope::Goal if goal.is_some_and(|goal| self.target == goal) => Some(self.scope),
            _ => None,
        }
    }
}

/// 完成声明字段的「内部记录」约束(UX-008)。
///
/// 这些字段只给轮末控制器与交付记录用;模型把它们当成「要向用户交代的内容」逐项复述
/// (交接范围/目标/验收标准/证据),用户读到的就是引擎内部术语与输入 id。系统提示、
/// `work` 工具描述与成功回执共用这一句,三处措辞不各写各的。
pub const INTERNAL_RECORD_NOTICE: &str = "The completion fields (handoff_scope, handoff_target, criterion, evidence_refs) and input ids are internal records for the controller and the delivery log. Never repeat, translate or list them in a reply to the user; say in plain language what was done, how it was verified and what remains.";

pub fn context_prompt(request_id: &str, goal: Option<&str>) -> String {
    format!(
        "\n\nCompletion contract: work handoff must provide handoff_scope and handoff_target, criterion and evidence_refs. \
         work_item targets the item id; batch targets the batch label; both record local progress and do not stop delegation. \
         request targets this exact admitted input id: {request_id}. goal targets the exact active goal text in this JSON: {}. \
         An autonomous queue continues while authorized work remains, even after a request handoff. \
         Never narrow the user's completion criterion: requested publication or installation must be finished before declaring the request/goal complete; use a local scope for intermediate delivery. \
         {INTERNAL_RECORD_NOTICE}",
        serde_json::to_string(&goal).unwrap_or_default()
    )
}

/// 按档位决定是否注入完成声明契约:只有开发档注入。
///
/// `work` 工具只存在于开发档(profiles/dev.rs),也只有开发档的轮末会消费 handoff
/// (研究档把 completion 置空走课题工作流,只读讨论不自动续跑)。其它档位读到这段契约
/// 既没有工具可调,又被告知「要交代范围、目标、证据」,模型便把这些内部字段当散文讲给
/// 用户(UX-008)——所以不注入,而不是注入后再靠「勿复述」兜底。
pub fn context_prompt_for(
    profile: crate::ProfileKind,
    request_id: &str,
    goal: Option<&str>,
) -> Option<String> {
    (profile == crate::ProfileKind::Dev).then(|| context_prompt(request_id, goal))
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

    /// UX-008:契约提示必须明确告诉模型这些字段是内部记录、不得复述给用户。
    #[test]
    fn 契约提示要求不向用户复述内部字段() {
        let prompt = context_prompt("input-a", Some("publish"));
        assert!(prompt.contains(INTERNAL_RECORD_NOTICE));
        for field in [
            "handoff_scope",
            "handoff_target",
            "criterion",
            "evidence_refs",
        ] {
            assert!(
                INTERNAL_RECORD_NOTICE.contains(field),
                "约束要点名每个内部字段: {field}"
            );
        }
        assert!(INTERNAL_RECORD_NOTICE.contains("Never repeat"));
        // 约束不能吃掉契约本体:输入 id 与目标原文仍要交给模型。
        assert!(prompt.contains("input-a"));
        assert!(prompt.contains("\"publish\""));
    }

    /// UX-008:只读讨论与研究档没有 work 工具、轮末也不消费 handoff,不注入契约。
    #[test]
    fn 契约只注入开发档() {
        use crate::ProfileKind;
        let dev = context_prompt_for(ProfileKind::Dev, "input-a", None).unwrap();
        assert!(dev.contains("Completion contract"));
        assert!(dev.contains(INTERNAL_RECORD_NOTICE));
        for profile in [ProfileKind::Readonly, ProfileKind::Research] {
            assert_eq!(
                context_prompt_for(profile, "input-a", None),
                None,
                "{profile:?} 不应带完成声明契约"
            );
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
        assert_eq!(declaration.bound_scope("input-a", Some("publish")), None);
        declaration.scope = HandoffScope::Goal;
        declaration.target = "publish".into();
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
