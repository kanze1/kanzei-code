//! 自主推进(auto-run,即「鞭挞」)状态机策略:轮末该继续、该追加推进指令、
//! 还是该停——全部由引擎判定,不依赖提示词恳求(D-120/D-128/D-163 教训:
//! 规则写在用户可编辑文案里会与引擎行为脱节)。
//!
//! 这些判定原本全在桌面端前端 JS(08-compose.js / 07-events.js):空转工具画像、
//! 连续推进的旧版次数上限、全部阻塞/清空停止、无动作 NUDGE、暂停/本轮后停/停止原因。
//! 本模块把它们下沉为**纯逻辑状态机**(无 IO):kanzei-core runner 轮末消费,
//! 桌面端与 CLI 共用同一套判定(D-229 类「能力只在桌面端」的架构债消除),
//! UI 只保留控件与状态回显。设计见 docs/design/continue_prompt_dissection.md §4。

/// 不构成实质进展的工具:一轮里只有这些(纯查询/探测/写记忆日记)时仍算空转——
/// 模型不能再靠 memory_note 或无关读取绕过刹车(D-044 教训的硬化,原 R-076 画像)。
/// bash/git/edit/write/tracker 等可能改变状态的工具不在列:名称粒度分不出
/// git status 与 git commit,误判成空转的代价(真干活被打断)比漏判高。
///
/// `task` 在列,但它的语义与其余成员不同,别照字面理解:**派子代理本身不算进展,
/// 子代理干的活算**。调用方在轮末必须把子代理内部用过的工具名并进画像再传进来
/// (桌面端见 run/mod.rs 的 subagent_round_tool 与 run/coordinator.rs 的 subagent_tools)，否则「整轮把活派出去」
/// 会只剩一个 task 留在画像里、被判空转——第一轮 Nudge、第二轮 Stop(NoAction),
/// 越守规矩地委派越快自停(D-361 现场)。上卷之后语义才自洽:子代理真干了活,
/// 画像里就有它调的 edit/bash;子代理也空手而归,画像里仍只有 task,刹车照旧生效。
pub const NON_PROGRESS_TOOLS: &[&str] = &[
    "memory_note",
    "memory_search",
    "memory_stats",
    "read",
    "grep",
    "glob",
    "webfetch",
    "ui_dom",
    "ui_console",
    "ui_style",
    "frontend_locate",
    "frontend_check",
    "tool_search",
    "task",
];

/// 本轮工具画像是否包含实质进展工具。
pub fn has_progress_tools(tools: &[String]) -> bool {
    tools
        .iter()
        .any(|name| !NON_PROGRESS_TOOLS.contains(&name.as_str()))
}

/// 停止原因(枚举;文案由 UI 层按枚举映射,引擎不产生用户可读文案)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoStopReason {
    /// 用户暂停。
    Paused,
    /// 用户勾选「本轮后停」(一次性意图,不持久化)。
    StopAfterRound,
    /// 连续两轮无实质动作。
    NoAction,
    /// 当前档位不允许自动续跑，停止由共享控制器判定。
    ProfileMismatch,
    /// 连续 N 轮瞬态失败(退避重试仍失败),停止并通知。
    RepeatedFailure(u32),
    /// 本轮以致命错误失败(认证/配置类),立即停。
    FatalError,
    /// provider 明确返回限流/过载(通常是 HTTP 429)。
    RateLimited,
    /// 连续 ZERO_OUTPUT_ROUND_LIMIT 轮真实进展签名未变。
    ZeroOutput(u32),
    /// 模型声明当前请求完成。
    ModelDeclaredDone,
    /// 用户给定的目标条件已达成(模型判定并声明),目标随之自动清除。
    GoalMet,
    /// 目标条件挂着,但连续 GOAL_IDLE_ROUND_LIMIT 轮没有实质动作。
    GoalUnreachable(u32),
    /// 模型在等用户回答。
    AwaitingUser,
}

/// 轮末判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoRunAction {
    /// 正常续跑(计数已 +1)。
    Continue,
    /// 无动作第一次:追加一条具体推进指令(计数已 +1,占一轮)。
    Nudge,
    /// 停止(保留累计轮次;携带原因供 UI 展示)。
    Stop(AutoStopReason),
    /// 用户拒绝/手动停止本轮:不续跑、不重置计数(等手动输入重新武装)。
    NoContinue,
    /// 本轮瞬态失败,退避后重试。
    RetryAfterFailure { attempt: u32 },
    /// 目标条件尚未达成,再推一轮(计数已 +1)。
    GoalPending,
}

/// 引擎生成的续跑检查点文案，不扩大原委托授权范围。
pub fn nudge_prompt() -> &'static str {
    "自动续跑检查点：根据当前委托和现有证据，自主决定下一步或交付结果。\n"
}

/// 自主推进状态:跨轮计数与用户一次性意图。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutoRunState {
    /// 本轮内已自动续跑的轮数(手动发送归零)。
    pub rounds: u32,
    /// 用户暂停。
    pub paused: bool,
    /// 「本轮后停」一次性意图(D-111:绝不持久化)。
    pub stop_after_round: bool,
    /// 连续无实质动作的轮数:第一次追加推进指令,第二次才停。
    no_action_rounds: u32,
    /// D-403:连续瞬态失败的轮数。成功轮归零;达 MAX_FAILED_ROUNDS 即停。
    failed_rounds: u32,
    /// D-583:连续几轮「真实进展签名」未变。签名由调用方按 (HEAD、代码 worktree
    /// hash、tracker 文档内容) 拼出,只在这里做纯比较——has_progress_tools 只看
    /// 工具名,一轮调用 bash 反复 cat 同一份证据清单也算「有进展工具」,穿不透那道
    /// 检测;这里比对的是真实状态有没有变,堵的是这类"看起来在干活"的空转。
    zero_output_rounds: u32,
    /// D-583:上一轮记录的真实进展签名,None = 尚未记录(刚 reset 或全新状态)。
    last_progress_signature: Option<String>,
    /// R-322 B3:目标挂着期间连续无实质动作的轮数。达 GOAL_IDLE_ROUND_LIMIT 即停。
    goal_idle_rounds: u32,
}

/// D-403:连续瞬态失败多少轮后停止(过夜场景:单发 503 退避重试可自愈,
/// provider 长时间不可用则不再空转烧钱,停止并通知)。
pub const MAX_FAILED_ROUNDS: u32 = 3;

/// D-583:连续几轮真实进展签名不变即熔断停鞭(现场实测 R-306 空转 10 轮无人发现;
/// 验收建议 2~3,取上限留够「连续两轮都在做同一件事的合法收尾」的余量)。
pub const ZERO_OUTPUT_ROUND_LIMIT: u32 = 3;

