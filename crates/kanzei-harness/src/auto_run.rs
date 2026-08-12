//! 自主推进(auto-run,即「鞭挞」)状态机策略:轮末该继续、该追加推进指令、
//! 还是该停——全部由引擎判定,不依赖提示词恳求(D-120/D-128/D-163 教训:
//! 规则写在用户可编辑文案里会与引擎行为脱节)。
//!
//! 这些判定原本全在桌面端前端 JS(08-compose.js / 07-events.js):空转工具画像、
//! 连数上限、全部阻塞/清空停止、无动作 NUDGE、暂停/本轮后停/停止原因。
//! 本模块把它们下沉为**纯逻辑状态机**(无 IO):kanzei-core runner 轮末消费,
//! 桌面端与 CLI 共用同一套判定(D-229 类「能力只在桌面端」的架构债消除),
//! UI 只保留控件与状态回显。设计见 docs/design/continue_prompt_dissection.md §4。

/// 不构成实质进展的工具:一轮里只有这些(纯查询/探测/写记忆日记)时仍算空转——
/// 模型不能再靠 memory_note 或无关读取绕过刹车(D-044 教训的硬化,原 R-076 画像)。
/// bash/git/edit/write/tracker 等可能改变状态的工具不在列:名称粒度分不出
/// git status 与 git commit,误判成空转的代价(真干活被打断)比漏判高。
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
    "task",
];

/// 本轮工具画像是否包含实质进展工具。
pub fn has_progress_tools(tools: &[String]) -> bool {
    tools
        .iter()
        .any(|name| !NON_PROGRESS_TOOLS.contains(&name.as_str()))
}

/// 轮末 backlog 状态(由调用方查询 docs_snapshot 后传入)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BacklogStatus {
    /// 存在可推进条目:继续正常取活。
    Workable,
    /// 活动条目全部外部阻塞:继续跑只会空转烧钱(R-128)。
    AllBlocked,
    /// 没有活动条目(已清空):同样空转。
    Empty,
    /// 查询失败:按可推进处理,绝不因探测故障误停。
    Unknown,
}

impl BacklogStatus {
    fn should_stop(&self) -> bool {
        matches!(self, BacklogStatus::AllBlocked | BacklogStatus::Empty)
    }
}

/// 停止原因(枚举;i18n 文案由 UI 层按枚举映射,引擎不产生用户可读文案)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoStopReason {
    /// 活动条目全部阻塞(R-128:阻塞解除后可恢复)。
    AllBlocked,
    /// 活动条目已清空。
    BacklogEmpty,
    /// 用户暂停。
    Paused,
    /// 用户勾选「本轮后停」(一次性意图,不持久化)。
    StopAfterRound,
    /// 已达连数上限(带上限值供展示)。
    MaxRounds(u32),
    /// 连续两轮无实质动作。
    NoAction,
    /// R-199:当前模式不允许自主推进(如 research/结对模式),引擎判定停止——
    /// 档位作为输入进 AutoRunCtx,前端不再持有引擎不知道的否决权。
    ProfileMismatch,
}

/// 轮末判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoRunAction {
    /// 正常续跑(计数已 +1)。
    Continue,
    /// 无动作第一次:追加一条具体推进指令(计数已 +1,占一轮)。
    Nudge,
    /// 停止(计数已重置;携带原因供 UI 展示)。
    Stop(AutoStopReason),
    /// 用户拒绝/手动停止本轮:不续跑、不重置计数(等手动输入重新武装)。
    NoContinue,
}

/// 取活顺序(与 `work_priority_guidance` 同源)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkPriority {
    RequirementFirst,
    DefectFirst,
}

impl WorkPriority {
    pub fn first_queue(&self) -> &'static str {
        match self {
            WorkPriority::RequirementFirst => "requirements.md",
            WorkPriority::DefectFirst => "defects.md",
        }
    }
    pub fn second_queue(&self) -> &'static str {
        match self {
            WorkPriority::RequirementFirst => "defects.md",
            WorkPriority::DefectFirst => "requirements.md",
        }
    }
}

/// 无动作时追加的具体推进指令(原 NUDGE_PROMPT,前端模板按取活顺序替换)。
/// 引擎生成后作为用户消息注入,规则不再驻留在可编辑文案里。
pub fn nudge_prompt(work_priority: WorkPriority) -> String {
    let first = work_priority.first_queue();
    let second = work_priority.second_queue();
    format!(
        "上一轮没有产生任何实质动作。不要再做可行性判断,直接执行:\n\
         从 {first} 最上面一条开始,说出它的下一个最小可执行步骤(具体到文件和改动),然后立刻做掉。\n\
         那一条一时推不动就跳到下一条,{second} 同理——总有一条是能动手的。\n\
         如果每一条都标着阻塞:先复核阻塞是否还成立。多数是你自己历轮写下的,解除条件早已满足,\n\
         清空这些条目的「阻塞」字段再取活;真正卡住的只有等用户拍板的那几条,把它们点名列给用户。\n\
         不要为了凑动作去做与当前条目无关的事,也不要只更新追踪文档就算一轮。"
    )
}

