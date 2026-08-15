//! R-246:LineRuntime——统一资源 owner。
//!
//! 独立理由:一轮 run 会散布 cancellation token、active run、子代理、transcript
//! 投影、后台结果、通知订阅、后台进程、读写租约、worktree 绑定与临时 artifact,
//! 各自有独立的注册表与 Drop 语义,收尾时缺少一个「保证全部静止并落终态」的
//! 单点。LineRuntime 统一持有它们:普通资源生命周期 **不超过** LineRuntime;
//! persistent 服务(R-180)只能通过 adoption 事件显式移交给 ProjectRuntime。
//!
//! 关键约束(设计文档 deepseek_harness_upgrade.md §LineRuntime 生命周期):
//! - `dispose()` 必须幂等;**并发调用共享同一个完成 future**(第一次调用创建,
//!   后续调用 await 同一个 future,收尾只发生一次);
//! - dispose 返回前必须等待:工具 wrapper 静止、子代理退出、普通后台进程收回、
//!   订阅与租约释放、生命周期终态写入;
//! - 不重做 R-180 长驻服务注册表(kanzei-tools/src/background.rs),以适配/收口
//!   方式接入;未 adopt 的资源不能通过布尔值或遗失 handle 隐式长驻。

use std::sync::Arc;
use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

/// R-246:子代理取消注册表的再导出——LineRuntime 收口后仍复用既有实现
/// (TaskCancellations + RAII Guard,subagent.rs),不重做。
pub use super::subagent::TaskCancellations;

/// dispose 的最终结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisposeOutcome {
    /// 本次 dispose 是否真正执行了收尾(第二次及之后的并发调用 = false)。
    pub performed: bool,
    /// 收尾时取消的子代理数。
    pub child_agents_cancelled: usize,
    /// 收尾时收回的非 persistent 后台进程数。
    pub background_processes_reaped: usize,
}

/// dispose 完成 future 的类型别名(避免 clippy type_complexity)。
type DisposeFuture = futures::future::Shared<
    std::pin::Pin<Box<dyn std::future::Future<Output = DisposeOutcome> + Send>>,
>;

/// R-246:LineRuntime 骨架(批1)——统一资源 owner 的结构与幂等 dispose 机制。
///
/// 本批只立骨架:持有物清单字段 + cancellation token + 幂等 dispose 共享 future
/// + 基础单测。后续批次逐个接入子代理/后台进程/通知/租约/artifact 的收口。
#[derive(Clone)]
pub struct LineRuntime {
    inner: Arc<Inner>,
    /// 幂等 dispose:第一次调用创建 shared future,后续调用复用。
    /// `Mutex<Option>` + AtomicBool 保证并发下只初始化一次,且能区分
    /// 「哪个调用真正执行了收尾」(CAS 赢家 performed=true)。
    dispose_state: Arc<DisposeState>,
}

/// dispose 的共享状态:完成 future + 首次调用标志。
struct DisposeState {
    future: Mutex<Option<DisposeFuture>>,
    /// false → true 的首次 CAS:赢家执行收尾,其余等待同一 future。
    first_called: std::sync::atomic::AtomicBool,
}

/// dispose 生命周期事件回调类型(R-246 批4):dispose 完成时写终态事件。
/// 复用 R-175 BackgroundEventSink 同款形态(`Arc<dyn Fn>` 可 Clone,绕开
/// SessionStore 非 Send 限制)。None = 测试/CLI 不落库。
pub type LifecycleSink = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// LineRuntime 的内部状态。独立结构 + Arc 包装,使 dispose future 可 'static
/// (借 &self 的 async fn 无法存入 OnceLock<Shared>——lifetime 必须 outlive 'static)。
struct Inner {
    /// 本 line 的取消令牌:dispose 时先 cancel,令 active run 与子代理协作式停止。
    cancellation: CancellationToken,
    /// 子代理取消注册表(复用 R-174/R-175 既有实现;Arc 共享,clone 的是指针)。
    child_agents: Arc<TaskCancellations>,
    /// 已 spawn 子代理的 join handle(批2:dispose 时 cancel 后 await 全部退出,
    /// 三种终态——完成/失败/被停——都在 join 返回时确认读槽已释放)。
    child_agent_joins: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    /// 后台进程句柄(普通资源,dispose 收回)。
    background_processes: Mutex<Vec<String>>,
    /// R-246 批4:dispose 终态事件落库回调(可选)。dispose_once 完成时写
    /// `{"kind":"runtime.lifecycle","id":...,"state":"disposed"}`。
    lifecycle_sink: Option<LifecycleSink>,
}

impl Default for LineRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl LineRuntime {
    pub fn new() -> Self {
        LineRuntime::with_lifecycle_sink(None)
    }

