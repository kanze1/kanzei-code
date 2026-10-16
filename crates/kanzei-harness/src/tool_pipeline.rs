//! 可选输入 Guard 与 runner 共用的执行辅助函数。
//!
//! `guarded_execute` 只负责 guards → body；工具自行解析输入，core runner
//! 独立强制权限、执行包装、ToolEnd、Recall 与遥测。没有结果策略或观察者扩展：
//! Guard 拒绝和 body 输出均原样返回，不能在后处理中改变其成功/失败事实。

use std::ops::ControlFlow;

use crate::tool::ToolOutput;

/// 工具特有的前置约束。实现持有已解析输入所需的事实；放行不等于权限授权。
pub trait ToolGuard: Send + Sync {
    /// Continue 放行；Break 携带最终结构化拒绝，后续 Guard 和 body 均不执行。
    fn check(&self) -> ControlFlow<ToolOutput>;
}

/// Runner 共用的执行包装：progress scope、执行前取消检查与 panic 分类。
///
/// runner 串行(drive.rs)与并行(tool_exec.rs)执行工具时**都**调它,把
/// 「注入 progress scope + 执行前 cancel 检查」收敛进 wrapper——工具 body
/// 不再各自实现这三者的注入/检查代码。
///
/// - `progress_tx`:注入后工具内 `progress::emit` 上报到该通道;None = 不注入
///   (测试/CLI 直调 no-op)。channel 由调用方建(串行一条、并行 wave 共用一条),
///   wrapper 只负责 handle 注入与 scope 包裹,不拥有通道生命周期。
/// - `halted`:执行前取消检查;返回 true 则 body 不执行,直接回 cancelled 错误。
///   **执行中**的取消由调用方 select 循环负责(D-342 drop future 中断)——那
///   是 runner 的事件循环职责,不是 wrap 段;wrap 段管「启动前门禁」。
pub async fn wrap_execute<B>(
    tool_call_id: String,
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::progress::ProgressChunk>>,
    halted: Option<&(dyn Fn() -> bool + Send + Sync)>,
    body: B,
) -> ToolOutput
where
    B: std::future::Future<Output = ToolOutput>,
{
    if halted.is_some_and(|h| h()) {
        return ToolOutput::error("cancelled: run stopped by user during execution");
    }
    use futures::FutureExt;
    let protected = std::panic::AssertUnwindSafe(body).catch_unwind();
    let result = match progress_tx {
        Some(tx) => {
            let handle = crate::progress::ProgressHandle::new(tool_call_id, tx);
            crate::progress::scope(handle, protected).await
        }
        None => protected.await,
    };
    match result {
        Ok(output) => output,
        Err(_) => ToolOutput::failed(
            "TOOL_PANICKED",
            "工具内部异常。操作可能已部分执行；先检查实际文件或进程状态，再决定是否重试。",
        ),
    }
}

/// [`with_timeout`] 的超时标记错误。不携带业务数据——超时善后所需的局部状态
/// (pid/缓冲)在调用方手里;clippy 1.98 起 `result_unit_err` 覆盖 async fn,
/// `Result<T, ()>` 不再可用,故以零尺寸类型承担同一语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedOut;

/// 限时执行辅助(R-259):对任意 future 施加 `tokio::time::timeout` 骨架。
///
/// 工具 body 可复用这一计时机制。超时后的**业务善后**(杀进程树/回传部分
/// 输出/围栏检查,如 bash)由调用方在 `Err` 分支处理:那是工具的执行语义,
/// 依赖 body 内局部状态(pid/缓冲),不属于通用 wrapper 机制。
pub async fn with_timeout<F, T>(fut: F, timeout: std::time::Duration) -> Result<T, TimedOut>
where
    F: std::future::Future<Output = T>,
{
    match tokio::time::timeout(timeout, fut).await {
        Ok(v) => Ok(v),
        Err(_) => Err(TimedOut),
    }
}