/// 自主推进状态:跨轮计数与用户一次性意图。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoRunState {
    /// 本轮内已自动续跑的轮数(手动发送归零)。
    pub rounds: u32,
    /// 连数上限(防失控;clamp 到 1..=100)。
    pub max_rounds: u32,
    /// 用户暂停。
    pub paused: bool,
    /// 「本轮后停」一次性意图(D-111:绝不持久化)。
    pub stop_after_round: bool,
    /// 连续无实质动作的轮数:第一次追加推进指令,第二次才停。
    no_action_rounds: u32,
}

impl AutoRunState {
    pub fn new(max_rounds: u32) -> Self {
        AutoRunState {
            rounds: 0,
            max_rounds: max_rounds.clamp(1, 100),
            paused: false,
            stop_after_round: false,
            no_action_rounds: 0,
        }
    }

    /// 手动发送/用户操作时归零(原前端 autoRounds=0; noActionRounds=0)。
    pub fn reset(&mut self) {
        self.rounds = 0;
        self.no_action_rounds = 0;
    }

    /// 轮末判定。判定顺序与前端 07-events.js:288-352 完全一致:
    /// ①backlog 全阻塞/清空最优先(前端 stopAutoWhenBacklogEmpty 最先跑);
    /// ②用户拒绝(halted)不续跑不重置;③暂停;④本轮后停;⑤连数上限;
    /// ⑥无动作(第一次 NUDGE/第二次停);⑦正常续跑。
    /// R-199:档位检查在 backlog 之后——模式不匹配时引擎 Stop(ProfileMismatch)
    /// 且计数不 +1(重置为 0),前端不再有第二次否决(计数与实际轮次不再漂移)。
    pub fn decide(&mut self, ctx: &AutoRunCtx) -> AutoRunAction {
        if ctx.backlog.should_stop() {
            let reason = match ctx.backlog {
                BacklogStatus::AllBlocked => AutoStopReason::AllBlocked,
                BacklogStatus::Empty => AutoStopReason::BacklogEmpty,
                _ => unreachable!(),
            };
            return self.stop_with(reason);
        }
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
        if self.rounds >= self.max_rounds {
            return self.stop_with(AutoStopReason::MaxRounds(self.max_rounds));
        }
        let no_action = ctx.steps <= 1 || !has_progress_tools(ctx.tools);
        if no_action && self.rounds > 0 {
            if self.no_action_rounds == 0 {
                self.no_action_rounds = 1;
                self.rounds += 1;
                return AutoRunAction::Nudge;
            }
            return self.stop_with(AutoStopReason::NoAction);
        }
        self.no_action_rounds = 0;
        self.rounds += 1;
        AutoRunAction::Continue
    }

    fn stop_with(&mut self, reason: AutoStopReason) -> AutoRunAction {
        self.rounds = 0;
        self.no_action_rounds = 0;
        AutoRunAction::Stop(reason)
    }
}

/// 默认连数上限 10(与桌面端 DEFAULT_AUTO_CONTINUE_MAX 一致)。
impl Default for AutoRunState {
    fn default() -> Self {
        AutoRunState::new(10)
    }
}

