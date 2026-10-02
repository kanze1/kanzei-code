//! R-242 批6:读路径投影 feature gate。
//!
//! 五条消息读路径(conversation_get / conversation_list / runner_prior /
//! ui_history / subagent_transcript)逐项切到事件投影后,任一读路径出现未知
//! 差异时必须能独立回退 legacy snapshot(验收⑥),gate 就是回退开关。
//!
//! 载体:环境变量 `KANZEI_PROJECTION_GATES`,逗号分隔启用的读路径(白名单)。
//! 未设置时按 [`DEFAULT_PROJECTION_PATHS`] 缺省启用(切换生效);显式设置后
//! 只启用列出的路径,剔除某条即回退该路径的 legacy 行为。
//!
//! 批6 边界:conversation_list 的投影段边界依赖 conversation.reset 事件
//! (批7 segment reset 已落地,conversation_list 已加入缺省启用);subagent_transcript
//! 由 R-279 建立事件投影真源(子代理对话落 subagent.transcript 事件)后加入缺省
//! 启用——进展里如实记录。

/// 缺省启用事件投影的读路径(未设置环境变量时)。
/// 批7a 后 conversation_list 的 reset 段边界已落地;R-279 后 subagent_transcript
/// 第五条路径加入(事件投影真源建立)。
const DEFAULT_PROJECTION_PATHS: [&str; 5] = [
    "conversation_get",
    "conversation_list",
    "runner_prior",
    "ui_history",
    "subagent_transcript",
];

/// 该读路径是否使用事件投影(而非 legacy snapshot)。
///
/// 纯判定,不读 env;env 解析在 [`read_path_uses_projection`] 完成,便于单测。
fn read_path_uses_projection_with(gates: Option<&str>, path: &str) -> bool {
    match gates {
        Some(value) => value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .any(|part| part == path),
        None => DEFAULT_PROJECTION_PATHS.contains(&path),
    }
}

/// 该读路径是否使用事件投影(读取 `KANZEI_PROJECTION_GATES`)。
pub(crate) fn read_path_uses_projection(path: &str) -> bool {
    read_path_uses_projection_with(
        std::env::var("KANZEI_PROJECTION_GATES").ok().as_deref(),
        path,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_paths_use_projection_when_env_absent() {
        for path in DEFAULT_PROJECTION_PATHS {
            assert!(
                read_path_uses_projection_with(None, path),
                "{path} 缺省应启用投影"
            );
        }
        // 五条读路径全部缺省启用(R-279 后 subagent_transcript 加入)。
        assert!(
            read_path_uses_projection_with(None, "subagent_transcript"),
            "subagent_transcript 缺省应启用投影(R-279)"
        );
    }

    #[test]
    fn explicit_gate_list_is_whitelist_and_supports_rollback() {
        // 显式白名单:只启用列出的路径。
        let gates = Some("runner_prior, conversation_get");
        assert!(read_path_uses_projection_with(gates, "runner_prior"));
        assert!(read_path_uses_projection_with(gates, "conversation_get"));
        // 未列入的路径回退 legacy(独立回滚,验收⑥)。
        assert!(!read_path_uses_projection_with(gates, "ui_history"));
        assert!(!read_path_uses_projection_with(gates, "conversation_list"));
        // 显式设置但为空/全空白 = 白名单为空,全部回退 legacy(强回退)。
        assert!(!read_path_uses_projection_with(Some(""), "runner_prior"));
        assert!(!read_path_uses_projection_with(
            Some("  "),
            "conversation_get"
        ));
    }
}
