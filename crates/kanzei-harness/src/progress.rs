//! 工具进度接力:runner 在执行工具前注入本次调用的上报通道,长耗时工具
//! (bash 等)在执行中增量上报输出;UI 据此实时显示"跑到哪了"。
//!
//! 为什么用 task-local 而不是给 ToolCtx 加字段:ToolCtx 以字面量在 26 处构造
//! (含大量测试),加字段是全仓破坏性改动;而进度是"执行环境"而非"工具输入",
//! task-local 让未注入的路径(测试、CLI 一次性调用)天然 no-op,零迁移成本。

use tokio::sync::mpsc;

/// 单条进度:哪次调用、这一段新增输出。
pub type ProgressChunk = (String, String);

#[derive(Clone)]
pub struct ProgressHandle {
    tool_call_id: String,
    sender: mpsc::UnboundedSender<ProgressChunk>,
}

impl ProgressHandle {
    pub fn new(tool_call_id: String, sender: mpsc::UnboundedSender<ProgressChunk>) -> Self {
        ProgressHandle {
            tool_call_id,
            sender,
        }
    }
}

tokio::task_local! {
    static PROGRESS: ProgressHandle;
}

/// 在注入了进度通道的作用域里执行工具 future。
pub async fn scope<F: std::future::Future>(handle: ProgressHandle, fut: F) -> F::Output {
    PROGRESS.scope(handle, fut).await
}

/// 工具侧上报一段新增输出。未注入通道(直调/测试)时静默丢弃;
/// 接收端已关闭同样静默——进度是尽力而为的旁路,绝不能让它影响工具本身。
pub fn emit(chunk: &str) {
    if chunk.is_empty() {
        return;
    }
    let _ = PROGRESS.try_with(|handle| {
        let _ = handle
            .sender
            .send((handle.tool_call_id.clone(), chunk.to_string()));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn 注入作用域内上报可收到_作用域外静默丢弃() {
        // 作用域外:不 panic、不阻塞。
        emit("孤儿输出");

        let (tx, mut rx) = mpsc::unbounded_channel();
        let handle = ProgressHandle::new("call_1".into(), tx);
        scope(handle, async {
            emit("第一段");
            emit(""); // 空段不该出现在通道里
            emit("第二段");
        })
        .await;
        assert_eq!(rx.recv().await.unwrap(), ("call_1".into(), "第一段".into()));
        assert_eq!(rx.recv().await.unwrap(), ("call_1".into(), "第二段".into()));
        assert!(rx.try_recv().is_err(), "空段与孤儿输出不该进通道");
    }
}
