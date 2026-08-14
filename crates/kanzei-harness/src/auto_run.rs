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
///
/// `task` 在列,但它的语义与其余成员不同,别照字面理解:**派子代理本身不算进展,
/// 子代理干的活算**。调用方在轮末必须把子代理内部用过的工具名并进画像再传进来
/// (桌面端见 run.rs 的 subagent_round_tool / subagent_tools),否则「整轮把活派出去」
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
    /// R-144:已关闭 N 条,插入一轮只读验收核查(计数已 +1,占一轮;核查
    /// 复用 SubagentBase read/glob/grep,核对验收证据与真实调用方,发现问题
    /// 生成候选缺陷或退回依据,不进入主 conversation/queue)。
    VerifyRound,
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

/// R-144:验收核查轮指令。自主推进每关闭 N 条后,引擎生成这条核查指令作为下一轮
/// 输入——主代理用只读 task 子代理(read/glob/grep,SubagentBase)核对最近关闭
/// 条目的验收证据与真实调用方,发现「宣称完成但无证据/无调用方」即生成候选缺陷
/// (defect add)或退回依据。核查不进入主 conversation/queue:它是一条独立输入,
/// 结果以 notice/候选缺陷形式可见,不污染主对话历史。与 nudge_prompt 同哲学:
/// 模板在引擎,前端不持文案。
pub fn verify_prompt() -> String {
    "验收核查轮:自主推进已连续关闭 N 条,现在插入一轮只读验收核查(不进入主对话历史)。\n\
     用 task 子代理(只读 read/glob/grep)核对最近关闭的若干条目:\n\
     第一,逐条读其关闭证据(进展/验收字段),核对引用的测试 ID 是否真实存在于 tests 记录,\n\
     file:line 是否真实存在;\n\
     第二,核对「声称完成的能力」是否有真实调用方或消费者——死代码、只展示未接入的界面壳不算完成;\n\
     第三,发现「宣称完成但证据不足/无调用方」的条目:用 defect add 生成候选缺陷(标注严重度与优先级,\n\
     来源写 self-found 验收核查),或给出退回依据(进展里写明缺口)。\n\
     核查完成后继续正常推进。"
        .to_string()
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
    /// R-144:自上次核查以来累计关闭的条目数。达阈值(verify_every_n)即触发
    /// 一轮只读验收核查,触发后归零。
    pub closed_since_verify: u32,
}

impl AutoRunState {
    pub fn new(max_rounds: u32) -> Self {
        AutoRunState {
            rounds: 0,
            max_rounds: max_rounds.clamp(1, 100),
            paused: false,
            stop_after_round: false,
            no_action_rounds: 0,
            closed_since_verify: 0,
        }
    }

    /// 手动发送/用户操作时归零(原前端 autoRounds=0; noActionRounds=0)。
    pub fn reset(&mut self) {
        self.rounds = 0;
        self.no_action_rounds = 0;
        self.closed_since_verify = 0;
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
        // R-144:先累加本轮关闭数,达阈值(>0 且 >=N)则插入一轮只读验收核查
        // (计数 +1 占一轮;核查由调用方执行,归零后再续跑)。
        self.closed_since_verify += ctx.closed_this_round;
        if ctx.verify_every_n > 0 && self.closed_since_verify >= ctx.verify_every_n {
            self.closed_since_verify = 0;
            self.rounds += 1;
            return AutoRunAction::VerifyRound;
        }
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
    /// R-144:本轮实际关闭的条目数(req/defect close 成功计数)。
    pub closed_this_round: u32,
    /// R-144:验收核查阈值——每关闭 N 条插入一轮只读核查;0 = 关闭该机制。
    pub verify_every_n: u32,
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
            closed_this_round: 0,
            verify_every_n: 0,
        }
    }

    /// D-361:委派轮不是空转轮。子代理内部用过的工具由调用方上卷进画像后,
    /// 「整轮只调 task」的委派必须能正常续跑;子代理也没动作时刹车照旧。
    #[test]
    fn 委派轮上卷子代理画像后不判空转() {
        let mut state = AutoRunState::new(10);
        state.rounds = 1; // no_action 只在 rounds > 0 时才触发,先跑过一轮
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
        let mut state = AutoRunState::new(10);
        state.rounds = 1;
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

    /// R-144 B1:每关闭 N 条触发一轮只读核查(VerifyRound),触发后计数归零;
    /// verify_every_n=0 关闭机制;未达阈值正常续跑。
    #[test]
    fn 每关闭n条触发核查轮_阈值0关闭机制() {
        // 阈值 3:两轮各关 1 + 2 条 → 第 2 轮末触发 VerifyRound。
        let mut state = AutoRunState::new(10);
        let ok = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["req", "edit"]),
            closed_this_round: 1,
            verify_every_n: 3,
            ..ctx_with_tools(&[])
        };
        assert_eq!(state.decide(&ok), AutoRunAction::Continue);
        assert_eq!(state.closed_since_verify, 1, "未达阈值只累计");
        let ok2 = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["req", "edit"]),
            closed_this_round: 2,
            verify_every_n: 3,
            ..ctx_with_tools(&[])
        };
        assert_eq!(
            state.decide(&ok2),
            AutoRunAction::VerifyRound,
            "累计达 3 必须触发核查轮"
        );
        assert_eq!(state.closed_since_verify, 0, "触发后计数归零");
        assert_eq!(state.rounds, 2, "核查轮占一轮计数");

        // 阈值 0 = 关闭机制:关闭再多也直接续跑。
        let mut off = AutoRunState::new(10);
        let close_many = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["req", "edit"]),
            closed_this_round: 99,
            verify_every_n: 0,
            ..ctx_with_tools(&[])
        };
        assert_eq!(off.decide(&close_many), AutoRunAction::Continue);
        assert_eq!(off.closed_since_verify, 99, "机制关闭时累计无意义但不阻断");

        // 单轮关闭超过阈值:当场触发。
        let mut burst = AutoRunState::new(10);
        let burst_ctx = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["req", "edit"]),
            closed_this_round: 5,
            verify_every_n: 3,
            ..ctx_with_tools(&[])
        };
        assert_eq!(burst.decide(&burst_ctx), AutoRunAction::VerifyRound);
        assert_eq!(burst.closed_since_verify, 0);
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
            closed_this_round: 0,
            verify_every_n: 0,
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