/// 检查可选 Guard，再执行一次 body；两个出口都保留原始结果语义及展示字段。
pub async fn guarded_execute<B>(guards: &[&dyn ToolGuard], body: B) -> ToolOutput
where
    B: std::future::Future<Output = ToolOutput>,
{
    for guard in guards {
        if let ControlFlow::Break(output) = guard.check() {
            return output;
        }
    }
    body.await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToolArtifact, ToolImage, ToolOutcome};
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    struct RejectGuard;
    impl ToolGuard for RejectGuard {
        fn check(&self) -> ControlFlow<ToolOutput> {
            ControlFlow::Break(
                ToolOutput::needs_correction("INPUT_REJECTED", "correct the input")
                    .with_display(json!({"guard":"input"})),
            )
        }
    }

    struct AllowGuard;
    impl ToolGuard for AllowGuard {
        fn check(&self) -> ControlFlow<ToolOutput> {
            ControlFlow::Continue(())
        }
    }

    struct UnreachableGuard;
    impl ToolGuard for UnreachableGuard {
        fn check(&self) -> ControlFlow<ToolOutput> {
            panic!("a rejected call must not reach later guards")
        }
    }

    #[tokio::test]
    async fn guarded_execute_rejection_preserves_result_and_never_polls_body() {
        let calls = AtomicU32::new(0);
        let out = guarded_execute(&[&AllowGuard, &RejectGuard, &UnreachableGuard], async {
            calls.fetch_add(1, Ordering::SeqCst);
            ToolOutput::ok("must not execute")
        })
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(out.is_error);
        assert_eq!(out.outcome, ToolOutcome::NeedsCorrection);
        assert_eq!(out.code, Some("INPUT_REJECTED"));
        assert_eq!(out.content, "correct the input");
        assert_eq!(out.display, Some(json!({"guard":"input"})));
    }

    #[tokio::test]
    async fn guarded_execute_preserves_every_body_outcome_and_executes_once() {
        for outcome in ToolOutcome::ALL {
            let calls = AtomicU32::new(0);
            let mut original = ToolOutput::ok("original body content");
            original.outcome = outcome;
            original.is_error = outcome != ToolOutcome::Success;
            original.code = Some("BODY_CODE");
            original.display = Some(json!({"original":true}));
            original.images = vec![ToolImage {
                media_type: "image/png".into(),
                data: "pixels".into(),
            }];
            original.artifact = Some(ToolArtifact {
                artifact_id: "body-artifact".into(),
                relative_path: "artifacts/body".into(),
                bytes: 7,
                sha256: "hash".into(),
                retrieval_hint: "read body".into(),
            });
            let out = guarded_execute(&[&AllowGuard], async {
                calls.fetch_add(1, Ordering::SeqCst);
                original.clone()
            })
            .await;
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(out.outcome, original.outcome);
            assert_eq!(out.is_error, original.is_error);
            assert_eq!(out.code, original.code);
            assert_eq!(out.content, original.content);
            assert_eq!(out.display, original.display);
            assert_eq!(out.images, original.images);
            assert_eq!(out.artifact, original.artifact);
        }
    }

    #[tokio::test]
    async fn wrap_execute_注入progress_作用域内上报可达() {
        // R-259 验收①progress 单点:wrap_execute 注入 handle 后,body 内
        // progress::emit 上报到调用方通道;未注入(tx=None)时静默 no-op。
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let out = wrap_execute("call_1".into(), Some(tx), None, async {
            crate::progress::emit("阶段一");
            ToolOutput::ok("done")
        })
        .await;
        assert!(!out.is_error);
        assert_eq!(
            rx.recv().await.unwrap(),
            ("call_1".into(), "阶段一".into()),
            "wrap 注入后 emit 必须到达调用方通道"
        );
        // 未注入:emit 静默,不 panic。
        let out = wrap_execute("call_2".into(), None, None, async {
            crate::progress::emit("孤儿");
            ToolOutput::ok("done")
        })
        .await;
        assert!(!out.is_error);
    }

    #[tokio::test]
    async fn wrap_execute_halted_前置拦截_不执行body() {
        // R-259 验收①cancellation 单点:halted 为 true 时 body 不执行,
        // 直接回 cancelled 错误;为 false 时正常执行。
        use std::sync::atomic::{AtomicU32, Ordering};
        let calls = Arc::new(AtomicU32::new(0));
        let calls2 = Arc::clone(&calls);
        let out = wrap_execute("call_1".into(), None, Some(&|| true), async move {
            calls2.fetch_add(1, Ordering::SeqCst);
            ToolOutput::ok("executed")
        })
        .await;
        assert!(out.is_error);
        assert!(out.content.contains("cancelled"));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "halted 前置拦截后 body 不得执行"
        );

        let out = wrap_execute("call_2".into(), None, Some(&|| false), async {
            ToolOutput::ok("executed")
        })
        .await;
        assert!(!out.is_error);
    }

    #[tokio::test]
    async fn with_timeout_timeout_returns_err_else_ok() {
        // R-259 验收①timeout 骨架单点:with_timeout 是 tokio::time::timeout 的
        // 唯一实现点;未超时 Ok、超时 Err(业务善后由调用方 Err 分支负责)。
        let ok = with_timeout(async { 42u32 }, std::time::Duration::from_secs(5)).await;
        assert_eq!(ok, Ok(42));
        let timed_out = with_timeout(
            tokio::time::sleep(std::time::Duration::from_millis(100)),
            std::time::Duration::from_millis(1),
        )
        .await;
        assert!(timed_out.is_err(), "超时必须返回 Err");
    }
    #[tokio::test]
    async fn panic_returns_one_failure_and_drops_progress_scope() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let output = wrap_execute("panic-call".into(), Some(tx), None, async {
            crate::progress::emit("before panic");
            panic!("bad tool");
            #[allow(unreachable_code)]
            ToolOutput::ok("unreachable")
        })
        .await;
        assert_eq!(output.code, Some("TOOL_PANICKED"));
        assert_eq!(output.outcome, ToolOutcome::Failed);
        assert!(output.is_error);
        assert_eq!(rx.recv().await.unwrap().1, "before panic");
        assert!(rx.recv().await.is_none());
        let next = wrap_execute("next".into(), None, None, async {
            ToolOutput::ok("next survived")
        })
        .await;
        assert_eq!(next.content, "next survived");
    }
}
