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
    /// D-403:连续 N 轮瞬态失败(退避重试仍失败),停止并通知——
    /// provider 可能长时间不可用,继续空转只会烧钱。
    RepeatedFailure(u32),
    /// D-403:本轮以致命错误失败(认证/配置类),重试只会原样复现,立即停。
    FatalError,
    /// provider 明确返回限流/过载(通常是 HTTP 429):本轮自动推进立即停，
    /// 等待用户确认配额恢复后手动恢复，避免继续消耗订阅额度。
    RateLimited,
    /// D-583:连续 ZERO_OUTPUT_ROUND_LIMIT 轮真实进展签名未变(带触发轮数供展示)——
    /// 与 NoAction 互补:那个防「整轮不调用任何工具」,这个防「调用了工具但什么也没变」。
    ZeroOutput(u32),
    /// R-322(#7):模型显式声明任务完成并交还控制权。**不是引擎的判断**,
    /// 引擎只是照做——与其余停止原因的性质不同,UI 文案不应写成「引擎停止了它」。
    ModelDeclaredDone,
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
    /// D-403:本轮瞬态失败,退避后重试(attempt = 当前连续失败次数,计数已 +1;
    /// 退避时长由调用方按 attempt 换算,引擎不持时钟)。
    RetryAfterFailure { attempt: u32 },
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
    /// D-403:连续瞬态失败的轮数。成功轮归零;达 MAX_FAILED_ROUNDS 即停。
    failed_rounds: u32,
    /// D-583:连续几轮「真实进展签名」未变。签名由调用方按 (HEAD、代码 worktree
    /// hash、tracker 文档内容) 拼出,只在这里做纯比较——has_progress_tools 只看
    /// 工具名,一轮调用 bash 反复 cat 同一份证据清单也算「有进展工具」,穿不透那道
    /// 检测;这里比对的是真实状态有没有变,堵的是这类"看起来在干活"的空转。
    zero_output_rounds: u32,
    /// D-583:上一轮记录的真实进展签名,None = 尚未记录(刚 reset 或全新状态)。
    last_progress_signature: Option<String>,
}

/// D-403:连续瞬态失败多少轮后停止(过夜场景:单发 503 退避重试可自愈,
/// provider 长时间不可用则不再空转烧钱,停止并通知)。
pub const MAX_FAILED_ROUNDS: u32 = 3;

/// D-583:连续几轮真实进展签名不变即熔断停鞭(现场实测 R-306 空转 10 轮无人发现;
/// 验收建议 2~3,取上限留够「连续两轮都在做同一件事的合法收尾」的余量)。
pub const ZERO_OUTPUT_ROUND_LIMIT: u32 = 3;

impl AutoRunState {
    pub fn new(max_rounds: u32) -> Self {
        AutoRunState {
            rounds: 0,
            max_rounds: max_rounds.clamp(1, 100),
            paused: false,
            stop_after_round: false,
            no_action_rounds: 0,
            closed_since_verify: 0,
            failed_rounds: 0,
            zero_output_rounds: 0,
            last_progress_signature: None,
        }
    }

    /// 手动发送/用户操作时归零(原前端 autoRounds=0; noActionRounds=0)。
    pub fn reset(&mut self) {
        self.rounds = 0;
        self.no_action_rounds = 0;
        self.closed_since_verify = 0;
        self.failed_rounds = 0;
        self.zero_output_rounds = 0;
        self.last_progress_signature = None;
    }

