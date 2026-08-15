//! R-244 批1:统一 Tool Pipeline 骨架。
//!
//! 固定阶段:parse/materialize → policy allow/deny/ask → monotonic guards →
//! execution wrappers → tool body → result policies → immutable observers。
//!
//! 本条批1 只立骨架 + 迁移一个无副作用工具(glob)验证通道;规则引擎
//! (Ruleset/hard_denies)与托管 fence 一律复用,不重写。边界:
//! - Guard 是**单调防线**:只收紧不放宽,policy allow 永远不能覆盖 guard deny;
//! - Observer 只观察最终结果,不得修改 ToolOutput 或反向影响执行;
//! - 无论成功/拒绝/失败,流水线都返回唯一 final ToolOutput。

use std::sync::Arc;

use crate::tool::{ToolCtx, ToolOutput};
use serde_json::Value;

/// 流水线阶段(供 Observer 定位与测试断言阶段顺序)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPhase {
    /// 入参解析(工具自报 schema 校验,失败即拒绝)。
    Parse,
    /// 权限策略(调用方在 drive 层已 evaluate;此处预留显式顺序标记)。
    Policy,
    /// 单调 Guard(deny / 托管文件 / writer ownership 等不可逆防线)。
    Guard,
    /// 执行包装(timeout / cancellation / progress——R-244 只在此一处实现)。
    Wrap,
    /// 工具本体。
    Body,
    /// 结果策略(recall 注入 / redundancy 提醒等后处理)。
    ResultPolicy,
    /// 不可变观察者(只读最终结果,抛错不得改变终态)。
    Observe,
}

/// 单调 Guard:拒绝即拦截,放行后阶段不可回退。
pub trait ToolGuard: Send + Sync {
    fn name(&self) -> &'static str;
    /// 返回 `Err(reason)` = 拦截;`Ok(())` = 放行。
    /// 语义约束:Guard 只收紧不放宽——后续阶段不得绕过已通过的 Guard。
    fn check(&self, tool_name: &str, input: &Value, ctx: &ToolCtx) -> Result<(), String>;
}

/// 结果策略:对 ToolOutput 就地后处理(不阻断,不改变成功/失败判定语义)。
pub trait ToolResultPolicy: Send + Sync {
    fn name(&self) -> &'static str;
    fn apply(&self, output: &mut ToolOutput);
}

/// 不可变观察者:只观察最终结果;抛错被流水线捕获,不得改变工具事实终态。
pub trait ToolObserver: Send + Sync {
    fn observe(&self, tool_name: &str, output: &ToolOutput);
}

/// Wrap 段执行(R-259):timeout/cancellation/progress 三能力的唯一实现点。
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
    match progress_tx {
        Some(tx) => {
            let handle = crate::progress::ProgressHandle::new(tool_call_id, tx);
            crate::progress::scope(handle, body).await
        }
        None => body.await,
    }
}

/// 限时执行辅助(R-259):对任意 future 施加 `tokio::time::timeout` 骨架。
///
/// 这是 timeout 机制的**唯一**实现点——工具 body 需要超时语义时调它,不再
/// 各自写 `tokio::time::timeout`。超时后的**业务善后**(杀进程树/回传部分
/// 输出/围栏检查,如 bash)由调用方在 `Err` 分支处理:那是工具的执行语义,
/// 依赖 body 内局部状态(pid/缓冲),不属于通用 wrapper 机制。
pub async fn with_timeout<F, T>(fut: F, timeout: std::time::Duration) -> Result<T, ()>
where
    F: std::future::Future<Output = T>,
{
    match tokio::time::timeout(timeout, fut).await {
        Ok(v) => Ok(v),
        Err(_) => Err(()),
    }
}

/// 执行统一流水线。
///
/// 顺序固定:guards 全部通过 → body(工具本体,调用方提供)执行 → result
/// policies 依次 apply → observers 依次 observe。任何阶段返回/产出唯一
/// ToolOutput:guard 拒绝、body 失败、策略与观察者异常都不产生第二条结果。
///
/// `body` 是调用方提供的工具实现 future——工具 `execute` 里把内部逻辑包成
/// body 传入,流水线负责 guards/policies/observers 包裹,避免递归。
pub async fn run_tool_pipeline<B>(
    tool_name: &str,
    input: Value,
    ctx: &ToolCtx,
    guards: &[Arc<dyn ToolGuard>],
    body: B,
    result_policies: &[Arc<dyn ToolResultPolicy>],
    observers: &[Arc<dyn ToolObserver>],
) -> ToolOutput
where
    B: std::future::Future<Output = ToolOutput>,
{
    // Guard 段:单调防线,任一拒绝即整体拒绝,不执行 body。
    for guard in guards {
        match guard.check(tool_name, &input, ctx) {
            Ok(()) => {}
            Err(reason) => {
                let output = ToolOutput::error(format!(
                    "permission denied by guard `{}`: {reason}",
                    guard.name()
                ));
                notify_observers(tool_name, &output, observers);
                return output;
            }
        }
    }

    // Body 段:工具本体(唯一真正执行点)。
    let mut output = body.await;

    // Result policy 段:就地后处理,不阻断。
    for policy in result_policies {
        policy.apply(&mut output);
    }

    notify_observers(tool_name, &output, observers);
    output
}

