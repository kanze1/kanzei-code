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
}