/// R-322 B3:目标挂着时连续几轮无实质动作即判「目标推不动」。
///
/// 取值与 ZERO_OUTPUT_ROUND_LIMIT 一致但**语义不同**:那个比对磁盘真实签名,
/// 这个看工具画像。目标 loop 关掉了 NoAction 刹车,必须另有一道兜底——
/// 条件写得含糊(「优化一下」)或根本达不到时,不能让它一直推下去。
pub const GOAL_IDLE_ROUND_LIMIT: u32 = 3;

impl AutoRunState {
    /// 手动发送/用户操作时归零(原前端 autoRounds=0; noActionRounds=0)。
    pub fn reset(&mut self) {
        self.rounds = 0;
        self.no_action_rounds = 0;
        self.failed_rounds = 0;
        self.zero_output_rounds = 0;
        self.last_progress_signature = None;
        self.goal_idle_rounds = 0;
    }

    /// 轮末判定。判定顺序与前端 07-events.js:288-352 保持一致,但不再受旧版连续次数上限影响:
    /// ①backlog 全阻塞/清空最优先;②用户拒绝;③暂停;④本轮后停;⑤失败;
    /// ⑥模型声明完成;⑦无动作/真实进展/核查等安全边界;⑧正常续跑。
    /// R-199:档位检查在 backlog 之后——模式不匹配时引擎 Stop(ProfileMismatch)
    /// 前端不再有第二次否决;终态保留已完成轮次,仅显式重启/手动重置归零。
    ///
    /// R-322 起完整顺序(**分界线是「谁有信息做这个判断」,不是重要程度**):
    ///
    /// ```text
    /// 资源/用户段(模型无权否决):
    ///   backlog(仅自主档:那一档的活来自队列) → auto_allowed → halted → paused
    ///   → stop_after_round → round_failure
    /// 完成声明段(核对范围和目标,局部交付继续):
    ///   → completion
    /// 等待用户段：awaiting_user 优先于目标与任务推进判断。
    /// 目标段：goal_active → 状态签名/无动作预算 → GoalPending。
    /// 普通推进段：no_action/Nudge(由强度决定) → zero_output → Continue
    ///
    /// ```
    ///
    /// `zero_output`(D-583)看着像任务判断,其实不是:它比对 (HEAD、worktree hash、
    /// tracker 内容) 的真实签名,不解释语义,只回答「磁盘上还在不在变」。
    /// 无人值守时这是唯一的失控兜底,**两档都保留**,别按强度关掉它。
    pub fn decide(&mut self, ctx: &AutoRunCtx) -> AutoRunAction {
        let policy = ctx.intensity.policy();
        if !ctx.auto_allowed {
            return self.stop_with(AutoStopReason::ProfileMismatch);
        }
        if ctx.halted {
            return AutoRunAction::NoContinue;
        }
        if self.paused {
            return self.stop_with(AutoStopReason::Paused);
        }
        if self.stop_after_round {
            return self.stop_with(AutoStopReason::StopAfterRound);
        }
        if let Some(failure) = ctx.round_failure {
            self.no_action_rounds = 0;
            return match failure {
                RoundFailure::Fatal => self.stop_with(AutoStopReason::FatalError),
                RoundFailure::RateLimited => self.stop_with(AutoStopReason::RateLimited),
                RoundFailure::Transient => {
                    self.failed_rounds += 1;
                    if self.failed_rounds >= MAX_FAILED_ROUNDS {
                        self.stop_with(AutoStopReason::RepeatedFailure(self.failed_rounds))
                    } else {
                        self.rounds = self.rounds.saturating_add(1);
                        AutoRunAction::RetryAfterFailure {
                            attempt: self.failed_rounds,
                        }
                    }
                }
            };
        }
        self.failed_rounds = 0;
        match ctx.completion {
            Some(crate::handoff::HandoffScope::Goal) if ctx.goal_active => {
                return self.stop_with(AutoStopReason::GoalMet);
            }
            Some(crate::handoff::HandoffScope::Request)
                if !ctx.goal_active && !policy.engine_nudge =>
            {
                return self.stop_with(AutoStopReason::ModelDeclaredDone);
            }
            _ => {}
        }
        // UI2-0926 #13:模型在等用户回答——停机权的另一种形态。放在所有任务判断
        // (Nudge/ZeroOutput/GoalPending)之前:问题挂着的时候再推一轮,
        // 模型只能要么复述问题、要么替用户拍板,两样都是错的(「MD文件保存」现场:模型在要
        // 仓库路径,引擎先 Continue 再 Nudge)。两档一致;目标挂着也停,但不清除目标。
        if ctx.awaiting_user {
            return self.stop_with(AutoStopReason::AwaitingUser);
        }
        let no_action = ctx.steps <= 1 || !has_progress_tools(ctx.tools);
        // R-322 B3:目标挂着 → 一轮没动作**不停**,复述用户给的条件再推一轮。
        // 引擎在这里不发明任何工作,只是不让它在条件达成前散场;真正判断
        // 「达成了没有」的仍然是模型(经 work handoff 声明)。
        // 兜底:连续 GOAL_IDLE_ROUND_LIMIT 轮推不动就停,条件多半含糊或不可达。
        if ctx.goal_active {
            // 目标 loop 关掉了 NoAction 刹车,D-583 的零产出熔断就成了唯一挡得住
            // 「每轮都调工具、磁盘一个字节没变」的防线,必须先过它再谈继续。
            if let Some(stop) = self.note_progress_signature(ctx) {
                return stop;
            }
            if no_action {
                self.goal_idle_rounds += 1;
                if self.goal_idle_rounds >= GOAL_IDLE_ROUND_LIMIT {
                    let rounds = self.goal_idle_rounds;
                    return self.stop_with(AutoStopReason::GoalUnreachable(rounds));
                }
            } else {
                self.goal_idle_rounds = 0;
            }
            self.no_action_rounds = 0;
            self.rounds = self.rounds.saturating_add(1);
            return AutoRunAction::GoalPending;
        }
        if no_action && self.rounds > 0 {
            // R-322:Nudge 是任务判断,只在无人监督档借给引擎。结伴档下模型
            // 一轮没动作就是没动作(多半是在回答用户的问题),不该被推着找活干。
            if !policy.engine_nudge {
                return self.stop_with(AutoStopReason::NoAction);
            }
            if self.no_action_rounds == 0 {
                self.no_action_rounds = 1;
                self.rounds = self.rounds.saturating_add(1);
                return AutoRunAction::Nudge;
            }
            return self.stop_with(AutoStopReason::NoAction);
        }
        self.no_action_rounds = 0;
        if let Some(stop) = self.note_progress_signature(ctx) {
            return stop;
        }
        self.rounds = self.rounds.saturating_add(1);
        AutoRunAction::Continue
    }

