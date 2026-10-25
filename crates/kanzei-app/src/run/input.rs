//! 输入准入域(R-253 批1,纯搬迁自 run.rs)。
//!
//! 独立理由:输入如何进入会话(交付模式解析、admit、promote)与「怎么跑」无关——
//! `parse_delivery` 是纯字符串→枚举映射,`admit_input`/`promote_next_input` 是
//! SessionStore 的薄封装,`code_root_for` 决定本轮代码树(cwd)是 worktree 还是
//! 项目目录。四者都被 run_prompt/run_task 调用,但彼此与事件循环、事件归约、
//! 落库零耦合,留在 run.rs 只会让运行主链路文件继续膨胀(照 files_view.rs 模式)。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// 交付模式解析:字符串 → Delivery(原 run.rs parse_delivery)。
pub(crate) fn parse_delivery(value: Option<&str>) -> anyhow::Result<kanzei_core::Delivery> {
    match value.unwrap_or("queue") {
        "steer" => Ok(kanzei_core::Delivery::Steer),
        "queue" => Ok(kanzei_core::Delivery::Queue),
        other => Err(anyhow::anyhow!("未知输入交付模式: {other}")),
    }
}

pub(crate) struct QueuedInput<'a> {
    pub prompt: &'a str,
    pub attachments: Vec<crate::PromptAttachment>,
    pub delivery: kanzei_core::Delivery,
    pub origin: kanzei_base::MessageOrigin,
    pub execution_batch: bool,
    pub work_item_id: Option<&'a str>,
    pub command: Option<&'a crate::commands::run::InputCommand>,
}

/// Persist the complete input and its source before admitting it to the queue.
pub(crate) fn admit_input(
    project_dir: &str,
    session_id: &str,
    input: QueuedInput<'_>,
) -> anyhow::Result<kanzei_core::AdmittedInput> {
    let QueuedInput {
        prompt,
        attachments,
        delivery,
        origin,
        execution_batch,
        work_item_id,
        command,
    } = input;
    let project_root = crate::normalized_project_root(Path::new(project_dir));
    let attachments = crate::attachments::prepare(&project_root, attachments)?.references;
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project_root))?;
    store.create_session(session_id, &project_root.display().to_string(), None)?;
    if let Some(command) = command {
        return Ok(store.admit_command(
            session_id,
            kanzei_core::CommandAdmission {
                command_id: &command.id,
                payload_hash: &command.payload_hash,
                prompt,
                delivery,
                origin,
                attachments: &attachments,
                execution_batch,
                work_item_id,
            },
        )?);
    }
    let input_id = format!(
        "input_{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    let input = if let Some(item_id) = work_item_id {
        store.admit_work_input(session_id, &input_id, prompt, item_id, origin, &attachments)?
    } else if execution_batch {
        store.admit_batch_input(
            session_id,
            &input_id,
            prompt,
            delivery,
            origin,
            &attachments,
        )?
    } else {
        store.admit_input_with_source(
            session_id,
            &input_id,
            prompt,
            delivery,
            Some(origin.as_str()),
            &attachments,
        )?
    };
    Ok(input)
}

/// 自动续跑的队列去重：同一会话中同一条自动推进指令已经 pending 时，
/// 重复 IPC 不得再次写入队列。手动输入不走这里的判断，保留用户有意重复发送的语义。
pub(crate) fn has_pending_queue_prompt(
    project_dir: &str,
    session_id: &str,
    prompt: &str,
) -> anyhow::Result<bool> {
    let project_root = crate::normalized_project_root(Path::new(project_dir));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project_root))?;
    store.create_session(session_id, &project_root.display().to_string(), None)?;
    Ok(store
        .list_pending_inputs(session_id)?
        .into_iter()
        .any(|input| {
            matches!(input.delivery, kanzei_core::Delivery::Queue) && input.prompt == prompt
        }))
}

/// 提升队首输入为当前运行输入并记 prompt.promoted 事件(原 run.rs promote_next_input)。
pub(crate) fn promote_next_input(
    project_dir: &str,
    session_id: &str,
) -> anyhow::Result<Option<kanzei_core::AdmittedInput>> {
    let project_root = crate::normalized_project_root(Path::new(project_dir));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project_root))?;
    Ok(store.promote_next_input(session_id)?)
}