    /// 轮末判定。判定顺序与前端 07-events.js:288-352 完全一致:
    /// ①backlog 全阻塞/清空最优先(前端 stopAutoWhenBacklogEmpty 最先跑);
    /// ②用户拒绝(halted)不续跑不重置;③暂停;④本轮后停;⑤连数上限;
    /// ⑥无动作(第一次 NUDGE/第二次停);⑦正常续跑。
    /// R-199:档位检查在 backlog 之后——模式不匹配时引擎 Stop(ProfileMismatch)
    /// 且计数不 +1(重置为 0),前端不再有第二次否决(计数与实际轮次不再漂移)。
    ///
    /// R-322 起完整顺序(**分界线是「谁有信息做这个判断」,不是重要程度**):
    ///
    /// ```text
    /// 资源/用户段(任何强度都执行,模型无权否决):
    ///   backlog → auto_allowed → halted → paused → stop_after_round
    ///   → max_rounds → round_failure
    /// 模型段(模型的停机权,引擎无权否决):
    ///   → model_declared_done
    /// 任务判断段(仅 Autonomous 档借给引擎):
    ///   → no_action/Nudge → zero_output → verify_round → Continue
    /// ```
    ///
    /// `zero_output`(D-583)看着像任务判断,其实不是:它比对 (HEAD、worktree hash、
    /// tracker 内容) 的真实签名,不解释语义,只回答「磁盘上还在不在变」。
    /// 无人值守时这是唯一的失控兜底,**两档都保留**,别按强度关掉它。
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
        // D-403:失败轮不算「无动作」——模型没机会动作,不该吃 Nudge/NoAction 刹车。
        // 瞬态失败退避重试,连续 MAX_FAILED_ROUNDS 轮才停;致命/限流失败立即停不空转。
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
                        self.rounds += 1;
                        AutoRunAction::RetryAfterFailure {
                            attempt: self.failed_rounds,
                        }
                    }
                }
            };
        }
        self.failed_rounds = 0;
        // R-322(#7):模型的停机权。位置很讲究——在用户意图(halted/paused/
        // stop_after_round)与资源兜底(max_rounds/round_failure)**之后**,在所有
        // 任务判断(Nudge/NoAction/ZeroOutput/VerifyRound)**之前**。
        //
        // 之后:用户按了停、或者 provider 已经限流,那些不需要征询模型意见。
        // 之前:一旦模型说完成,引擎就不再对「是不是真的完成了」发表意见——
        // 这正是原来 Nudge 干的事,也正是双控制器干扰回路的入口。
        if ctx.model_declared_done {
            return self.stop_with(AutoStopReason::ModelDeclaredDone);
        }
        let policy = ctx.intensity.policy();
        let no_action = ctx.steps <= 1 || !has_progress_tools(ctx.tools);
        if no_action && self.rounds > 0 {
            // R-322:Nudge 是任务判断,只在无人监督档借给引擎。结伴档下模型
            // 一轮没动作就是没动作(多半是在回答用户的问题),不该被推着找活干。
            if !policy.engine_nudge {
                return self.stop_with(AutoStopReason::NoAction);
            }
            if self.no_action_rounds == 0 {
                self.no_action_rounds = 1;
                self.rounds += 1;
                return AutoRunAction::Nudge;
            }
            return self.stop_with(AutoStopReason::NoAction);
        }
        self.no_action_rounds = 0;
        // D-583:工具画像有「进展工具」但真实状态连续 N 轮未变——纯复诵证据清单、
        // 反复读同一批文件也会调用 bash/read,穿得过上面基于工具名的检测,只有比对
        // 真实签名才拦得住。签名相同才计数;换了就清零重新起算,不跨越已中断的沉默期。
        // 空字符串是调用方未接线时的哨兵值(测试桩/尚未接入的调用方),视为不追踪——
        // 生产侧签名由真实哈希拼出,不会自然产出空串,不影响真实场景。
        if !ctx.progress_signature.is_empty() {
            if self.rounds > 0
                && self.last_progress_signature.as_deref() == Some(ctx.progress_signature)
            {
                self.zero_output_rounds += 1;
                if self.zero_output_rounds >= ZERO_OUTPUT_ROUND_LIMIT {
                    return self.stop_with(AutoStopReason::ZeroOutput(self.zero_output_rounds));
                }
            } else {
                self.zero_output_rounds = 0;
                self.last_progress_signature = Some(ctx.progress_signature.to_string());
            }
        }
        // R-144:先累加本轮关闭数,达阈值(>0 且 >=N)则插入一轮只读验收核查
        // (计数 +1 占一轮;核查由调用方执行,归零后再续跑)。
        // R-322:核查轮同属任务判断,结伴档由用户当场验收,引擎不插队。
        // 计数照常累加——中途切回自主档时节律不从零重来。
        self.closed_since_verify += ctx.closed_this_round;
        if policy.verify_rounds
            && ctx.verify_every_n > 0
            && self.closed_since_verify >= ctx.verify_every_n
        {
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
        self.failed_rounds = 0;
        self.zero_output_rounds = 0;
        self.last_progress_signature = None;
        AutoRunAction::Stop(reason)
    }
}

