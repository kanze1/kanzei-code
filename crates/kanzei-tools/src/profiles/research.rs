use std::sync::Arc;

use kanzei_harness::{rule, Effect, HarnessDraft};

use crate::tracker::{ResearchTrackerTool, TrackerTool};
use kanzei_memory::docstore::{DEFECTS, FINDINGS, REQUIREMENTS, SOURCES};

/// Research tools and project draft feedback, composed by ResearchProfile.
pub(crate) fn register_tools(draft: &mut HarnessDraft) {
    draft.tools.insert(
        "source",
        Arc::new(TrackerTool {
            tool_name: "source",
            noun: "source",
            kind: &SOURCES,
            requires_refs: None,
        }),
    );
    draft.tools.insert(
        "websearch",
        Arc::new(crate::websearch::ResearchWebSearchTool),
    );
    draft
        .tools
        .insert("webfetch", Arc::new(crate::webfetch::ResearchWebFetchTool));
    draft.tools.insert(
        "research_plan",
        Arc::new(crate::research_plan::ResearchPlanTool),
    );
    draft.tools.insert(
        "research_loop",
        Arc::new(crate::research_loop::ResearchLoopTool),
    );
    draft.tools.insert(
        "research_runner",
        Arc::new(crate::research_runner::ResearchRunnerTool),
    );
    draft.tools.insert(
        "research_write",
        Arc::new(crate::research_write::ResearchWriteTool),
    );
    draft.tools.insert(
        "research_verify",
        Arc::new(crate::research_verify::ResearchVerifyTool),
    );
    draft.tools.insert(
        "research_index",
        Arc::new(crate::research_index::ResearchIndexTool),
    );
    draft.tools.insert(
        "finding",
        Arc::new(TrackerTool {
            tool_name: "finding",
            noun: "finding",
            kind: &FINDINGS,
            requires_refs: Some(&SOURCES),
        }),
    );

    draft.tools.insert(
        "memory_search",
        Arc::new(kanzei_memory::memory::MemorySearchTool),
    );
    draft.tools.insert(
        "memory_note",
        Arc::new(kanzei_memory::memory::MemoryNoteTool),
    );

    draft.tools.insert(
        "req",
        Arc::new(ResearchTrackerTool::new(
            "req",
            "requirement",
            &REQUIREMENTS,
            None,
        )),
    );
    draft.tools.insert(
        "defect",
        Arc::new(ResearchTrackerTool::new("defect", "defect", &DEFECTS, None)),
    );
}

/// Research 档位的权限与专用工具资源注册。
pub(crate) fn configure_permissions(draft: &mut HarnessDraft) {
    draft
        .permissions
        .push(rule("memory_search", "*", Effect::Allow));
    draft
        .permissions
        .push(rule("memory_note", "*", Effect::Allow));

    // research 回流只允许读取既有单条目和新增草稿；禁止 list/update/close 等既有状态变更。
    for tool_name in ["source", "finding", "req", "defect"] {
        draft
            .permissions
            .push(rule(tool_name, "read:get", Effect::Allow));
        draft
            .permissions
            .push(rule(tool_name, "write:add", Effect::Allow));
        for action in [
            "update",
            "close",
            "archive",
            "reorder",
            "reopen",
            "fix_terminal",
            "archive_fill",
            "raw_delete",
            "repair_reused_id",
            "repair_missing_id",
            "void_id",
            "normalize",
        ] {
            let resource = format!("write:{action}");
            draft.permissions.push_managed_hard_deny(
                rule(tool_name, &resource, Effect::Deny),
                Some(tool_name),
                Some("research 档只允许 tracker get/add；不可修改既有条目状态"),
            );
        }
    }

    // 写权限收窄:仅 .kanzei/research/** 可写(report.md 等自由写作);其余 deny。
    for action in ["write", "edit", "insert"] {
        draft.permissions.push(rule(action, "*", Effect::Deny));
        draft
            .permissions
            .push(rule(action, "*.kanzei/research/*", Effect::Allow));
        for (file, tool) in [
            ("loop.json", "research_loop"),
            ("budget.json", "research_verify"),
            ("plan.json", "research_plan"),
        ] {
            draft.permissions.push_managed_hard_deny(
                rule(action, &format!("*.kanzei/research/*/{file}"), Effect::Deny),
                Some(tool),
                Some("结构化研究状态通过对应专用工具更新"),
            );
        }
    }
    // 研究档位的命令面是硬拒绝:专用工具是唯一真实替代通道,避免模型从 bash 绕过
    // LaTeX/绘图工具的权限边界。硬拒绝不可被用户规则重新放开。
    draft.permissions.push_managed_hard_deny(
        rule("bash", "*", Effect::Deny),
        None,
        Some("research 档禁止 bash；实验命令请使用 `research_runner`，编译与绘图使用 `latex` 或 `plot`，或使用 read/glob/grep/files/git status|diff|log 读取事实"),
    );
    // research 允许观察 Git，但禁止任何会改写仓库或发布的结构化动作。
    // UI2-0926 #13 复核:git 工具新增的 `init` 同样是改写(建库),独立课题工作区也不该被建成仓库。
    for subcommand in ["stage", "commit", "merge_ff", "finalize", "init"] {
        draft.permissions.push_managed_hard_deny(
            rule("git", subcommand, Effect::Deny),
            None,
            Some("research 档只允许 git status|diff|log 观察；不可提交或修改仓库"),
        );
    }
    draft.permissions.push(rule("webfetch", "*", Effect::Allow));
    draft
        .permissions
        .push(rule("websearch", "*", Effect::Allow));
    for resource in [
        "read:get",
        "write:create",
        "write:clarify",
        "write:request_approval",
    ] {
        draft
            .permissions
            .push(rule("research_plan", resource, Effect::Allow));
    }
    for resource in ["write:run", "write:cancel", "read:get"] {
        draft
            .permissions
            .push(rule("research_runner", resource, Effect::Allow));
    }
    for resource in [
        "read:resume",
        "write:start",
        "write:begin_search",
        "write:add_evidence",
        "write:reflect",
        "write:add_finding",
    ] {
        draft
            .permissions
            .push(rule("research_loop", resource, Effect::Allow));
    }
    for resource in [
        "write:write_outline",
        "write:write_section",
        "write:assemble_paper",
        "write:compile_paper",
        "write:repair_paper",
    ] {
        draft
            .permissions
            .push(rule("research_write", resource, Effect::Allow));
    }
    for resource in [
        "write:capture_source",
        "write:verify_claims",
        "write:budget_get",
        "write:budget_set",
    ] {
        draft
            .permissions
            .push(rule("research_verify", resource, Effect::Allow));
    }
    for resource in ["write:index_build", "write:index_resume"] {
        draft
            .permissions
            .push(rule("research_index", resource, Effect::Allow));
    }
    for resource in ["read:search", "read:symbols"] {
        draft
            .permissions
            .push(rule("research_index", resource, Effect::Allow));
    }
}