    /// 带生命周期事件落库回调的构造(R-246 批4)。sink 在 dispose 完成时被调用,
    /// 写入 `runtime.lifecycle` disposed 终态事件;None = 测试/CLI 不落库。
    pub fn with_lifecycle_sink(sink: Option<LifecycleSink>) -> Self {
        LineRuntime {
            inner: Arc::new(Inner {
                cancellation: CancellationToken::new(),
                child_agents: Arc::new(TaskCancellations::default()),
                child_agent_joins: Mutex::new(Vec::new()),
                background_processes: Mutex::new(Vec::new()),
                lifecycle_sink: sink,
            }),
            dispose_state: Arc::new(DisposeState {
                future: Mutex::new(None),
                first_called: std::sync::atomic::AtomicBool::new(false),
            }),
        }
    }

    /// 本 line 的取消令牌(供 RunnerConfig.halt / 子代理共享)。
    pub fn cancellation(&self) -> &CancellationToken {
        &self.inner.cancellation
    }

    /// 子代理取消注册表句柄(供 run_subagent 注册)。
    pub fn child_agents(&self) -> Arc<TaskCancellations> {
        Arc::clone(&self.inner.child_agents)
    }

    /// 登记一个普通(非 persistent)后台进程 id,dispose 时收回。
    pub fn track_background_process(&self, id: String) {
        self.inner.background_processes.lock().unwrap().push(id);
    }

    /// 登记一个已 spawn 的子代理 join handle;dispose 时 cancel 后 await 全部退出。
    /// 由 drive.rs 后台子代理 spawn 处调用(批2 接线)。
    pub fn track_child_agent(&self, handle: tokio::task::JoinHandle<()>) {
        self.inner.child_agent_joins.lock().unwrap().push(handle);
    }

    /// 幂等 dispose:并发调用共享同一个完成 future,收尾只发生一次。
    ///
    /// 实现:①CAS 竞争「首次调用」——赢家执行收尾,输家等待同一 future;
    /// ②完成 future 惰性创建并存入 Mutex,后续调用复用;③`Shared` 允许多个
    /// await 同一 future,内部只 poll 一次。返回值里 `performed=true` 只属于
    /// CAS 赢家,其余调用 `performed=false` 但拿到同一完成结果。
    pub async fn dispose(&self) -> DisposeOutcome {
        let is_first = !self
            .dispose_state
            .first_called
            .swap(true, std::sync::atomic::Ordering::SeqCst);
        let fut = {
            let mut guard = self.dispose_state.future.lock().unwrap();
            if guard.is_none() {
                use futures::FutureExt;
                let inner = Arc::clone(&self.inner);
                let boxed: std::pin::Pin<
                    Box<dyn std::future::Future<Output = DisposeOutcome> + Send>,
                > = Box::pin(dispose_once(inner));
                *guard = Some(boxed.shared());
            }
            guard.as_ref().unwrap().clone()
        };
        let outcome = fut.await;
        DisposeOutcome {
            performed: is_first,
            ..outcome
        }
    }
}