/// 轮末判定输入。
#[derive(Clone, Debug)]
pub struct AutoRunCtx<'a> {
    pub backlog: BacklogStatus,
    /// 用户拒绝/手动停止(前端 kz:done 的 halted 字段)。
    pub halted: bool,
    /// 本轮工具调用轮数。
    pub steps: u32,
    /// 本轮实际调用的工具名列表(供空转画像判定)。
    pub tools: &'a [String],
    /// R-199:当前模式是否允许自主推进(引擎知道的档位条件,前端不再持有)。
    pub auto_allowed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_tools(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn ctx_with_tools(tools: &[String]) -> AutoRunCtx<'_> {
        AutoRunCtx {
            backlog: BacklogStatus::Workable,
            halted: false,
            steps: 1,
            tools,
            auto_allowed: true,
        }
    }

    /// R-199:模式不允许自主推进时引擎 Stop(ProfileMismatch)且计数不 +1(重置)。
    #[test]
    fn 模式不匹配时引擎停止且计数不漂移() {
        let mut state = AutoRunState::new(10);
        state.rounds = 3; // 假设已跑了 3 轮
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
        assert_eq!(
            state.rounds, 0,
            "否决发生时引擎计数必须重置为 0,不再与前端轮次漂移"
        );
    }

    #[test]
    fn 有实质动作的轮次_正常续跑并计数() {
        let mut state = AutoRunState::new(10);
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
        let mut state = AutoRunState::new(10);
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
        let mut state = AutoRunState::new(10);
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
        assert_eq!(state.rounds, 0, "停止后计数归零");
    }

    #[test]
    fn 达连数上限_停止并带上限值() {
        let mut state = AutoRunState::new(2);
        let ok = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["edit"]),
            ..ctx_with_tools(&[])
        };
        assert_eq!(state.decide(&ok), AutoRunAction::Continue); // rounds=1
        assert_eq!(state.decide(&ok), AutoRunAction::Continue); // rounds=2
        assert_eq!(
            state.decide(&ok),
            AutoRunAction::Stop(AutoStopReason::MaxRounds(2))
        );
        assert_eq!(state.rounds, 0);
    }

    #[test]
    fn 暂停时停止_恢复后继续() {
        let mut state = AutoRunState::new(10);
        state.paused = true;
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
        let mut state = AutoRunState::new(10);
        state.stop_after_round = true;
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
    fn 全部阻塞或清空_优先于其它判定停止() {
        let mut state = AutoRunState::new(10);
        // 即使有动作、未暂停、未达上限:全阻塞照样停(前端 stopAutoWhenBacklogEmpty 最先跑)。
        let ok = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["edit"]),
            ..ctx_with_tools(&[])
        };
        let mut blocked_ctx = ok.clone();
        blocked_ctx.backlog = BacklogStatus::AllBlocked;
        assert_eq!(
            state.decide(&blocked_ctx),
            AutoRunAction::Stop(AutoStopReason::AllBlocked)
        );
        assert_eq!(state.rounds, 0);

        let mut empty_ctx = ok.clone();
        empty_ctx.backlog = BacklogStatus::Empty;
        assert_eq!(
            state.decide(&empty_ctx),
            AutoRunAction::Stop(AutoStopReason::BacklogEmpty)
        );
    }

    #[test]
    fn 阻塞解除后_恢复续跑() {
        // R-128 验收后半段:全阻塞停止后,阻塞字段清空、backlog 回到 Workable,
        // 下一轮判定应恢复正常续跑(停止仅由当时的 backlog 状态触发,不持久锁死)。
        let mut state = AutoRunState::new(10);
        let ok = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["edit"]),
            ..ctx_with_tools(&[])
        };
        let mut blocked_ctx = ok.clone();
        blocked_ctx.backlog = BacklogStatus::AllBlocked;
        assert_eq!(
            state.decide(&blocked_ctx),
            AutoRunAction::Stop(AutoStopReason::AllBlocked)
        );
        assert_eq!(state.rounds, 0);
        // 阻塞解除:同一状态机,backlog 回到 Workable → 正常续跑并计数。
        assert_eq!(state.decide(&ok), AutoRunAction::Continue);
        assert_eq!(state.rounds, 1);
    }

    #[test]
    fn backlog查询失败_按可推进处理不误停() {
        let mut state = AutoRunState::new(10);
        let t = mk_tools(&["edit"]);
        let ctx = AutoRunCtx {
            backlog: BacklogStatus::Unknown,
            halted: false,
            steps: 2,
            tools: &t,
            auto_allowed: true,
        };
        assert_eq!(state.decide(&ctx), AutoRunAction::Continue);
    }

    #[test]
    fn 用户拒绝_halted_不续跑也不重置计数() {
        let mut state = AutoRunState::new(10);
        state.rounds = 5;
        let ctx = AutoRunCtx {
            halted: true,
            ..ctx_with_tools(&[])
        };
        assert_eq!(state.decide(&ctx), AutoRunAction::NoContinue);
        assert_eq!(state.rounds, 5, "halted 不重置:等手动输入重新武装");
    }

    #[test]
    fn 手动输入_reset_归零() {
        let mut state = AutoRunState::new(10);
        state.rounds = 7;
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

    #[test]
    fn max_rounds_clamp_到1_100() {
        assert_eq!(AutoRunState::new(0).max_rounds, 1);
        assert_eq!(AutoRunState::new(500).max_rounds, 100);
        assert_eq!(AutoRunState::new(10).max_rounds, 10);
    }

    #[test]
    fn nudge_prompt_按取活顺序生成() {
        let p = nudge_prompt(WorkPriority::RequirementFirst);
        assert!(p.contains("requirements.md 最上面一条"));
        assert!(p.contains("defects.md 同理"));
        let p2 = nudge_prompt(WorkPriority::DefectFirst);
        assert!(p2.contains("defects.md 最上面一条"));
        assert!(p2.contains("requirements.md 同理"));
    }
}