    /// D-583:工具画像有「进展工具」但真实状态连续 N 轮未变——纯复诵证据清单、
    /// 反复读同一批文件也会调用 bash/read,穿得过基于工具名的检测,只有比对真实
    /// 签名才拦得住。签名相同才计数;换了就清零重新起算,不跨越已中断的沉默期。
    /// 空字符串是调用方未接线时的哨兵值(测试桩/尚未接入的调用方),视为不追踪——
    /// 生产侧签名由真实哈希拼出,不会自然产出空串,不影响真实场景。
    ///
    /// R-322 B3 抽成方法:常规路径与目标 loop 共用同一实现。目标 loop 必须**先**
    /// 过这道熔断——它关掉了 NoAction 刹车,这里是唯一挡得住无限空转的防线。
    /// 返回 `Some(Stop)` = 熔断触发,调用方立即返回。
    fn note_progress_signature(&mut self, ctx: &AutoRunCtx) -> Option<AutoRunAction> {
        if ctx.progress_signature.is_empty() {
            return None;
        }
        if self.rounds > 0
            && self.last_progress_signature.as_deref() == Some(ctx.progress_signature)
        {
            self.zero_output_rounds += 1;
            if self.zero_output_rounds >= ZERO_OUTPUT_ROUND_LIMIT {
                let rounds = self.zero_output_rounds;
                return Some(self.stop_with(AutoStopReason::ZeroOutput(rounds)));
            }
        } else {
            self.zero_output_rounds = 0;
            self.last_progress_signature = Some(ctx.progress_signature.to_string());
        }
        None
    }

    fn stop_with(&mut self, reason: AutoStopReason) -> AutoRunAction {
        self.rounds = self.rounds.saturating_add(1);
        self.no_action_rounds = 0;
        self.failed_rounds = 0;
        self.zero_output_rounds = 0;
        self.last_progress_signature = None;
        self.goal_idle_rounds = 0;
        AutoRunAction::Stop(reason)
    }
}

/// R-322:门禁强度——引擎对**任务判断**介入多深。
///
/// # 为什么需要这个维度
///
/// 2026-08-21 的外部评估指出:结伴开发与过夜自主推进共用同一套门禁机械,
/// 差异只落在系统提示词,于是用户「看不见自己选了什么」,而重门禁的成本
/// (Harness Tax)在有人监督的小任务上纯属浪费。用户定调:
/// **结伴接近高自治,自主推进保留重门禁,且决策点要呈现给用户。**
///
/// # 分档依据不是「重要程度」,是「谁有信息做这个判断」
///
/// 轮末判定拆成两类,只有前一类随强度变化:
///
/// - **任务判断**(任务完成了吗、下一步做什么):需要语义理解。引擎只有工具名和
///   锁键,信息量本就不足;有人监督时该由人和模型决定,无人监督时才借给引擎兜底。
/// - **资源判断**(限流了吗、连跑多少轮了、真实状态还在不在变):与语义无关,
///   纯机械可判定,**两档都保留**——无人值守时没有第二个判断者。
///
/// 权限硬门禁、托管围栏与事件真源**不在本维度内**:它们约束的是副作用边界,
/// 与「模型有多自治」正交,任何强度下都不放松。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HarnessIntensity {
    /// 结伴开发:用户在场监督。模型自治优先,引擎只保留资源类兜底。
    Paired,
    /// 自主推进:无人监督。全套门禁——目标是抑制长时间无人值守的信息熵增。
    #[default]
    Autonomous,
}

impl std::str::FromStr for HarnessIntensity {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "paired" => Ok(HarnessIntensity::Paired),
            "autonomous" => Ok(HarnessIntensity::Autonomous),
            other => Err(format!(
                "unknown harness intensity `{other}` (paired|autonomous)"
            )),
        }
    }
}

impl HarnessIntensity {
    pub fn as_str(self) -> &'static str {
        match self {
            HarnessIntensity::Paired => "paired",
            HarnessIntensity::Autonomous => "autonomous",
        }
    }

    /// 该强度下引擎的介入策略。**新增机制时在这里加字段,不要再长一个布尔开关**
    pub fn policy(self) -> IntensityPolicy {
        match self {
            HarnessIntensity::Paired => IntensityPolicy {
                engine_nudge: false,
            },
            HarnessIntensity::Autonomous => IntensityPolicy { engine_nudge: true },
        }
    }
}

/// 强度展开后的逐机制开关。只覆盖**任务判断**类机制;资源类兜底不在此列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntensityPolicy {
    /// 引擎能否在模型本轮无实质动作时追加推进指令(Nudge)。
    /// false = 模型说没什么可做就是没什么可做,引擎不制造工作。
    pub engine_nudge: bool,
}

/// 轮末判定输入。
#[derive(Clone, Debug)]
pub struct AutoRunCtx<'a> {
    /// 用户拒绝/手动停止(前端 kz:done 的 halted 字段)。
    pub halted: bool,
    /// 本轮门禁强度，决定无动作续跑提示。
    pub intensity: HarnessIntensity,
    /// 已由调用方核对当前请求/目标身份的完成范围。局部事项和批次只记进度。
    pub completion: Option<crate::handoff::HandoffScope>,
    /// 模型本轮在等用户回答。
    pub awaiting_user: bool,
    /// 是否挂着用户给定的目标条件。
    pub goal_active: bool,
    /// 本轮工具调用轮数。
    pub steps: u32,
    /// 本轮实际调用的工具名列表(供空转画像判定)。
    pub tools: &'a [String],
    /// 当前模式是否允许自主推进。
    pub auto_allowed: bool,
    /// 本轮运行失败及其性质;None = 本轮正常完成。
    pub round_failure: Option<RoundFailure>,
    /// 本轮「真实进展」签名——与上一轮的值逐字节比较。
    pub progress_signature: &'a str,
}