/// 默认连数上限 10(与桌面端 DEFAULT_AUTO_CONTINUE_MAX 一致)。
impl Default for AutoRunState {
    fn default() -> Self {
        AutoRunState::new(10)
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
    /// ——phase_pipeline_enabled 已经是第二个独立开关,再加就是配置面爆炸。
    pub fn policy(self) -> IntensityPolicy {
        match self {
            HarnessIntensity::Paired => IntensityPolicy {
                engine_nudge: false,
                redundancy_hints: false,
                verify_rounds: false,
            },
            HarnessIntensity::Autonomous => IntensityPolicy {
                engine_nudge: true,
                redundancy_hints: true,
                verify_rounds: true,
            },
        }
    }
}

/// 强度展开后的逐机制开关。只覆盖**任务判断**类机制;资源类兜底不在此列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntensityPolicy {
    /// 引擎能否在模型本轮无实质动作时追加推进指令(Nudge)。
    /// false = 模型说没什么可做就是没什么可做,引擎不制造工作。
    pub engine_nudge: bool,
    /// 冗余机械提醒(工具结果里就地追加 `[冗余提醒]`)。
    /// 结伴档关闭:用户在场,重复的 git status 他自己看得见。
    pub redundancy_hints: bool,
    /// 每关闭 N 条插入的只读验收核查轮(R-144)。
    /// 结伴档关闭:验收由用户当场做。
    pub verify_rounds: bool,
}

