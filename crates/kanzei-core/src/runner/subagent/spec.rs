//! task 工具的描述和输入 schema；人格枚举来自本轮实际可选名册。

use kanzei_llm::ToolSpec;

/// R-327:按运行时可选人格生成 task 的 schema。
///
/// `agent` 的 enum **由名册决定而不是硬编码**:名册里只有默认人格时(CLI 单运行、
/// 测试桩)整个参数不出现,模型看到的 schema 与引入前逐字节一致;接了名册才多这
/// 一个可选项。硬编码枚举会让「schema 说有、运行时没有」——模型照着选一个不存在
/// 的人格,而回落是静默的,它永远不知道自己没选中。
pub(crate) fn task_spec_for(agent_names: &[String]) -> ToolSpec {
    let mut spec = task_spec();
    if agent_names.len() > 1 {
        spec.input_schema["properties"]["agent"] = serde_json::json!({
            "type": "string",
            "enum": agent_names,
            "description": "Which read-only subagent persona to use.                             `explore` = fast model, mechanical search, small step budget.                             `plan` = main model, larger budget, establishes constraints and                             returns a concrete plan with file:line evidence.                             All personas share the same read-only toolset; only the prompt                             and step budget differ. Defaults to the first one."
        });
    }
    spec
}

pub(crate) fn task_spec() -> ToolSpec {
    ToolSpec {
        name: "task".into(),
        description: "Delegate a narrow read-only exploration task (find files, call \
                      sites, usages; read and summarize code; inspect git status/diff/\
                      log — R-218) to a subagent with ONLY read/glob/grep/files and \
                      git read-only subcommands. It cannot write/edit files, run bash, \
                      stage/commit/merge git changes, or publish/release; those \
                      authority-bearing actions belong to the primary agent. Params: \
                      prompt (self-contained instruction saying exactly what to find and \
                      what to report back); optional description: a short 3-8 word label \
                      the user sees for this delegation; optional model: \"fast\" (default, local model, \
                      mechanical searches) | \"primary\" (tasks needing code comprehension); \
                      optional schema: a JSON Schema — when given, the subagent must answer \
                      with JSON matching it and you receive the validated object instead of \
                      prose, so you never have to parse a summary. \
                      Multiple task calls in one turn run in parallel — when you have several \
                      independent scouting/finding questions (different files, regions, or \
                      aspects of the codebase), dispatch them as SEPARATE task calls in the \
                      SAME turn (up to max_tasks_per_turn) instead of one at a time; parallel \
                      dispatch is significantly faster than serial scouting."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Self-contained task: what to find and exactly what to report back"
                },
                "description": {
                    "type": "string",
                    "description": "Short 3-8 word label shown to the user for this delegation, e.g. \"Find auth call sites\"."
                },
                "model": {
                    "type": "string",
                    "enum": ["fast", "primary"],
                    "description": "fast = local small model (default); primary = main model"
                },
                "schema": {
                    "type": "object",
                    "description": "Optional JSON Schema the answer must match. Supported keywords: type, required, properties, items, enum. Use it whenever you want structured data back rather than a written summary."
                }
            },
            "required": ["prompt"]
        }),
    }
}