/// D-403:失败轮的性质分类(由调用方按 LlmError 变体判定)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundFailure {
    /// 瞬态(限流/过载/5xx/传输中断):退避后值得重试。
    Transient,
    /// provider 已明确限流/过载；继续重试会放大订阅消耗，立即停止自动链。
    RateLimited,
    /// 致命(认证/配置/协议/上下文压缩已尽仍溢出):重试只会原样复现,立即停。
    Fatal,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_tools(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn ctx_with_tools(tools: &[String]) -> AutoRunCtx<'_> {
        AutoRunCtx {
            halted: false,
            intensity: HarnessIntensity::Autonomous,
            completion: None,
            awaiting_user: false,
            goal_active: false,
            steps: 1,
            tools,
            auto_allowed: true,
            round_failure: None,
            progress_signature: "",
        }
    }

    /// D-403:瞬态失败退避重试,连续 MAX_FAILED_ROUNDS 轮才停;成功轮清零;
    /// 致命失败立即停;失败轮不吃 NoAction 刹车(steps=1 也不判空转)。
    #[test]
    fn 失败轮_瞬态退避重试_连续三轮停_致命立即停_成功清零() {
        let tools = mk_tools(&["edit"]);
        let mut state = AutoRunState::default();
        let mut fail = ctx_with_tools(&tools);
        fail.round_failure = Some(RoundFailure::Transient);
        assert_eq!(
            state.decide(&fail),
            AutoRunAction::RetryAfterFailure { attempt: 1 }
        );
        assert_eq!(
            state.decide(&fail),
            AutoRunAction::RetryAfterFailure { attempt: 2 }
        );
        // 中间一轮成功:连续失败清零,下次失败从 attempt=1 重新起算。
        let mut ok = ctx_with_tools(&tools);
        ok.steps = 2;
        assert_eq!(state.decide(&ok), AutoRunAction::Continue);
        assert_eq!(
            state.decide(&fail),
            AutoRunAction::RetryAfterFailure { attempt: 1 },
            "成功轮必须清零连续失败计数"
        );
        assert_eq!(
            state.decide(&fail),
            AutoRunAction::RetryAfterFailure { attempt: 2 }
        );
        // 连续第 MAX_FAILED_ROUNDS 轮:停止并携带次数,计数重置。
        assert_eq!(
            state.decide(&fail),
            AutoRunAction::Stop(AutoStopReason::RepeatedFailure(MAX_FAILED_ROUNDS))
        );
        assert_eq!(
            state.decide(&fail),
            AutoRunAction::RetryAfterFailure { attempt: 1 },
            "停止已重置计数,新失败重新起算"
        );
        // 致命失败:立即停,不退避不占次数。
        let mut fatal = ctx_with_tools(&tools);
        fatal.round_failure = Some(RoundFailure::Fatal);
        assert_eq!(
            state.decide(&fatal),
            AutoRunAction::Stop(AutoStopReason::FatalError)
        );

        let mut limited = ctx_with_tools(&tools);
        limited.round_failure = Some(RoundFailure::RateLimited);
        assert_eq!(
            state.decide(&limited),
            AutoRunAction::Stop(AutoStopReason::RateLimited)
        );
    }

    /// D-361:委派轮不是空转轮。子代理内部用过的工具由调用方上卷进画像后,
    /// 「整轮只调 task」的委派必须能正常续跑;子代理也没动作时刹车照旧。
    #[test]
    fn 委派轮上卷子代理画像后不判空转() {
        let mut state = AutoRunState {
            rounds: 1, // no_action 只在 rounds > 0 时才触发,先跑过一轮
            ..Default::default()
        };
        // 主轮只留下一个 task 调用,上卷后画像里带上子代理真正调过的 edit。
        let tools = mk_tools(&["task", "edit"]);
        let ctx = AutoRunCtx {
            steps: 2,
            ..ctx_with_tools(&tools)
        };
        assert_eq!(
            state.decide(&ctx),
            AutoRunAction::Continue,
            "子代理干了实质活的委派轮必须续跑,不能判成空转"
        );
    }

    /// D-361 反证:上卷之后刹车不能被削弱——子代理空手而归时画像里仍只有 task,
    /// 依旧走无动作路径(第一次 Nudge,第二次 Stop(NoAction))。
    #[test]
    fn 子代理也没动作的委派轮仍判无动作() {
        let mut state = AutoRunState {
            rounds: 1,
            ..Default::default()
        };
        let tools = mk_tools(&["task"]);
        let ctx = AutoRunCtx {
            steps: 2,
            ..ctx_with_tools(&tools)
        };
        assert_eq!(
            state.decide(&ctx),
            AutoRunAction::Nudge,
            "子代理也空转时第一次应 Nudge"
        );
        assert_eq!(
            state.decide(&ctx),
            AutoRunAction::Stop(AutoStopReason::NoAction),
            "连续两轮无动作仍须停,上卷不得削弱刹车"
        );
    }

    /// D-361:task 单独出现不算进展,与子代理上卷的工具名区分开。
    #[test]
    fn task_单独不算进展工具_上卷后算() {
        assert!(
            !has_progress_tools(&mk_tools(&["tool_search", "memory_search"])),
            "tool discovery and memory lookup alone do not count as progress"
        );
        assert!(
            !has_progress_tools(&mk_tools(&["task"])),
            "只派子代理、子代理什么也没干,不算实质进展"
        );
        assert!(
            has_progress_tools(&mk_tools(&["task", "edit"])),
            "子代理调过 edit,上卷后整轮必须算实质进展"
        );
        assert!(
            !has_progress_tools(&mk_tools(&["task", "read", "grep"])),
            "子代理只读不写,仍是查询类画像,不算进展"
        );
    }

    /// R-199:模式不允许自主推进时引擎 Stop(ProfileMismatch)且计数不 +1(重置)。
    #[test]
    fn 模式不匹配时引擎停止且计数不漂移() {
        let mut state = AutoRunState {
            rounds: 3, // 假设已跑了 3 轮
            ..Default::default()
        };
        let tools = mk_tools(&["edit", "bash"]);
        let ctx = AutoRunCtx {
            auto_allowed: false,
            ..ctx_with_tools(&tools)
        };
        assert_eq!(
            state.decide(&ctx),
            AutoRunAction::Stop(AutoStopReason::ProfileMismatch),
            "模式不匹配必须 Stop(ProfileMismatch)"
        );
        assert_eq!(state.rounds, 4, "停机保留已执行轮次,与前端回显一致");
    }

    /// D-680/R-322:只有成功收口的显式完成声明才停止；临时无动作不应伪造该信号。
    #[test]
    fn d680_显式完成声明才停止鞭挞() {
        let mut state = AutoRunState::default();
        let ctx = AutoRunCtx {
            intensity: HarnessIntensity::Paired,
            completion: Some(crate::handoff::HandoffScope::Request),
            tools: &mk_tools(&["work"]),
            ..ctx_with_tools(&[])
        };
        assert_eq!(
            state.decide(&ctx),
            AutoRunAction::Stop(AutoStopReason::ModelDeclaredDone)
        );
        assert_eq!(state.rounds, 1, "显式完成停止后保留累计轮次");
    }

    #[test]
    fn 有实质动作的轮次_正常续跑并计数() {
        let mut state = AutoRunState::default();
        let ctx = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["read", "edit", "bash"]),
            ..ctx_with_tools(&[])
        };
        assert_eq!(state.decide(&ctx), AutoRunAction::Continue);
        assert_eq!(state.rounds, 1);
        // 连续多轮有动作:一直续跑。
        let ctx2 = AutoRunCtx {
            steps: 3,
            tools: &mk_tools(&["edit", "bash"]),
            ..ctx_with_tools(&[])
        };
        assert_eq!(state.decide(&ctx2), AutoRunAction::Continue);
        assert_eq!(state.rounds, 2);
    }

    #[test]
    fn 只读或记忆日记类工具画像_判为空转轮() {
        // 纯查询 + memory_note:画像判定无实质进展。
        let ctx = AutoRunCtx {
            steps: 4,
            tools: &mk_tools(&["memory_note", "read", "grep", "ui_dom"]),
            ..ctx_with_tools(&[])
        };
        let mut state = AutoRunState::default();
        assert!(!has_progress_tools(&mk_tools(&[
            "memory_note",
            "read",
            "ui_dom"
        ])));
        assert!(has_progress_tools(&mk_tools(&["edit"])));
        // 首轮(rounds=0)无动作不 NUDGE 不停:直接续(前端 `noAction && autoRounds > 0` 语义)。
        assert_eq!(state.decide(&ctx), AutoRunAction::Continue);
        assert_eq!(state.rounds, 1);
    }

    #[test]
    fn 连续无动作_第一次追加推进指令_第二次才停() {
        let mut state = AutoRunState::default();
        // 第 1 轮:有动作(武装 rounds=1)。
        let ok = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["edit"]),
            ..ctx_with_tools(&[])
        };
        assert_eq!(state.decide(&ok), AutoRunAction::Continue);
        // 第 2 轮:无动作 → NUDGE。
        let bad = AutoRunCtx {
            steps: 1,
            tools: &mk_tools(&["read"]),
            ..ctx_with_tools(&[])
        };
        assert_eq!(state.decide(&bad), AutoRunAction::Nudge);
        assert_eq!(state.rounds, 2);
        // 第 3 轮:仍无动作 → 停。
        assert_eq!(
            state.decide(&bad),
            AutoRunAction::Stop(AutoStopReason::NoAction)
        );
        assert_eq!(state.rounds, 3, "停止后保留累计轮次");
    }

    #[test]
    fn 旧连续次数上限不再阻断鞭挞_但轮次继续计数() {
        let mut state = AutoRunState::default();
        let ok = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["edit"]),
            ..ctx_with_tools(&[])
        };
        assert_eq!(state.decide(&ok), AutoRunAction::Continue); // rounds=1
        assert_eq!(state.decide(&ok), AutoRunAction::Continue); // rounds=2
        assert_eq!(state.decide(&ok), AutoRunAction::Continue); // 旧上限不再阻断
        assert_eq!(state.rounds, 3, "轮次仍应持续计数而不是被旧上限归零");
    }

    #[test]
    fn 暂停时停止_恢复后继续() {
        let mut state = AutoRunState {
            paused: true,
            ..Default::default()
        };
        let ok = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["edit"]),
            ..ctx_with_tools(&[])
        };
        assert_eq!(
            state.decide(&ok),
            AutoRunAction::Stop(AutoStopReason::Paused)
        );
        state.paused = false;
        assert_eq!(state.decide(&ok), AutoRunAction::Continue);
    }

    #[test]
    fn 本轮后停_是一次性意图且不持久化状态() {
        let mut state = AutoRunState {
            stop_after_round: true,
            ..Default::default()
        };
        let ok = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["edit"]),
            ..ctx_with_tools(&[])
        };
        assert_eq!(
            state.decide(&ok),
            AutoRunAction::Stop(AutoStopReason::StopAfterRound)
        );
        // 本轮后停不改变 stop_after_round 字段本身:持久化与否由调用方决定(D-111)。
        assert!(state.stop_after_round);
    }

    #[test]
    fn 用户拒绝_halted_不续跑也不重置计数() {
        let mut state = AutoRunState {
            rounds: 5,
            ..Default::default()
        };
        let ctx = AutoRunCtx {
            halted: true,
            ..ctx_with_tools(&[])
        };
        assert_eq!(state.decide(&ctx), AutoRunAction::NoContinue);
        assert_eq!(state.rounds, 5, "halted 不重置:等手动输入重新武装");
    }

    #[test]
    fn 手动输入_reset_归零() {
        let mut state = AutoRunState {
            rounds: 7,
            ..Default::default()
        };
        let ok = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["edit"]),
            ..ctx_with_tools(&[])
        };
        state.decide(&ok); // rounds=8
        state.reset();
        assert_eq!(state.rounds, 0);
        assert_eq!(state.decide(&ok), AutoRunAction::Continue);
        assert_eq!(state.rounds, 1);
    }

    /// D-583:连续 ZERO_OUTPUT_ROUND_LIMIT 轮真实进展签名不变即熔断——即使每轮都
    /// 调用了 bash(会通过 has_progress_tools),真实状态没变仍要拦下。
    #[test]
    fn 连续三轮真实签名不变_即使工具画像正常也熔断停鞭() {
        let tools = mk_tools(&["bash"]);
        let mut state = AutoRunState::default();
        let mut stuck = ctx_with_tools(&tools);
        stuck.steps = 2; // 排除 steps<=1 的既有空转判据,单独测 D-583 的签名比对
        stuck.progress_signature = "same-sig";
        // 第1轮只起算签名(基线,不计入「未变」次数);此后每轮签名与上一轮相同才 +1,
        // 连续 ZERO_OUTPUT_ROUND_LIMIT 次「未变」(即第 1+LIMIT 轮)达阈值熔断。
        assert_eq!(
            state.decide(&stuck),
            AutoRunAction::Continue,
            "第1轮起算签名"
        );
        assert_eq!(
            state.decide(&stuck),
            AutoRunAction::Continue,
            "签名未变,计1次"
        );
        assert_eq!(
            state.decide(&stuck),
            AutoRunAction::Continue,
            "签名未变,计2次"
        );
        assert_eq!(
            state.decide(&stuck),
            AutoRunAction::Stop(AutoStopReason::ZeroOutput(ZERO_OUTPUT_ROUND_LIMIT)),
            "签名未变,计3次,达阈值熔断"
        );
        assert_eq!(state.rounds, 4, "熔断后保留累计轮次");
    }

    /// D-583:签名一旦变化(真实文件/提交/tracker 有变动)就清零重新起算,不会
    /// 因为「历史上出现过相同签名」被跨轮误伤。
    #[test]
    fn 真实签名变化后清零_不会跨轮误触熔断() {
        let tools = mk_tools(&["bash"]);
        let mut state = AutoRunState::default();
        let mut ctx = ctx_with_tools(&tools);
        ctx.steps = 2;
        ctx.progress_signature = "sig-a";
        assert_eq!(state.decide(&ctx), AutoRunAction::Continue);
        assert_eq!(state.decide(&ctx), AutoRunAction::Continue);
        ctx.progress_signature = "sig-b";
        assert_eq!(
            state.decide(&ctx),
            AutoRunAction::Continue,
            "签名变化,不计入连续未变"
        );
        ctx.progress_signature = "sig-a";
        assert_eq!(
            state.decide(&ctx),
            AutoRunAction::Continue,
            "回到旧签名值不算复发——只看与上一轮的比较,不是历史集合"
        );
    }

    /// D-583:空字符串是「调用方未接线」的哨兵,不追踪——防止既有测试桩(默认
    /// progress_signature 为空)被这项新检测误伤,生产侧签名恒为真实哈希不会触发。
    #[test]
    fn 空签名视为不追踪_不会熔断() {
        let tools = mk_tools(&["bash"]);
        let mut state = AutoRunState::default();
        let mut ctx = ctx_with_tools(&tools);
        ctx.steps = 2;
        assert_eq!(ctx.progress_signature, "");
        for _ in 0..5 {
            assert_eq!(state.decide(&ctx), AutoRunAction::Continue);
        }
    }

    /// UI2-0926 #13:等用户是模型的停机权——两档都停,即使本轮有进展工具;
    /// 排在 Nudge/GoalPending/ZeroOutput 之前,排在资源/用户段之后;目标不被清除。
    #[test]
    fn 模型在等用户_两档都停_优先于任务判断() {
        let tools = mk_tools(&["edit", "bash"]);
        for intensity in [HarnessIntensity::Autonomous, HarnessIntensity::Paired] {
            let mut state = AutoRunState {
                rounds: 3,
                ..Default::default()
            };
            let ctx = AutoRunCtx {
                intensity,
                awaiting_user: true,
                steps: 8,
                ..ctx_with_tools(&tools)
            };
            assert_eq!(
                state.decide(&ctx),
                AutoRunAction::Stop(AutoStopReason::AwaitingUser),
                "{intensity:?}"
            );
            assert_eq!(state.rounds, 4);
        }
        // 无动作轮(本来会 Nudge)与挂着目标(本来会 GoalPending)都让位。
        let idle = mk_tools(&["read"]);
        let mut state = AutoRunState {
            rounds: 1,
            ..Default::default()
        };
        assert_eq!(
            state.decide(&AutoRunCtx {
                awaiting_user: true,
                ..ctx_with_tools(&idle)
            }),
            AutoRunAction::Stop(AutoStopReason::AwaitingUser)
        );
        let mut state = AutoRunState::default();
        assert_eq!(
            state.decide(&AutoRunCtx {
                awaiting_user: true,
                goal_active: true,
                ..ctx_with_tools(&tools)
            }),
            AutoRunAction::Stop(AutoStopReason::AwaitingUser),
            "目标挂着也停(目标由调用方保留,不像 GoalMet 那样清除)"
        );
        // 零产出熔断也让位:同一签名连续多轮,只要在等用户就先停在 AwaitingUser。
        let mut state = AutoRunState::default();
        let busy = AutoRunCtx {
            steps: 4,
            progress_signature: "same",
            ..ctx_with_tools(&tools)
        };
        for _ in 0..ZERO_OUTPUT_ROUND_LIMIT - 1 {
            assert_eq!(state.decide(&busy), AutoRunAction::Continue);
        }
        assert_eq!(
            state.decide(&AutoRunCtx {
                awaiting_user: true,
                ..busy.clone()
            }),
            AutoRunAction::Stop(AutoStopReason::AwaitingUser)
        );
    }

    /// 资源/用户段仍排在前面:暂停、本轮后停、致命/限流失败、backlog 全阻塞不被「等用户」遮住。
    #[test]
    fn 等用户不遮住资源与用户段() {
        let tools = mk_tools(&["edit"]);
        let mut paused = AutoRunState {
            paused: true,
            ..Default::default()
        };
        assert_eq!(
            paused.decide(&AutoRunCtx {
                awaiting_user: true,
                ..ctx_with_tools(&tools)
            }),
            AutoRunAction::Stop(AutoStopReason::Paused)
        );
        let mut stop_after = AutoRunState {
            stop_after_round: true,
            ..Default::default()
        };
        assert_eq!(
            stop_after.decide(&AutoRunCtx {
                awaiting_user: true,
                ..ctx_with_tools(&tools)
            }),
            AutoRunAction::Stop(AutoStopReason::StopAfterRound)
        );
        for (failure, reason) in [
            (RoundFailure::Fatal, AutoStopReason::FatalError),
            (RoundFailure::RateLimited, AutoStopReason::RateLimited),
        ] {
            let mut state = AutoRunState::default();
            assert_eq!(
                state.decide(&AutoRunCtx {
                    awaiting_user: true,
                    round_failure: Some(failure),
                    ..ctx_with_tools(&tools)
                }),
                AutoRunAction::Stop(reason)
            );
        }
    }

    // ---- R-322:门禁强度与模型停机权 ----

    #[test]
    fn 局部完成不终止委托_自主队列不被当前请求完成截断() {
        use crate::handoff::HandoffScope;
        let tools = mk_tools(&["edit", "work"]);
        for intensity in [HarnessIntensity::Paired, HarnessIntensity::Autonomous] {
            for scope in [HandoffScope::WorkItem, HandoffScope::Batch] {
                let mut state = AutoRunState {
                    rounds: 3,
                    ..Default::default()
                };
                let ctx = AutoRunCtx {
                    intensity,
                    steps: 5,
                    completion: Some(scope),
                    ..ctx_with_tools(&tools)
                };
                assert_eq!(state.decide(&ctx), AutoRunAction::Continue);
                assert_eq!(state.rounds, 4);
                assert_eq!(
                    state.decide(&AutoRunCtx {
                        goal_active: true,
                        ..ctx
                    }),
                    AutoRunAction::GoalPending
                );
            }
        }
        for (intensity, expected) in [
            (
                HarnessIntensity::Paired,
                AutoRunAction::Stop(AutoStopReason::ModelDeclaredDone),
            ),
            (HarnessIntensity::Autonomous, AutoRunAction::Continue),
        ] {
            let mut state = AutoRunState::default();
            let ctx = AutoRunCtx {
                intensity,
                steps: 5,
                completion: Some(HandoffScope::Request),
                ..ctx_with_tools(&tools)
            };
            assert_eq!(state.decide(&ctx), expected);
            assert_eq!(state.rounds, 1);
        }
        let mut state = AutoRunState::default();
        let ctx = AutoRunCtx {
            goal_active: true,
            steps: 5,
            completion: Some(HandoffScope::Request),
            ..ctx_with_tools(&tools)
        };
        assert_eq!(
            state.decide(&ctx),
            AutoRunAction::GoalPending,
            "请求级完成不能清除独立目标"
        );
    }

    /// 反证:同样的输入,只把 completion 关掉,自主档仍走原来的 Nudge。
    /// 两条一起看才说明「变的只是模型声明这一条通路」,不是把刹车拆了。
    #[test]
    fn 未声明完成时_自主档仍走原有推进刹车() {
        let mut state = AutoRunState {
            rounds: 3,
            ..Default::default()
        };
        let tools = mk_tools(&["read", "grep"]);
        let ctx = AutoRunCtx {
            intensity: HarnessIntensity::Autonomous,
            completion: None,
            ..ctx_with_tools(&tools)
        };
        assert_eq!(state.decide(&ctx), AutoRunAction::Nudge);
        // 第二次才停(原有两段式刹车不变)。
        assert_eq!(
            state.decide(&ctx),
            AutoRunAction::Stop(AutoStopReason::NoAction)
        );
    }

    /// 结伴档:引擎不制造工作。一轮没动作直接停,不追加推进指令——
    /// 用户在场,多半是在问问题而不是在等模型找活干。
    #[test]
    fn 结伴档_无动作直接停_不追加推进指令() {
        let mut state = AutoRunState {
            rounds: 3,
            ..Default::default()
        };
        let tools = mk_tools(&["read"]);
        let ctx = AutoRunCtx {
            intensity: HarnessIntensity::Paired,
            ..ctx_with_tools(&tools)
        };
        assert_eq!(
            state.decide(&ctx),
            AutoRunAction::Stop(AutoStopReason::NoAction),
            "结伴档不得出现 Nudge"
        );
    }

    /// 资源类兜底与副作用边界不随强度松动:结伴档同样吃限流、致命与零产出熔断。
    /// 连续次数上限是旧配置兼容字段,不再属于资源停机条件。
    #[test]
    fn 结伴档_资源类兜底一条不少() {
        let tools = mk_tools(&["edit", "bash"]);
        let paired = |over: AutoRunCtx<'_>| {
            let mut state = AutoRunState::default();
            (state.decide(&over), state)
        };

        // 限流
        let (action, _) = paired(AutoRunCtx {
            intensity: HarnessIntensity::Paired,
            round_failure: Some(RoundFailure::RateLimited),
            ..ctx_with_tools(&tools)
        });
        assert_eq!(action, AutoRunAction::Stop(AutoStopReason::RateLimited));

        // 致命
        let (action, _) = paired(AutoRunCtx {
            intensity: HarnessIntensity::Paired,
            round_failure: Some(RoundFailure::Fatal),
            ..ctx_with_tools(&tools)
        });
        assert_eq!(action, AutoRunAction::Stop(AutoStopReason::FatalError));

        // 旧版连续次数上限不再阻断结伴档的正常续跑,但轮次仍计数。
        let mut state = AutoRunState {
            rounds: 2,
            ..Default::default()
        };
        let ctx = AutoRunCtx {
            intensity: HarnessIntensity::Paired,
            steps: 5,
            ..ctx_with_tools(&tools)
        };
        assert_eq!(state.decide(&ctx), AutoRunAction::Continue);
        assert_eq!(state.rounds, 3);
    }

    /// D-583 的 ZeroOutput 熔断是**资源判断**(比对真实签名),不随强度关闭。
    /// 别看它长得像任务判断就按强度门控——那是无人值守唯一的失控兜底。
    #[test]
    fn 结伴档_零产出熔断仍然生效() {
        let mut state = AutoRunState::default();
        let tools = mk_tools(&["edit", "bash"]);
        let ctx = AutoRunCtx {
            intensity: HarnessIntensity::Paired,
            steps: 5,
            progress_signature: "sig-unchanged",
            ..ctx_with_tools(&tools)
        };
        // 第一轮记录签名,之后连续同签名累计到上限熔断。
        let mut last = state.decide(&ctx);
        for _ in 0..ZERO_OUTPUT_ROUND_LIMIT + 1 {
            last = state.decide(&ctx);
            if matches!(last, AutoRunAction::Stop(AutoStopReason::ZeroOutput(_))) {
                break;
            }
        }
        assert!(
            matches!(last, AutoRunAction::Stop(AutoStopReason::ZeroOutput(_))),
            "结伴档也必须熔断,实际: {last:?}"
        );
    }

    /// 用户意图优先于模型声明:用户按了停/暂停,不需要征询模型意见。
    #[test]
    fn 用户意图仍然压过模型声明() {
        let tools = mk_tools(&["edit"]);
        let mut state = AutoRunState {
            paused: true,
            ..Default::default()
        };
        let ctx = AutoRunCtx {
            completion: Some(crate::handoff::HandoffScope::Request),
            ..ctx_with_tools(&tools)
        };
        assert_eq!(
            state.decide(&ctx),
            AutoRunAction::Stop(AutoStopReason::Paused),
            "暂停是用户意图,排在模型声明之前"
        );
    }

    #[test]
    fn 强度往返解析与策略表() {
        for intensity in [HarnessIntensity::Paired, HarnessIntensity::Autonomous] {
            assert_eq!(
                intensity.as_str().parse::<HarnessIntensity>(),
                Ok(intensity)
            );
        }
        assert!("nonsense".parse::<HarnessIntensity>().is_err());
        assert_eq!(HarnessIntensity::default(), HarnessIntensity::Autonomous);
        // 默认必须是自主档:强度未接线的调用方(CLI/测试桩)保持引入前行为。
        let auto = HarnessIntensity::Autonomous.policy();
        assert!(auto.engine_nudge);
        let paired = HarnessIntensity::Paired.policy();
        assert!(!paired.engine_nudge);
    }

    /// 结伴档轻 loop 的完整形状:有实质动作就续跑,一轮没动作就停(不 Nudge),
    /// 模型声明完成也停。这三条合起来才是「简单 loop」。
    #[test]
    fn 结伴档轻loop_有动作续跑_无动作即停() {
        let mut state = AutoRunState::default();
        let working = mk_tools(&["edit", "bash"]);
        let idle = mk_tools(&["read"]);
        fn paired_ctx(tools: &[String]) -> AutoRunCtx<'_> {
            AutoRunCtx {
                intensity: HarnessIntensity::Paired,
                steps: 4,
                ..ctx_with_tools(tools)
            }
        }
        assert_eq!(state.decide(&paired_ctx(&working)), AutoRunAction::Continue);
        assert_eq!(state.decide(&paired_ctx(&working)), AutoRunAction::Continue);
        assert_eq!(
            state.decide(&paired_ctx(&idle)),
            AutoRunAction::Stop(AutoStopReason::NoAction),
            "结伴档一轮没动作就交还,不追加推进指令"
        );
    }

    // ---- R-322 B3:目标条件 loop(结伴档的「简单 loop」) ----

    /// 目标挂着时一轮没动作**不停**——复述用户给的条件再推一轮。
    /// 这是与 Nudge 的分水岭:Nudge 的内容是引擎发明的,这条是用户写的。
    #[test]
    fn 目标挂着_无动作不停而是复述目标再推一轮() {
        let mut state = AutoRunState::default();
        let idle = mk_tools(&["read"]);
        let ctx = AutoRunCtx {
            intensity: HarnessIntensity::Paired,
            goal_active: true,
            steps: 1,
            ..ctx_with_tools(&idle)
        };
        assert_eq!(state.decide(&ctx), AutoRunAction::GoalPending);
        assert_eq!(state.rounds, 1, "GoalPending 占一轮");
    }

    /// 目标达成 = 模型自己声明的。引擎不判断达没达成,只负责在那之前不散场。
    #[test]
    fn 目标达成由模型声明_停止原因区分于普通交还() {
        let tools = mk_tools(&["edit"]);
        let mut with_goal = AutoRunState::default();
        assert_eq!(
            with_goal.decide(&AutoRunCtx {
                goal_active: true,
                completion: Some(crate::handoff::HandoffScope::Goal),
                ..ctx_with_tools(&tools)
            }),
            AutoRunAction::Stop(AutoStopReason::GoalMet)
        );
        let mut no_goal = AutoRunState::default();
        assert_eq!(
            no_goal.decide(&AutoRunCtx {
                intensity: HarnessIntensity::Paired,
                completion: Some(crate::handoff::HandoffScope::Request),
                ..ctx_with_tools(&tools)
            }),
            AutoRunAction::Stop(AutoStopReason::ModelDeclaredDone),
            "没挂目标时仍是普通交还,UI 文案不同"
        );
    }

    /// 兜底:条件含糊或不可达时,连续 N 轮推不动就停,让用户改条件。
    #[test]
    fn 目标连续推不动_达上限即停并带轮数() {
        let mut state = AutoRunState::default();
        let idle = mk_tools(&["read"]);
        let ctx = AutoRunCtx {
            intensity: HarnessIntensity::Paired,
            goal_active: true,
            steps: 1,
            ..ctx_with_tools(&idle)
        };
        for _ in 0..GOAL_IDLE_ROUND_LIMIT - 1 {
            assert_eq!(state.decide(&ctx), AutoRunAction::GoalPending);
        }
        assert_eq!(
            state.decide(&ctx),
            AutoRunAction::Stop(AutoStopReason::GoalUnreachable(GOAL_IDLE_ROUND_LIMIT))
        );
    }

    /// 有实质动作的轮次清零空转计数——目标推进断断续续也不该被误判不可达。
    #[test]
    fn 目标空转计数被有动作的轮次清零() {
        let mut state = AutoRunState::default();
        let idle = mk_tools(&["read"]);
        let working = mk_tools(&["edit", "bash"]);
        fn goal_ctx(tools: &[String]) -> AutoRunCtx<'_> {
            AutoRunCtx {
                intensity: HarnessIntensity::Paired,
                goal_active: true,
                steps: 3,
                ..ctx_with_tools(tools)
            }
        }
        assert_eq!(state.decide(&goal_ctx(&idle)), AutoRunAction::GoalPending);
        assert_eq!(state.decide(&goal_ctx(&idle)), AutoRunAction::GoalPending);
        assert_eq!(
            state.decide(&goal_ctx(&working)),
            AutoRunAction::GoalPending
        );
        // 计数已清零,再来两轮空转仍不该停。
        assert_eq!(state.decide(&goal_ctx(&idle)), AutoRunAction::GoalPending);
        assert_eq!(state.decide(&goal_ctx(&idle)), AutoRunAction::GoalPending);
    }

    /// 目标 loop 关掉了 NoAction 刹车,零产出熔断必须仍然挡得住
    /// 「每轮都调工具、磁盘一个字节没变」——否则无限空转。
    #[test]
    fn 目标挂着_零产出熔断仍是兜底() {
        let mut state = AutoRunState::default();
        let working = mk_tools(&["edit", "bash"]);
        let ctx = AutoRunCtx {
            intensity: HarnessIntensity::Paired,
            goal_active: true,
            steps: 5,
            progress_signature: "unchanged",
            ..ctx_with_tools(&working)
        };
        let mut last = state.decide(&ctx);
        for _ in 0..ZERO_OUTPUT_ROUND_LIMIT + 2 {
            last = state.decide(&ctx);
            if matches!(last, AutoRunAction::Stop(AutoStopReason::ZeroOutput(_))) {
                break;
            }
        }
        assert!(
            matches!(last, AutoRunAction::Stop(AutoStopReason::ZeroOutput(_))),
            "目标挂着也必须熔断,实际: {last:?}"
        );
    }

    /// 用户意图与资源兜底压过目标:按了停、限流、失败等立即停。
    #[test]
    fn 目标不能压过用户意图与资源兜底() {
        let tools = mk_tools(&["edit"]);
        let mut paused = AutoRunState {
            paused: true,
            ..Default::default()
        };
        assert_eq!(
            paused.decide(&AutoRunCtx {
                goal_active: true,
                ..ctx_with_tools(&tools)
            }),
            AutoRunAction::Stop(AutoStopReason::Paused)
        );

        let mut limited = AutoRunState::default();
        assert_eq!(
            limited.decide(&AutoRunCtx {
                goal_active: true,
                round_failure: Some(RoundFailure::RateLimited),
                ..ctx_with_tools(&tools)
            }),
            AutoRunAction::Stop(AutoStopReason::RateLimited)
        );

        let mut unlimited = AutoRunState {
            rounds: 2,

            ..Default::default()
        };
        assert_eq!(
            unlimited.decide(&AutoRunCtx {
                goal_active: true,
                steps: 5,
                ..ctx_with_tools(&tools)
            }),
            AutoRunAction::GoalPending,
            "目标推进不应被旧连续次数上限截断"
        );
        assert_eq!(unlimited.rounds, 3);
    }
}