pub(crate) enum SavedInputResume {
    Start(kanzei_core::AdmittedInput),
    Queued,
    Consumed,
}

/// Called under the owner's lifecycle lock, shared with run_prompt, stop and round draining.
/// Retrying a review never duplicates its input or starts two schedulers.
pub(crate) fn resume_saved_input(
    project_dir: &str,
    session_id: &str,
    input_id: &str,
    running: bool,
) -> anyhow::Result<SavedInputResume> {
    let root = crate::normalized_project_root(Path::new(project_dir));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))?;
    match store.input_status(input_id)?.as_deref() {
        Some("pending") => {}
        Some("promoted" | "running" | "completed") => return Ok(SavedInputResume::Consumed),
        Some("cancelled") => anyhow::bail!("回复已撤销，未恢复执行"),
        Some("failed") => anyhow::bail!("回复已尝试执行但失败，请查看原对话里的错误后继续"),
        _ => anyhow::bail!("已保存的回复不存在"),
    }
    anyhow::ensure!(
        store
            .list_pending_inputs(session_id)?
            .iter()
            .any(|i| i.input_id == input_id),
        "回复不属于原执行的对话"
    );
    if running {
        return Ok(SavedInputResume::Queued);
    }
    // Preserve the existing queue order, including any replies saved by an older app.
    let input = promote_next_input(project_dir, session_id)?
        .ok_or_else(|| anyhow::anyhow!("回复队列已变化，请重试"))?;
    Ok(SavedInputResume::Start(input))
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

    #[test]
    fn attachment_inputs_keep_their_delivery_and_survive_queue_recovery() {
        use base64::Engine;
        let root = kanzei_base::test_support::TestDir::new("kz-queued-image-");
        let bytes = b"\x89PNG\r\n\x1a\nqueued image fixture";
        for (delivery, origin) in [
            (
                kanzei_core::Delivery::Queue,
                kanzei_base::MessageOrigin::Human,
            ),
            (
                kanzei_core::Delivery::Steer,
                kanzei_base::MessageOrigin::Engine,
            ),
        ] {
            let input = super::admit_input(
                root.to_str().unwrap(),
                "s",
                super::QueuedInput {
                    prompt: "",
                    attachments: vec![crate::PromptAttachment {
                        file_name: "image.png".into(),
                        media_type: "image/png".into(),
                        data: base64::engine::general_purpose::STANDARD.encode(bytes),
                    }],
                    delivery,
                    origin,
                    execution_batch: false,
                    work_item_id: None,
                    command: None,
                },
            )
            .unwrap();
            let store =
                kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
            let restored = store
                .list_pending_inputs("s")
                .unwrap()
                .into_iter()
                .find(|saved| saved.input_id == input.input_id)
                .unwrap();
            assert_eq!(restored.delivery, delivery);
            assert_eq!(restored.prompt, input.prompt);
            assert_eq!(restored.origin, origin);
            assert!(restored.prompt.is_empty());
            assert_eq!(restored.attachments, input.attachments);
            assert_eq!(restored.attachments[0].name, "image.png");
            assert_eq!(std::fs::read(&restored.attachments[0].path).unwrap(), bytes);
        }
    }

    #[test]
    fn invalid_attachment_never_admits_an_input() {
        let root = kanzei_base::test_support::TestDir::new("kz-queued-invalid-image-");
        let result = super::admit_input(
            root.to_str().unwrap(),
            "s",
            super::QueuedInput {
                prompt: "keep this draft",
                attachments: vec![crate::PromptAttachment {
                    file_name: "image.png".into(),
                    media_type: "image/png".into(),
                    data: "invalid base64!".into(),
                }],
                delivery: kanzei_core::Delivery::Steer,
                origin: kanzei_base::MessageOrigin::Human,
                execution_batch: false,
                work_item_id: None,
                command: None,
            },
        );
        assert!(result.is_err());
        assert!(
            !kanzei_core::project_state_path(&root).exists(),
            "validation must precede durable admission"
        );
    }

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