/// 轮末判定输入。
#[derive(Clone, Debug)]
pub struct AutoRunCtx<'a> {
    pub backlog: BacklogStatus,
    /// 用户拒绝/手动停止(前端 kz:done 的 halted 字段)。
    pub halted: bool,
    /// R-322:本轮门禁强度。决定引擎是否行使 Nudge / 验收核查等**任务判断**权。
    pub intensity: HarnessIntensity,
    /// R-322(#7 双控制器):模型本轮显式声明任务已完成、交还控制权
    /// (`work` 工具的 `handoff` 动作成功执行)。
    ///
    /// **这是模型的停机权,引擎不得否决。** 原实现里 `Nudge` 是唯一一条引擎否决
    /// 模型判断的动作:模型认为没什么可做了,引擎追加一条推进指令逼它继续,
    /// 于是出现「模型想停 → 引擎 nudge → 模型被迫继续 → 没什么可做 → 引擎又发现
    /// idle → 再 nudge → 模型开始制造工作」的干扰回路。用户 2026-08-21 定调:
    /// 控制权交给模型。资源类停止(限流/致命/连数/ZeroOutput)仍在引擎手里——
    /// 那些不是对「任务完成了没有」的判断。
    pub model_declared_done: bool,
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
    /// D-403:本轮运行失败及其性质;None = 本轮正常完成。失败轮由调用方分类后
    /// 送进判定(瞬态=退避重试,致命=立即停),不再在轮末判定之前提前返回。
    pub round_failure: Option<RoundFailure>,
    /// D-583:本轮「真实进展」签名——调用方按 (HEAD、代码 worktree hash、tracker
    /// 文档内容) 拼出,与上一轮的值逐字节比较;引擎不关心怎么算出来的,只做纯比较。
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
            backlog: BacklogStatus::Workable,
            halted: false,
            // 既有测试全部锚定自主档(引入强度维度前的行为),逐条断言不变。
            intensity: HarnessIntensity::Autonomous,
            model_declared_done: false,
            steps: 1,
            tools,
            auto_allowed: true,
            closed_this_round: 0,
            verify_every_n: 0,
            round_failure: None,
            // 空串 = 不追踪(见 decide() 里的哨兵说明),现有测试默认不关心 D-583;
            // 需要模拟「签名不变/改变」的测试自行覆盖为具体值。
            progress_signature: "",
        }
    }

    /// D-403:瞬态失败退避重试,连续 MAX_FAILED_ROUNDS 轮才停;成功轮清零;
    /// 致命失败立即停;失败轮不吃 NoAction 刹车(steps=1 也不判空转)。
    #[test]
    fn 失败轮_瞬态退避重试_连续三轮停_致命立即停_成功清零() {
        let tools = mk_tools(&["edit"]);
        let mut state = AutoRunState::new(10);
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
            round_failure: None,
            ..ctx_with_tools(&[])
        };
        assert_eq!(state.decide(&ok), AutoRunAction::Continue);
        assert_eq!(state.closed_since_verify, 1, "未达阈值只累计");
        let ok2 = AutoRunCtx {
            steps: 2,
            tools: &mk_tools(&["req", "edit"]),
            closed_this_round: 2,
            verify_every_n: 3,
            round_failure: None,
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
            round_failure: None,
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
            round_failure: None,
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
            intensity: HarnessIntensity::Autonomous,
            model_declared_done: false,
            steps: 2,
            tools: &t,
            auto_allowed: true,
            closed_this_round: 0,
            verify_every_n: 0,
            round_failure: None,
            progress_signature: "",
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

    /// D-583:连续 ZERO_OUTPUT_ROUND_LIMIT 轮真实进展签名不变即熔断——即使每轮都
    /// 调用了 bash(会通过 has_progress_tools),真实状态没变仍要拦下。
    #[test]
    fn 连续三轮真实签名不变_即使工具画像正常也熔断停鞭() {
        let tools = mk_tools(&["bash"]);
        let mut state = AutoRunState::new(10);
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
        assert_eq!(state.rounds, 0, "熔断后计数重置");
    }

    /// D-583:签名一旦变化(真实文件/提交/tracker 有变动)就清零重新起算,不会
    /// 因为「历史上出现过相同签名」被跨轮误伤。
    #[test]
    fn 真实签名变化后清零_不会跨轮误触熔断() {
        let tools = mk_tools(&["bash"]);
        let mut state = AutoRunState::new(10);
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
        let mut state = AutoRunState::new(10);
        let mut ctx = ctx_with_tools(&tools);
        ctx.steps = 2;
        assert_eq!(ctx.progress_signature, "");
        for _ in 0..5 {
            assert_eq!(state.decide(&ctx), AutoRunAction::Continue);
        }
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

    // ---- R-322:门禁强度与模型停机权 ----

    /// #7 双控制器:模型声明完成后,引擎一律不再 Nudge,两档都一样。
    /// 这是本轮最核心的行为变化——原来「模型想停 → 引擎 nudge → 模型被迫继续
    /// → 没什么可做 → 引擎再 nudge → 模型开始制造工作」的干扰回路从此不成立。
    #[test]
    fn 模型声明完成_引擎不再推进_两档一致() {
        for intensity in [HarnessIntensity::Paired, HarnessIntensity::Autonomous] {
            let mut state = AutoRunState::new(10);
            state.rounds = 3;
            // 画像里全是非进展工具 + steps=1:这正是原来会吃 Nudge 的形状。
            let tools = mk_tools(&["read", "grep"]);
            let ctx = AutoRunCtx {
                intensity,
                model_declared_done: true,
                ..ctx_with_tools(&tools)
            };
            assert_eq!(
                state.decide(&ctx),
                AutoRunAction::Stop(AutoStopReason::ModelDeclaredDone),
                "{intensity:?}:模型说完成就是完成,引擎不得改判"
            );
            assert_eq!(state.rounds, 0, "停止后计数归零");
        }
    }

    /// 反证:同样的输入,只把 model_declared_done 关掉,自主档仍走原来的 Nudge。
    /// 两条一起看才说明「变的只是模型声明这一条通路」,不是把刹车拆了。
    #[test]
    fn 未声明完成时_自主档仍走原有推进刹车() {
        let mut state = AutoRunState::new(10);
        state.rounds = 3;
        let tools = mk_tools(&["read", "grep"]);
        let ctx = AutoRunCtx {
            intensity: HarnessIntensity::Autonomous,
            model_declared_done: false,
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
        let mut state = AutoRunState::new(10);
        state.rounds = 3;
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

    /// 资源类兜底与副作用边界不随强度松动:结伴档同样吃限流、致命、连数上限、
    /// backlog 停。这条是 R-322 的护栏——强度只调「任务判断」,不调资源判断。
    #[test]
    fn 结伴档_资源类兜底一条不少() {
        let tools = mk_tools(&["edit", "bash"]);
        let paired = |over: AutoRunCtx<'_>| {
            let mut state = AutoRunState::new(10);
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

        // backlog 清空
        let (action, _) = paired(AutoRunCtx {
            intensity: HarnessIntensity::Paired,
            backlog: BacklogStatus::Empty,
            ..ctx_with_tools(&tools)
        });
        assert_eq!(action, AutoRunAction::Stop(AutoStopReason::BacklogEmpty));

        // 连数上限
        let mut state = AutoRunState::new(2);
        state.rounds = 2;
        let ctx = AutoRunCtx {
            intensity: HarnessIntensity::Paired,
            steps: 5,
            ..ctx_with_tools(&tools)
        };
        assert_eq!(
            state.decide(&ctx),
            AutoRunAction::Stop(AutoStopReason::MaxRounds(2))
        );
    }

    /// D-583 的 ZeroOutput 熔断是**资源判断**(比对真实签名),不随强度关闭。
    /// 别看它长得像任务判断就按强度门控——那是无人值守唯一的失控兜底。
    #[test]
    fn 结伴档_零产出熔断仍然生效() {
        let mut state = AutoRunState::new(10);
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

    /// 验收核查轮属任务判断:自主档插队,结伴档由用户当场验收所以不插。
    /// 计数照常累加——中途切回自主档时节律不从零重来。
    #[test]
    fn 验收核查轮_仅自主档插入_计数两档都累加() {
        let tools = mk_tools(&["edit", "bash"]);
        let mk = |intensity| AutoRunCtx {
            intensity,
            steps: 5,
            closed_this_round: 3,
            verify_every_n: 3,
            ..ctx_with_tools(&tools)
        };

        let mut auto = AutoRunState::new(10);
        assert_eq!(
            auto.decide(&mk(HarnessIntensity::Autonomous)),
            AutoRunAction::VerifyRound
        );

        let mut paired = AutoRunState::new(10);
        assert_eq!(
            paired.decide(&mk(HarnessIntensity::Paired)),
            AutoRunAction::Continue
        );
        assert_eq!(
            paired.closed_since_verify, 3,
            "结伴档不插核查轮,但关闭计数照常累加,切回自主档时节律接得上"
        );
    }

    /// 用户意图优先于模型声明:用户按了停/暂停,不需要征询模型意见。
    #[test]
    fn 用户意图仍然压过模型声明() {
        let tools = mk_tools(&["edit"]);
        let mut state = AutoRunState::new(10);
        state.paused = true;
        let ctx = AutoRunCtx {
            model_declared_done: true,
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
        assert!(auto.engine_nudge && auto.redundancy_hints && auto.verify_rounds);
        let paired = HarnessIntensity::Paired.policy();
        assert!(!paired.engine_nudge && !paired.redundancy_hints && !paired.verify_rounds);
    }
}