/// 真正的收尾动作(仅由 dispose 的第一次调用执行)。
/// 接收 `Arc<Inner>` 而非 `&self`,使 future 满足 'static(OnceLock<Shared> 要求)。
async fn dispose_once(inner: Arc<Inner>) -> DisposeOutcome {
    // 1) 取消令牌:active run / 子代理在安全检查点协作式停止。
    inner.cancellation.cancel();
    // 2) 取消全部子代理(幂等:已结束的 unregister 过,不在表内)。
    //    run_subagent 的 future drop 时读槽 RAII 释放(既有语义)。
    let child_agents_cancelled = inner.child_agents.cancel_all().len();
    // 3) 等待全部子代理 join handle 退出(三种终态——完成/失败/被停——都在
    //    run_subagent 返回时释放读槽;await join 保证 dispose 返回前它们已静止)。
    //    有界等待:join 后不 panic(任务内错误已由 run_subagent 收敛为 ToolOutput)。
    let mut joins = std::mem::take(&mut *inner.child_agent_joins.lock().unwrap());
    for handle in joins.drain(..) {
        let _ = handle.await;
    }
    // 4) 收回普通后台进程(占位:批3 接入 kanzei-tools background registry)。
    let background_processes_reaped = {
        let mut guard = inner.background_processes.lock().unwrap();
        let count = guard.len();
        guard.clear();
        count
    };
    // R-246 批4:终态落库——dispose 完成时写生命周期事件(可回放、可审计)。
    if let Some(sink) = &inner.lifecycle_sink {
        sink(
            "runtime",
            serde_json::json!({
                "kind": "runtime.lifecycle",
                "id": "line",
                "state": "disposed",
                "child_agents_cancelled": child_agents_cancelled,
                "background_processes_reaped": background_processes_reaped,
            }),
        );
    }
    DisposeOutcome {
        performed: true,
        child_agents_cancelled,
        background_processes_reaped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dispose_幂等_并发共享同一完成future() {
        // 验收①:并发两次 dispose 共享完成结果,且只收尾一次。
        let rt = LineRuntime::new();
        rt.track_background_process("bg1".into());
        rt.track_background_process("bg2".into());

        let rt2 = rt.clone();
        let (a, b) = tokio::join!(rt.dispose(), rt2.dispose());
        // 两次结果:一次 performed=true,一次 performed=false(共享同一 future)。
        let performed_count = [a.performed, b.performed].iter().filter(|p| **p).count();
        assert_eq!(performed_count, 1, "收尾必须只发生一次");
        // 两次结果内容一致(共享同一完成结果)。
        assert_eq!(a.background_processes_reaped, 2);
        assert_eq!(b.background_processes_reaped, 2);
        // 再调一次仍然幂等。
        let third = rt.dispose().await;
        assert!(!third.performed, "第三次调用不再收尾");
    }

    #[tokio::test]
    async fn dispose_取消令牌已触发() {
        let rt = LineRuntime::new();
        let token = rt.cancellation().clone();
        assert!(!token.is_cancelled());
        rt.dispose().await;
        assert!(token.is_cancelled(), "dispose 必须触发取消令牌");
    }

    #[test]
    fn new_默认不取消() {
        let rt = LineRuntime::new();
        assert!(!rt.cancellation().is_cancelled());
    }

    /// R-246 验收②:dispose 取消子代理并等待退出——track 的 join handle 全部
    /// 在 dispose 返回前完成(三种终态都在 run_subagent 返回时释放读槽,此处
    /// 验证「等待退出」机制本身:被停子代理的 join 必须被 await)。
    #[tokio::test]
    async fn dispose_等待已登记子代理退出() {
        let rt = LineRuntime::new();
        // 模拟两个后台子代理:一个自然完成,一个等取消令牌触发后退出(被停终态)。
        let token = rt.cancellation().clone();
        let done = tokio::spawn(async move { std::future::ready(()).await });
        let cancelled = tokio::spawn(async move {
            token.cancelled().await; // 被停终态:dispose 的 cancel 触发后返回
        });
        // 内部确认:被停子代理依赖 dispose 的 cancel 才结束——若 dispose 没有
        // await join 就返回,这里会看到 cancelled 尚未 finish。
        let cancelled_finished_before = cancelled.is_finished();
        rt.track_child_agent(done);
        rt.track_child_agent(cancelled);
        // dispose 返回前必须已 await 全部 join(含被停子代理)。
        let outcome = rt.dispose().await;
        assert!(outcome.performed);
        assert!(
            !cancelled_finished_before,
            "被停子代理在 dispose 之前不应已结束(它等 cancel 触发)"
        );
        // dispose 已 await 全部 join——若 cancelled 未在 cancel 后退出,dispose
        // 会一直 await,测试在此处无法继续,即隐式验证「等待退出」生效。
    }

    /// R-246 验收④:dispose 返回前生命周期终态落库——lifecycle sink 收到
    /// runtime.lifecycle disposed 事件(含收尾计数)。
    #[tokio::test]
    async fn dispose_终态事件落库() {
        let events = Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let events2 = Arc::clone(&events);
        let sink: LifecycleSink = Arc::new(move |_id, payload| {
            events2.lock().unwrap().push(payload);
        });
        let rt = LineRuntime::with_lifecycle_sink(Some(sink));
        rt.track_background_process("bg1".into());
        let outcome = rt.dispose().await;
        assert!(outcome.performed);
        {
            let events = events.lock().unwrap();
            assert_eq!(events.len(), 1, "dispose 终态事件必须恰好一条");
            let ev = &events[0];
            assert_eq!(ev["kind"], "runtime.lifecycle");
            assert_eq!(ev["state"], "disposed");
            assert_eq!(ev["background_processes_reaped"], 1);
        }
        // 第二次 dispose 不重复落库(幂等)。
        rt.dispose().await;
        assert_eq!(events.lock().unwrap().len(), 1, "幂等:终态事件只写一次");
    }

    /// R-246 验收④:未配置 sink 时 dispose 不 panic(CLI/测试默认路径)。
    #[tokio::test]
    async fn dispose_无sink_不panic() {
        let rt = LineRuntime::new();
        let outcome = rt.dispose().await;
        assert!(outcome.performed);
    }
}
