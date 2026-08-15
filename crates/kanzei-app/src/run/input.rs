//! 输入准入域(R-253 批1,纯搬迁自 run.rs)。
//!
//! 独立理由:输入如何进入会话(交付模式解析、admit、promote)与「怎么跑」无关——
//! `parse_delivery` 是纯字符串→枚举映射,`admit_input`/`promote_next_input` 是
//! SessionStore 的薄封装,`code_root_for` 决定本轮代码树(cwd)是 worktree 还是
//! 项目目录。四者都被 run_prompt/run_task 调用,但彼此与事件循环、事件归约、
//! 落库零耦合,留在 run.rs 只会让运行主链路文件继续膨胀(照 files_view.rs 模式)。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

/// 交付模式解析:字符串 → Delivery(原 run.rs parse_delivery)。
pub(crate) fn parse_delivery(value: Option<&str>) -> anyhow::Result<kanzei_core::Delivery> {
    match value.unwrap_or("queue") {
        "steer" => Ok(kanzei_core::Delivery::Steer),
        "queue" => Ok(kanzei_core::Delivery::Queue),
        other => Err(anyhow::anyhow!("未知输入交付模式: {other}")),
    }
}

/// 提交一条输入到会话队列并记 prompt.admitted 事件(原 run.rs admit_input)。
pub(crate) fn admit_input(
    project_dir: &str,
    session_id: &str,
    prompt: &str,
    delivery: kanzei_core::Delivery,
) -> anyhow::Result<kanzei_core::AdmittedInput> {
    let project_root = crate::normalized_project_root(Path::new(project_dir));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project_root))?;
    store.create_session(session_id, &project_root.display().to_string(), None)?;
    let input_id = format!(
        "input_{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    let input = store.admit_input(session_id, &input_id, prompt, delivery)?;
    store.append_event(session_id, "prompt.admitted", &json!({ "input_id": input_id, "delivery": if matches!(delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }))?;
    Ok(input)
}

/// 提升队首输入为当前运行输入并记 prompt.promoted 事件(原 run.rs promote_next_input)。
pub(crate) fn promote_next_input(
    project_dir: &str,
    session_id: &str,
) -> anyhow::Result<Option<kanzei_core::AdmittedInput>> {
    let project_root = crate::normalized_project_root(Path::new(project_dir));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project_root))?;
    let Some(input) = store.promote_next_input(session_id)? else {
        return Ok(None);
    };
    store.append_event(session_id, "prompt.promoted", &json!({ "input_id": input.input_id, "delivery": if matches!(input.delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }))?;
    Ok(Some(input))
}

/// 本轮代码树:线绑了 worktree 就在那棵树上跑,否则就是项目目录本身。
///
/// **这是唯一让 `cwd` 真正指向 worktree 的地方**(R-177 内容②)。`main_root`
/// 一路不变——托管文档、state.db、记忆、配置全部仍落主根,两者第一次真正分叉。
/// 抽成纯函数是为了让这条判定可以直接测,不必去构造 `Window` 与整条运行链。
pub(crate) fn code_root_for(worktree_path: Option<&str>, project_dir: &str) -> String {
    worktree_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| project_dir.to_string())
}

#[cfg(test)]
mod tests {
    use super::code_root_for;

    /// R-177 验收② 前半:线上运行时 cwd = worktree、project_root = 主根,两者**不相等**。
    #[test]
    fn 线上运行cwd是worktree_project_root是主根() {
        let main_root = "C:/proj/kanzei";
        let worktree = "C:/proj/.kanzei-worktree-kanzei.f6";
        let cwd = code_root_for(Some(worktree), main_root);
        assert_eq!(cwd, worktree, "线绑了树就必须在那棵树上跑");
        assert_ne!(cwd, main_root, "cwd 与主根必须分叉,否则线没有物理隔离");
        // 主树进程一个字节都不变:worktree_path 为 None(或空串)时恒等于项目目录。
        assert_eq!(code_root_for(None, main_root), main_root);
        assert_eq!(code_root_for(Some("   "), main_root), main_root);
    }
}