/// Observer 段:抛错被捕获(记入遥测),不改变工具事实终态。
fn notify_observers(tool_name: &str, output: &ToolOutput, observers: &[Arc<dyn ToolObserver>]) {
    for observer in observers {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            observer.observe(tool_name, output);
        }));
        if result.is_err() {
            // 观察者异常只留遥测,不改变 final result。
            tracing::warn!(
                tool = tool_name,
                "tool observer panicked; final result unchanged"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct DenyGuard;
    impl ToolGuard for DenyGuard {
        fn name(&self) -> &'static str {
            "deny-guard"
        }
        fn check(&self, _tool_name: &str, _input: &Value, _ctx: &ToolCtx) -> Result<(), String> {
            Err("hard deny".into())
        }
    }

    struct AllowGuard;
    impl ToolGuard for AllowGuard {
        fn name(&self) -> &'static str {
            "allow-guard"
        }
        fn check(&self, _tool_name: &str, _input: &Value, _ctx: &ToolCtx) -> Result<(), String> {
            Ok(())
        }
    }

    struct StampPolicy;
    impl ToolResultPolicy for StampPolicy {
        fn name(&self) -> &'static str {
            "stamp"
        }
        fn apply(&self, output: &mut ToolOutput) {
            output.display = Some(json!({ "stamped": true }));
        }
    }

    struct RecordingObserver {
        seen: std::sync::Mutex<Vec<(String, String)>>,
    }
    impl ToolObserver for RecordingObserver {
        fn observe(&self, tool_name: &str, output: &ToolOutput) {
            self.seen
                .lock()
                .unwrap()
                .push((tool_name.to_string(), output.content.clone()));
        }
    }

    struct PanicObserver;
    impl ToolObserver for PanicObserver {
        fn observe(&self, _tool_name: &str, _output: &ToolOutput) {
            panic!("observer exploded");
        }
    }

    fn ctx() -> ToolCtx {
        ToolCtx::new(std::path::PathBuf::from("."), std::path::PathBuf::from("."))
    }

    async fn ok_body(_input: Value, _ctx: &ToolCtx) -> ToolOutput {
        ToolOutput::ok("executed")
    }

    #[tokio::test]
    async fn pipeline_guard拒绝_不执行body_返回唯一拒绝结果() {
        // R-244 验收③:policy allow 不能覆盖 guard deny——guard 拒绝即整体拒绝。
        let out = run_tool_pipeline(
            "probe",
            json!({}),
            &ctx(),
            &[Arc::new(DenyGuard)],
            ok_body(json!({}), &ctx()),
            &[],
            &[],
        )
        .await;
        assert!(out.is_error, "guard 拒绝必须产生错误结果");
        assert!(
            out.content.contains("deny-guard") && out.content.contains("hard deny"),
            "拒绝理由点名 guard 与原因: {}",
            out.content
        );
        assert!(
            !out.content.contains("executed"),
            "guard 拒绝后 body 不得执行"
        );
    }

    #[tokio::test]
    async fn pipeline_成功_阶段顺序固定_结果策略与观察者生效() {
        // 阶段顺序:guards → body → result policies → observers。
        let observer = Arc::new(RecordingObserver {
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let out = run_tool_pipeline(
            "probe",
            json!({}),
            &ctx(),
            &[Arc::new(AllowGuard)],
            ok_body(json!({}), &ctx()),
            &[Arc::new(StampPolicy)],
            &[Arc::clone(&observer) as Arc<dyn ToolObserver>],
        )
        .await;
        assert!(!out.is_error);
        assert_eq!(
            out.display,
            Some(json!({ "stamped": true })),
            "result policy 在返回前已 apply"
        );
        let seen = observer.seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "观察者只看到最终结果一次");
        assert_eq!(seen[0].0, "probe");
        assert_eq!(seen[0].1, "executed");
    }

    #[tokio::test]
    async fn pipeline_观察者抛错_不改变工具事实终态() {
        // R-244 验收⑤:observer 抛错不改终态,但留下遥测(此处以返回正常结果验证)。
        let out = run_tool_pipeline(
            "probe",
            json!({}),
            &ctx(),
            &[],
            ok_body(json!({}), &ctx()),
            &[],
            &[Arc::new(PanicObserver)],
        )
        .await;
        assert!(!out.is_error, "观察者 panic 不得改变工具终态");
        assert_eq!(out.content, "executed");
    }

    #[tokio::test]
    async fn pipeline_失败与拒绝都返回唯一结果() {
        // R-244 验收⑦:失败、拒绝路径都产生唯一 final result。
        async fn fail_body(_input: Value, _ctx: &ToolCtx) -> ToolOutput {
            ToolOutput::error("boom")
        }
        let out = run_tool_pipeline(
            "failing",
            json!({}),
            &ctx(),
            &[],
            fail_body(json!({}), &ctx()),
            &[],
            &[],
        )
        .await;
        assert!(out.is_error);
        assert_eq!(out.content, "boom");
    }

    #[tokio::test]
    async fn pipeline_body_恰好执行一次_无双执行() {
        // R-244 验收⑥:工具走统一通道但无双执行——body 必须恰好执行一次,
        // pipeline 不产生第二遍执行(迁移工具 execute 调 pipeline,body 是独立函数)。
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;
        let calls = Arc::new(AtomicU32::new(0));
        let calls2 = Arc::clone(&calls);
        let body = async move {
            calls2.fetch_add(1, Ordering::SeqCst);
            ToolOutput::ok("once")
        };
        run_tool_pipeline("probe", json!({}), &ctx(), &[], body, &[], &[]).await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "body 必须恰好执行一次(无双执行)"
        );
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
}
