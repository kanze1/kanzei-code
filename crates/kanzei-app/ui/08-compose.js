// ---------- 发送 / 停止 ----------
// 鞭挞状态:自动续跑计数(手动发送归零),上限防失控。
const DEFAULT_AUTO_CONTINUE_MAX = 10;
let autoRounds = 0;
let autoPaused = false;
let autoStopAfterRound = false;
let autoContinueTimer = null;
let autoContinueGeneration = 0;
let autoStopReason = "";
// 连续无实质动作的轮数:第一次只追加推进指令,第二次才刹车。
let noActionRounds = 0;
// R-076:不构成实质进展的工具。一轮里只有这些(纯查询/探测/写记忆日记)时
// 仍算空转——模型不能再靠 memory_note 或无关读取绕过刹车(D-044 教训的硬化)。
// bash/git/edit/write/tracker 等可能改变状态的工具不在列:名称粒度分不出
// git status 与 git commit,误判成空转的代价(真干活被打断)比漏判高。
const NON_PROGRESS_TOOLS = new Set([
  "memory_note", "memory_search", "memory_stats",
  "read", "grep", "glob", "webfetch",
  "ui_dom", "ui_console", "ui_style", "frontend_locate", "frontend_check",
  "task",
]);
function hasProgressTools(tools) {
  if (!tools || typeof tools !== "object") return false;
  return Object.keys(tools).some((name) => !NON_PROGRESS_TOOLS.has(name));
}
// R-157:节奏配置渲染规则 6。默认值 = conventions §1.4(entry_close/every_commit/
// per_batch/per_entry),settings_get 异步填充后继续文案随配置变化。
const DEFAULT_CADENCE = {
  full_test: "entry_close",
  full_test_batches: null,
  targeted_test: "every_commit",
  commit: "per_batch",
  push: "per_entry",
};
let cadenceSettings = null; // 异步加载;未就绪时按 DEFAULT_CADENCE 渲染(与 §1.4 一致)
let lastRenderedPrompt = null; // 最近一次渲染给用户的默认文案;用于判断"用户是否自定义过"
function effectiveCadence() {
  return cadenceSettings && typeof cadenceSettings === "object" ? cadenceSettings : DEFAULT_CADENCE;
}
// R-157:设置页(或启动)拿到 settings_get 后调用——把生效节奏存下来,并让
// continue-prompt 文案跟着重渲染(仅当用户没自定义过文案;自定义的绝不覆盖)。
function applyCadenceSettings(s) {
  if (!s || typeof s.cadence !== "object" || !s.cadence) return;
  cadenceSettings = s.cadence;
  const textarea = $("continue-prompt");
  const current = (textarea.value || "").trim();
  if (!current || current === lastRenderedPrompt) {
    lastRenderedPrompt = buildContinuePrompt(effectiveCadence());
    textarea.value = lastRenderedPrompt;
    localStorage.setItem("kz-continue-prompt", lastRenderedPrompt);
  }
}
function cadenceVerificationText(c) {
  const ft = c.full_test || "entry_close";
  const full =
    ft === "every_commit" ? "全量测试每次提交前跑" :
    ft === "every_n_batches" ? `全量测试每 ${c.full_test_batches || 3} 批跑一次` :
    ft === "release_only" ? "全量测试只在发版前跑(本地不跑)" :
    "全量测试条目关闭前跑一次";
  const tt = c.targeted_test || "every_commit";
  const targeted = tt === "off" ? "定向测试关闭:验证按改动面自选" :
    "动了 crates/ 每次提交前跑定向 cargo test -p <改动 crate>";
  const cm = c.commit || "per_batch";
  const commit = cm === "per_entry" ? "每条目一提交" : "多批大条目每批一提交(回滚锚点)";
  const pu = c.push || "per_entry";
  const push =
    pu === "per_commit" ? "每提交后 push" :
    pu === "periodic" ? "定期自动 push(引擎自动,失败可见不阻断)" :
    "条目完成后 push";
  return `${full};${targeted};${commit};${push}`;
}
function buildContinuePrompt(c) {
  return (
    CONTINUE_PROMPT_HEAD +
    "6. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
    "验证选择与改动面匹配:纯 ui/ 改动跑 node 检查与冒烟脚本;" +
    cadenceVerificationText(c) +
    "。\n" +
    CONTINUE_PROMPT_TAIL
  );
}

const CONTINUE_PROMPT_HEAD =
  "继续推进。取活顺序按本轮末尾给出的「开发重心」执行(它来自记忆里的用户定调,是唯一权威);" +
  "两个队列内部都按文档顺序自上而下拿第一个可做的,列表已按阶段排好,不要自行挑看起来容易的。\n" +
  "1. 本轮必须产生落地动作:改代码或跑测试。先做再说明,不要只做判断。\n" +
  "2. 粒度 = 一轮一个批次。复杂度「小」的条目一轮做完,不分批;" +
  "复杂度「中」「大」的条目,**第一轮的落地动作就是定出批次表并写入 `批次: 0/N`**——" +
  "N 是侧栏那排格子的格数,默认由复杂度给(中 3、大 8),实际批数不止就照实写(如 `批次: 0/11`)。" +
  "此后每做完一个批次就改成 `批次: k/N`:这是外部唯一看得见推进的地方,不填格等于没推进。" +
  "关闭时批次必须走满;当初估多了就把总数改成实际批数(`批次: 5/5`)——改它比留着空格诚实。" +
  "同构批量改动(i18n、重命名、迁移这类)一轮吃掉完整类别,不要按两三处微切片。" +
  "「工作量大」「要改多个文件」都是正常工作,不是停下的理由。\n" +
  "3. 卡住就换一条:某条一时推不动,在「进展」里记一句原因,直接跳到下一条继续,不要停下来等。\n" +
  "「阻塞」字段只写解除权不在你手里的事(已问过用户在等回复/缺凭据/依赖外部服务/用户直营)," +
  "且要写出具名解除人;「涉及多文件」「跨层改动」「需先确认方案(但没真问过)」都不是阻塞,写进展。" +
  "顺手复核碰到的条目:阻塞条件已满足的当场清空「阻塞」字段。看到 [调度死锁] 横幅时按横幅执行。\n" +
  "4. 关闭条目前逐条对照验收原文,每项给出精确代码位置证据;声称完成的能力必须有真实调用方或消费者," +
  "没有消费者的命令、死代码或只展示不接数据源的壳不算完成;沿用既有实现要显式标注为既有能力而非本次交付;" +
  "不得缩小验收里的平台或范围限定词。任一项证据不足就保留活动态写清缺口,不要打勾。\n" +
  "5. doing 最多 2 个;已满就继续推进这两项。标着「阶段 5 后」的功能需求暂不启动。\n";
const CONTINUE_PROMPT_TAIL = "一直做下去,不要用纯文本收尾。";
// 默认文案(未自定义时兜底,= 默认节奏渲染)。
const DEFAULT_CONTINUE_PROMPT = buildContinuePrompt(DEFAULT_CADENCE);

// 旧版默认文案:用户没改过(存的就是某个旧默认)时静默升级到新默认,
// 否则鞭挞的刹车契约会和提示词对不上(用户自定义过的文案不动)。
const LEGACY_CONTINUE_PROMPTS = [
  // 一轮一条目版:大条目从头到尾只有一个 doing,进展字段单行覆写,外部看不出推进
  // (R-153 一天 36 个提交、8 个批次,界面上始终是一条不动的 doing)。改为一轮一批次 +
  // 侧栏格子计数。
  "继续推进。取活顺序按本轮末尾给出的「开发重心」执行(它来自记忆里的用户定调,是唯一权威);" +
    "两个队列内部都按文档顺序自上而下拿第一个可做的,列表已按阶段排好,不要自行挑看起来容易的。\n" +
    "1. 本轮必须产生落地动作:改代码或跑测试。先做再说明,不要只做判断。\n" +
    "2. 粒度 = 一轮一个完整条目:以做完当前这一条缺陷/需求为本轮目标;" +
    "同构批量改动(i18n、重命名、迁移这类)一轮吃掉完整类别,不要按两三处微切片。" +
    "确实超出单轮容量才按验收子项分轮,并在进展里写明批次边界。" +
    "「工作量大」「要改多个文件」都是正常工作,不是停下的理由。\n" +
    "3. 卡住就换一条:某条一时推不动,在「进展」里记一句原因,直接跳到下一条继续,不要停下来等。\n" +
    "「阻塞」字段只写解除权不在你手里的事(已问过用户在等回复/缺凭据/依赖外部服务/用户直营)," +
    "且要写出具名解除人;「涉及多文件」「跨层改动」「需先确认方案(但没真问过)」都不是阻塞,写进展。" +
    "顺手复核碰到的条目:阻塞条件已满足的当场清空「阻塞」字段。看到 [调度死锁] 横幅时按横幅执行。\n" +
    "4. 关闭条目前逐条对照验收原文,每项给出精确代码位置证据;声称完成的能力必须有真实调用方或消费者," +
    "没有消费者的命令、死代码或只展示不接数据源的壳不算完成;沿用既有实现要显式标注为既有能力而非本次交付;" +
    "不得缩小验收里的平台或范围限定词。任一项证据不足就保留活动态写清缺口,不要打勾。\n" +
    "5. doing 最多 2 个;已满就继续推进这两项。标着「阶段 5 后」的功能需求暂不启动。\n" +
    "6. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
    "验证选择与改动面匹配:纯 ui/ 改动跑 node 检查与冒烟脚本,动了 crates/ 才跑 cargo test。\n" +
    "一直做下去,不要用纯文本收尾。",
  // 开发重心版:规则 3 只说"在条目里记一句原因",模型把它落成「阻塞」字段,
  // 而调度器把该字段当永久压制 → 31/35 条目被自记阻塞锁死(D-163)。
  "继续推进。取活顺序按本轮末尾给出的「开发重心」执行(它来自记忆里的用户定调,是唯一权威);" +
    "两个队列内部都按文档顺序自上而下拿第一个可做的,列表已按阶段排好,不要自行挑看起来容易的。\n" +
    "1. 本轮必须产生落地动作:改代码或跑测试。先做再说明,不要只做判断。\n" +
    "2. 粒度 = 一轮一个完整条目:以做完当前这一条缺陷/需求为本轮目标;" +
    "同构批量改动(i18n、重命名、迁移这类)一轮吃掉完整类别,不要按两三处微切片。" +
    "确实超出单轮容量才按验收子项分轮,并在进展里写明批次边界。" +
    "「工作量大」「要改多个文件」都是正常工作,不是停下的理由。\n" +
    "3. 卡住就换一条:某条一时推不动,在条目里记一句原因,直接跳到下一条继续,不要停下来等。\n" +
    "4. 关闭条目前对照验收原文,给出改动位置;没有调用方的命令或按钮不算完成;" +
    "沿用既有实现要说明。拿不准就保留活动态写清缺口,不要打勾。\n" +
    "5. doing 最多 2 个;已满就继续推进这两项。标着「阶段 5 后」的功能需求暂不启动。\n" +
    "6. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
    "验证选择与改动面匹配:纯 ui/ 改动跑 node 检查与冒烟脚本,动了 crates/ 才跑 cargo test。\n" +
    "一直做下去,不要用纯文本收尾。",
  // 硬编码取活顺序版:开篇写死"先扫 defects.md",与结尾追加的取活模式行直接矛盾,
  // 开篇权威句胜出 → 用户切「需求优先」始终不生效(D-128)。
  "继续推进。取活顺序 = 文档顺序:先从上往下扫 defects.md,再扫 requirements.md," +
    "拿第一个可做的。列表已按阶段排好,不要自行挑看起来容易的。\n" +
    "1. 本轮必须产生落地动作:改代码或跑测试。先做再说明,不要只做判断。\n" +
    "2. 粒度 = 一轮一个完整条目:以做完当前这一条缺陷/需求为本轮目标;" +
    "同构批量改动(i18n、重命名、迁移这类)一轮吃掉完整类别,不要按两三处微切片。" +
    "确实超出单轮容量才按验收子项分轮,并在进展里写明批次边界。" +
    "「工作量大」「要改多个文件」都是正常工作,不是停下的理由。\n" +
    "3. 卡住就换一条:某条一时推不动,在条目里记一句原因,直接跳到下一条继续,不要停下来等。\n" +
    "4. 关闭条目前对照验收原文,给出改动位置;没有调用方的命令或按钮不算完成;" +
    "沿用既有实现要说明。拿不准就保留活动态写清缺口,不要打勾。\n" +
    "5. doing 最多 2 个;已满就继续推进这两项。标着「阶段 5 后」的功能需求暂不启动。\n" +
    "6. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
    "验证选择与改动面匹配:纯 ui/ 改动跑 node 检查与冒烟脚本,动了 crates/ 才跑 cargo test。\n" +
    "一直做下去,不要用纯文本收尾。",
  // 微切片版:「最小可执行步骤」导致 i18n 类批量任务两三处一轮,单条缺陷拖 30+ 轮(D-114,用户定调改为一轮一条目)。
  "继续推进。取活顺序 = 文档顺序:先从上往下扫 defects.md,再扫 requirements.md," +
    "拿第一个可做的。列表已按阶段排好,不要自行挑看起来容易的。\n" +
    "1. 本轮必须产生落地动作:改代码或跑测试。先做再说明,不要只做判断。\n" +
    "2. 大项拆着做,本轮只推进下一个最小可执行步骤。" +
    "「工作量大」「要改多个文件」「需要多轮」都是正常工作,不是停下的理由。\n" +
    "3. 卡住就换一条:某条一时推不动,在条目里记一句原因,直接跳到下一条继续,不要停下来等。\n" +
    "4. 关闭条目前对照验收原文,给出改动位置;没有调用方的命令或按钮不算完成;" +
    "沿用既有实现要说明。拿不准就保留活动态写清缺口,不要打勾。\n" +
    "5. doing 最多 2 个;已满就继续推进这两项。标着「阶段 5 后」的功能需求暂不启动。\n" +
    "6. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。\n" +
    "一直做下去,不要用纯文本收尾。",
  "继续:先检查缺陷列表,再检查需求与活跃目标,推进下一个具体步骤并落地(改代码/跑测试/更新文档);" +
    "完成后用 goal update 记录状态。收尾优先:已是 doing 的事项先关闭再开新的,doing 同时不超过 2 个。" +
    "取活顺序:按缺陷列表优先,随后按需求列表自上而下拿第一个可做的(列表顺序即用户意志,priority 只是背景信息)。" +
    "若工作区有已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
    "若活跃目标/需求全部被阻塞或无可推进项:只用【纯文本】说明原因并停住——" +
    "不要调用任何工具、不要往 goal/req 写'仍在阻塞'类记录、不要产生空提交;" +
    "纯文本回复会让鞭挞自动刹车,写阻塞日记则会让它空转烧钱。",
  // D-097 版:引入【阻塞】刹车契约,但还没有阶段/证据等级与完成判定约束。
  "继续推进。取活顺序:缺陷列表优先,然后按需求列表自上而下拿第一个可做的" +
    "(列表顺序即用户意志,priority 只是背景信息)。\n" +
    "1. 本轮必须产生一个具体落地动作:改代码、跑测试、或更新文档。先做再说明,不要只做判断。\n" +
    "2. 大项拆着做:复杂度大的条目不要求本轮关闭,只要推进它的下一个最小可执行步骤。" +
    "「工作量大」「要改多个文件」「需要多轮」都不是阻塞,是正常工作。\n" +
    "3. doing 已满 2 个不代表没事可做——那意味着继续推进这两个 doing 项,而不是停下。\n" +
    "4. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。\n" +
    "5. 只有确实缺少外部输入时才算阻塞:等待用户拍板、缺凭据/权限、依赖外部服务或他人。" +
    "此时回复以【阻塞】开头的纯文本,写清缺什么、解除条件是什么,不要调用任何工具、" +
    "不要往 goal/req/defect 写「仍在阻塞」类记录、不要产生空提交。\n" +
    "除【阻塞】外不要用纯文本收尾——没有动作的一轮会被判为空转。",
  // 证据等级版:测试门槛过严,且仍保留【阻塞】出口(用户定调:阻塞太好走,取消)。
  "继续推进。取活顺序 = 文档顺序:先从上往下扫 defects.md,再从上往下扫 requirements.md," +
    "拿第一个可做的。列表已按质量阶段排好,不要自行挑看起来容易的。\n" +
    "1. 本轮必须产生落地动作:改代码、跑测试或补证据。先做再说明,不要只做判断。\n" +
    "2. 每条都带 `阶段`/`不变量`/`证据等级`。修复要保护该不变量本身,不能把故障挪到另一条路径;" +
    "验证要达到标注的证据等级——E2 需跨模块/并发/故障注入,E3 需真实运行时;" +
    "单元测试证明不了 E2 结论,静态检查证明不了任何运行时结论。\n" +
    "3. 关闭条目前逐条对照验收原文:每项给出代码位置证据;声称完成的能力必须有真实调用方" +
    "(没有消费者的命令或按钮判为未完成);沿用既有实现要显式标注为既有能力;" +
    "不得缩小验收里的平台或范围限定词。做不到就保留活动态并写清缺口,不要打勾。\n" +
    "4. 大项拆着做:本轮只推进下一个最小可执行步骤。" +
    "「工作量大」「要改多个文件」「需要多轮」都不是阻塞,是正常工作。\n" +
    "5. doing 最多 2 个;已满就继续推进这两项,不是停下。标着「阶段 5 后」的功能需求," +
    "在质量收口完成前不启动。\n" +
    "6. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。\n" +
    "7. 只有确实缺少外部输入时才算阻塞:等待用户拍板、缺凭据/权限、依赖外部服务或他人。" +
    "此时回复以【阻塞】开头的纯文本,写清缺什么、解除条件是什么,不要调用任何工具、" +
    "不要往 goal/req/defect 写「仍在阻塞」类记录、不要产生空提交。\n" +
    "除【阻塞】外不要用纯文本收尾——没有动作的一轮会被判为空转。",
];
// 没有实质动作时先给一次具体的推进指令,而不是直接停:一轮无动作往往是模型
// 在"这条该不该做"上想岔了,而不是真没活干(D-097)。
const NUDGE_PROMPT =
  "上一轮没有产生任何实质动作。不要再做可行性判断,直接执行:\n" +
  "从 defects.md 最上面一条开始,说出它的下一个最小可执行步骤(具体到文件和改动),然后立刻做掉。\n" +
  "那一条一时推不动就跳到下一条,需求同理——总有一条是能动手的。\n" +
  "如果每一条都标着阻塞:先复核阻塞是否还成立。多数是你自己历轮写下的,解除条件早已满足,\n" +
  "清空这些条目的「阻塞」字段再取活;真正卡住的只有等用户拍板的那几条,把它们点名列给用户。\n" +
  "不要为了凑动作去做与当前条目无关的事,也不要只更新追踪文档就算一轮。";

function nudgePrompt() {
  const first = selectedWorkPriority() === "requirement-first" ? "requirements.md" : "defects.md";
  const second = selectedWorkPriority() === "requirement-first" ? "defects.md" : "requirements.md";
  return NUDGE_PROMPT.replace("defects.md", first).replace("需求同理", `${second} 同理`);
}

function selectedAgent() {
  const mode = $("profile-select").value;
  if (mode === "dev-pair") return { profile: "dev", agent: "dev-pair" };
  if (mode === "dev-auto") return { profile: "dev", agent: "dev" };
  return { profile: "research", agent: "research" };
}
function workPriorityStorageKey() {
  return `kz-work-priority:${currentProject || "default"}`;
}
function selectedWorkPriority() {
  return $("work-priority-select").value === "requirement-first" ? "requirement-first" : "defect-first";
}
function syncWorkPriorityControl() {
  const saved = localStorage.getItem(workPriorityStorageKey());
  $("work-priority-select").value = saved === "requirement-first" ? saved : "defect-first";
  loadWorkFocus();
}

// 开发重心 = preference 记忆条目(真源)。下拉框只是快捷写法,记忆页可手写任意细度
// (「先收完这批缺陷再转需求」这类二元开关表达不了的意图);提示词由记忆生成,
// 所以开关与提示词不可能再互相矛盾——D-128 的根因就是二者写死后对打。
let workFocusMemory = null;
const WORK_FOCUS_PRESETS = {
  "defect-first": {
    title: "开发重心:缺陷优先",
    body: "取活顺序:先从上到下扫描 defects.md,再扫描 requirements.md;前一个队列没有可做项时才看后一个。\npriority 标签只是背景信息,不改变列表顺序。",
  },
  "requirement-first": {
    title: "开发重心:需求优先",
    body: "取活顺序:先从上到下扫描 requirements.md,再扫描 defects.md;前一个队列没有可做项时才看后一个。\npriority 标签只是背景信息,不改变列表顺序。",
  },
};
async function loadWorkFocus() {
  if (!currentProject) return;
  try {
    workFocusMemory = await invoke("memory_focus_get", { projectDir: currentProject });
  } catch {
    workFocusMemory = null;
  }
  // 回显:手写的自定义重心不强行归入两个预设,保持用户当前选择不被覆盖。
  const title = workFocusMemory?.title || "";
  if (title.includes("需求优先")) $("work-priority-select").value = "requirement-first";
  else if (title.includes("缺陷优先")) $("work-priority-select").value = "defect-first";
}

function renderAutoStatus(text = autoStopReason) {
  const el = $("auto-status");
  if (!el) return;
  const max = autoContinueMax();
  el.textContent = localizeDynamic(text || `连续推进上限 ${max}`);
}
function continuePrompt() {
  const base = $("continue-prompt").value.trim() || buildContinuePrompt(effectiveCadence());
  // 重心正文优先取记忆(用户可手写细度);记忆缺失时回落到下拉框预设。
  const focus = workFocusMemory?.body?.trim() || WORK_FOCUS_PRESETS[selectedWorkPriority()].body;
  const from = workFocusMemory?.id ? `记忆 ${workFocusMemory.id}` : "当前选择";
  return `${base}\n开发重心(来自${from},这是取活顺序的唯一权威):\n${focus}`;
}

function setAutoStopReason(reason) {
  autoStopReason = reason;
  renderAutoStatus(reason);
}
function autoContinueAllowed() {
  return $("profile-select").value === "dev-auto";
}
function autoContinueMax() {
  const value = Number.parseInt($("auto-max").value, 10);
  return Number.isFinite(value) ? Math.min(100, Math.max(1, value)) : DEFAULT_AUTO_CONTINUE_MAX;
}
function cancelAutoContinueTimer() {
  if (autoContinueTimer) clearTimeout(autoContinueTimer);
  autoContinueTimer = null;
  autoContinueGeneration += 1;
}
function scheduleAutoContinue() {
  cancelAutoContinueTimer();
  const generation = autoContinueGeneration;
  autoContinueTimer = setTimeout(() => {
    autoContinueTimer = null;
    if (generation !== autoContinueGeneration || autoPaused || autoStopAfterRound) return;
    if ($("auto-continue").checked && autoContinueAllowed() && !running) {
      sendText(continuePrompt(), { auto: true });
    }
  }, 2000);
}

async function stopAutoWhenBacklogEmpty() {
  if (!$("auto-continue").checked || !autoContinueAllowed()) return false;
  try {
    const snapshot = await invoke("docs_snapshot", { projectDir: currentProject });
    // R-076:只数"可推进"的条目——closed/终态不算,外部阻塞(阻塞:字段有值)也不算。
    // blocked-but-open 的 backlog 同样没有可做的事,继续跑只会空转烧钱;空与全阻塞
    // 给出不同的停止原因,状态迁移可区分、可断言。
    const active = [...(snapshot.requirements ?? []), ...(snapshot.defects ?? [])].filter(
      (entry) => !entry.closed && !["done", "dropped", "fixed", "wontfix"].includes(entry.status)
    );
    const workable = active.some((entry) => !entryBlocked(entry));
    if (workable) return false;
    const blocked = active.length > 0 && active.every((entry) => entryBlocked(entry));
    $("auto-continue").checked = false;
    localStorage.setItem("kz-auto-continue", "0");
    autoRounds = 0;
    cancelAutoContinueTimer();
    const reason = blocked ? t("需求与缺陷全部被阻塞，自动推进已停止") : t("需求与缺陷已清空，自动推进已停止");
    setAutoStopReason(reason);
    addMessage("notice", `✅ ${reason}`);
    log(blocked ? t("自动推进停止:需求与缺陷全部被阻塞") : t("自动推进停止:需求与缺陷已清空"));
    return true;
  } catch (error) {
    log(`${t("检查需求/缺陷是否清空失败")}:${error}`, "warn");
    return false;
  }
}
function renderAttachments() {
  const box = $("attachments");
  box.innerHTML = "";
  box.classList.toggle("hidden", attachments.length === 0);
  attachments.forEach((item, index) => {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.className = "attachment-chip";
    chip.textContent = `${item.file_name} ×`;
    chip.title = t("移除附件");
    chip.setAttribute("aria-label", `${t("移除附件")} ${item.file_name}`);
    chip.addEventListener("click", () => { attachments.splice(index, 1); renderAttachments(); });
    box.appendChild(chip);
  });
}

function addFiles(files) {
  for (const file of files) {
    if (!(file.type.startsWith("image/") || file.type === "application/pdf" || file.name.toLowerCase().endsWith(".pdf"))) {
      toast(`${t("不支持的附件类型")}: ${file.name}`);
      continue;
    }
    const reader = new FileReader();
    reader.onload = () => {
      const dataUrl = String(reader.result);
      attachments.push({ file_name: file.name, media_type: file.type || "application/pdf", data: dataUrl.split(",", 2)[1] || "" });
      renderAttachments();
    };
    reader.onerror = () => toastError(`${t("读取附件失败")}: ${file.name}`);
    reader.readAsDataURL(file);
  }
}

$("attach").addEventListener("click", () => $("attachment-input").click());
$("attachment-input").addEventListener("change", (e) => { addFiles(e.target.files); e.target.value = ""; });
promptBox.addEventListener("dragover", (e) => { e.preventDefault(); });
promptBox.addEventListener("drop", (e) => { e.preventDefault(); addFiles(e.dataTransfer.files); });
promptBox.addEventListener("paste", (e) => {
  const files = [...(e.clipboardData?.files || [])];
  if (files.length) { e.preventDefault(); addFiles(files); }
});

async function sendText(prompt, { auto = false, promptAttachments = [] } = {}) {
  // 任何拒绝发送的理由都要说出来,绝不静默(D-004)。
  if (!prompt) return;
  const delivery = $("delivery-select").value;
  if (running && auto) {
    toast(t("当前任务还在运行，自动鞭挞将在本轮完成后继续"));
    return;
  }
  if (!currentProject) {
    toast(t("先在左侧「项目」里添加并选择一个目录"));
    return;
  }
  if (!auto) void ensureNotificationPermission();
  if (running) {
    addMessage("user", prompt);
    log(`${t("运行中")}${delivery === "steer" ? t("插入") : t("排队")}:${prompt.slice(0, 80)}`);
    try {
      const mode = selectedAgent();
      await invoke("run_prompt", {
        prompt,
        projectDir: currentProject,
        profile: mode.profile,
        agent: mode.agent,
        model: $("model-select").value || null,
        delivery,
        attachments: promptAttachments,
        processId: activeProcessId,
      });
      toast(localizeDynamic(delivery === "steer" ? "已插入当前会话，将优先执行" : "已加入队列，将按顺序执行"));
      await refreshPendingInputs();
    } catch (err) {
      reportError(String(err), { retryable: false });
    }
    return;
  }
  if (!auto) {
    autoRounds = 0;
    noActionRounds = 0;
    cancelAutoContinueTimer();
  }
  currentAssistant = null;
  currentReasoning = null;
  runTokens = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };
  ctxTokens = 0;
  outputChars = 0;
  renderTokens();
  const attachmentStatus = promptAttachments.length > 0
    ? `${auto ? `${t("鞭挞")} ${autoRounds}/${autoContinueMax()} · ` : ""}${t("正在发送")} ${promptAttachments.length} ${t("个附件")} · ${t("准备中")}`
    : auto ? `${t("鞭挞")} ${autoRounds}/${autoContinueMax()} · ${t("准备中")}` : t("准备中");
  if (auto) {
    addMessage("notice", `${t("鞭挞已触发")} · ${autoRounds}/${autoContinueMax()}`);
  } else {
    addUserMessage(prompt, promptAttachments);
  }
  setRunning(true, attachmentStatus);
  // R-086:本轮运行开始,活动会话状态机同步为运行中——控制事件与状态机同源。
  // converged 复位:新一轮可以覆盖旧终态。
  if (activeSessionId) {
    const state = sessionState(activeSessionId);
    state.running = true;
    state.converged = false;
  }
  startElapsed();
  log(`${auto ? t("鞭挞") : t("发送")}:${prompt.slice(0, 80)}`);
  try {
    const mode = selectedAgent();
    const request = {
      prompt,
      projectDir: currentProject,
      profile: mode.profile,
      agent: mode.agent,
      model: $("model-select").value || null,
      workPriority: selectedWorkPriority(),
      delivery,
      attachments: promptAttachments.map((item) => ({ ...item })),
      processId: activeProcessId,
    };
    if (!auto) lastRequest = request;
    await invoke("run_prompt", request);
  } catch (err) {
    reportError(String(err));
    stopElapsed();
    setRunning(false);
  }
}

const PROMPT_HISTORY_KEY = "kz-prompt-history";
const PROMPT_HISTORY_LIMIT = 30;
let promptHistory = (() => {
  try { return JSON.parse(localStorage.getItem(PROMPT_HISTORY_KEY) || "[]").filter((item) => typeof item === "string"); }
  catch (_) { return []; }
})();
let promptHistoryIndex = -1;
let promptHistoryDraft = "";

function rememberPrompt(prompt) {
  const value = prompt.trim();
  if (!value) return;
  promptHistory = [value, ...promptHistory.filter((item) => item !== value)].slice(0, PROMPT_HISTORY_LIMIT);
  localStorage.setItem(PROMPT_HISTORY_KEY, JSON.stringify(promptHistory));
  promptHistoryIndex = -1;
}

function navigatePromptHistory(direction) {
  if (promptHistory.length === 0) return false;
  if (promptHistoryIndex === -1) promptHistoryDraft = promptBox.value;
  const next = promptHistoryIndex + direction;
  if (next < 0 || next > promptHistory.length) return false;
  promptHistoryIndex = next;
  promptBox.value = next === promptHistory.length ? promptHistoryDraft : promptHistory[next];
  promptBox.setSelectionRange(promptBox.value.length, promptBox.value.length);
  return true;
}

let fileSuggestions = [];
let fileSuggestionIndex = -1;
let fileSuggestionToken = null;
let fileSuggestionRequest = 0;

function currentFileToken() {
  const cursor = promptBox.selectionStart;
  const before = promptBox.value.slice(0, cursor);
  const match = before.match(/(?:^|\s)@([^\s]*)$/);
  if (!match) return null;
  return { start: cursor - match[1].length - 1, end: cursor, query: match[1] };
}

function hideFileSuggestions() {
  fileSuggestions = [];
  fileSuggestionIndex = -1;
  fileSuggestionToken = null;
  $("file-suggestions").classList.add("hidden");
  $("file-suggestions").replaceChildren();
}

function renderFileSuggestions() {
  const box = $("file-suggestions");
  box.replaceChildren();
  fileSuggestions.forEach((path, index) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `file-suggestion${index === fileSuggestionIndex ? " active" : ""}`;
    button.textContent = `@${path}`;
    button.addEventListener("mousedown", (event) => {
      event.preventDefault();
      chooseFileSuggestion(index);
    });
    box.appendChild(button);
  });
  box.classList.toggle("hidden", fileSuggestions.length === 0);
}

function chooseFileSuggestion(index = fileSuggestionIndex) {
  const path = fileSuggestions[index];
  const token = currentFileToken() || fileSuggestionToken;
  if (!path || !token) return;
  promptBox.value = `${promptBox.value.slice(0, token.start)}@${path} ${promptBox.value.slice(token.end)}`;
  const cursor = token.start + path.length + 2;
  promptBox.focus();
  promptBox.setSelectionRange(cursor, cursor);
  hideFileSuggestions();
}

async function refreshFileSuggestions() {
  const token = currentFileToken();
  if (!token || !currentProject) {
    hideFileSuggestions();
    return;
  }
  fileSuggestionToken = token;
  const request = ++fileSuggestionRequest;
  try {
    const paths = await invoke("project_files", { projectDir: currentProject, query: token.query });
    if (request !== fileSuggestionRequest || !currentFileToken()) return;
    fileSuggestions = paths;
    fileSuggestionIndex = paths.length ? 0 : -1;
    renderFileSuggestions();
  } catch (error) {
    hideFileSuggestions();
    log(`文件补全失败:${error}`, "warn");
  }
}

let fileSuggestionTimer = null;
promptBox.addEventListener("input", () => {
  promptHistoryIndex = -1;
  clearTimeout(fileSuggestionTimer);
  fileSuggestionTimer = setTimeout(refreshFileSuggestions, 80);
});
function stopAutoForManualInput() {
  if (!$('auto-continue').checked) return false;
  $('auto-continue').checked = false;
  localStorage.setItem("kz-auto-continue", "0");
  autoRounds = 0;
  noActionRounds = 0;
  cancelAutoContinueTimer();
  const message = t("收到手动输入，鞭挞已停止");
  setAutoStopReason(message);
  addMessage("notice", message);
  toast(message);
  log(message);
  return true;
}

function send() {
  const prompt = promptBox.value.trim();
  if (!prompt && attachments.length === 0) return;
  stopAutoForManualInput();
  // 只有附件没有文字时,sendText 的空 prompt 早退会静默吞掉附件(附件在此已被清空)。
  // 给一句默认描述,让图片/文件真的发得出去。
  if (!prompt && attachments.length > 0) {
    sendText(t("看一下这些附件"), { promptAttachments: attachments });
    promptBox.value = "";
    attachments = [];
    renderAttachments();
    return;
  }
  rememberPrompt(prompt);
  hideFileSuggestions();
  const promptAttachments = attachments;
  promptBox.value = "";
  attachments = [];
  renderAttachments();
  sendText(prompt, { promptAttachments });
}

$("send").addEventListener("click", send);
$("continue-btn").addEventListener("click", () => sendText(continuePrompt()));

async function openSopPicker() {
  if (!currentProject) {
    toast(t("先在左侧「项目」里添加并选择一个目录"));
    return;
  }
  const panel = $("sop-picker-panel");
  const list = $("sop-list");
  panel.classList.remove("hidden");
  list.replaceChildren();
  const loading = document.createElement("p");
  loading.className = "dim";
  loading.textContent = `${t("选择 SOP")}…`;
  list.appendChild(loading);
  try {
    const scopes = await Promise.all(["project", "global"].map((scope) =>
      invoke("memory_entries", { projectDir: currentProject, scope, category: "sop" })
    ));
    const entries = scopes.flat().filter((entry) => entry.status === "active");
    list.replaceChildren();
    if (!entries.length) {
      const empty = document.createElement("p");
      empty.className = "dim";
      empty.textContent = t("暂无可调用的 SOP");
      list.appendChild(empty);
      return;
    }
    for (const entry of entries) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "sop-entry";
      const title = document.createElement("strong");
      title.textContent = entry.title;
      const description = document.createElement("span");
      description.className = "dim";
      description.textContent = entry.description || entry.body?.slice(0, 120) || "";
      button.append(title, description);
      button.addEventListener("click", () => {
        const content = String(entry.body || "").trim();
        promptBox.value = content;
        panel.classList.add("hidden");
        stopAutoForManualInput();
        promptBox.focus();
        if (!content) {
          toast(t("SOP 内容为空"));
          return;
        }
        rememberPrompt(content);
        const delivery = $("delivery-select");
        const previous = delivery.value;
        delivery.value = "queue";
        void sendText(content).finally(() => { delivery.value = previous; });
        toast(t("SOP 已填入继续输入"));
      });
      list.appendChild(button);
    }
  } catch (error) {
    list.replaceChildren();
    const failed = document.createElement("p");
    failed.className = "dim";
    failed.textContent = `${t("SOP 加载失败")}: ${error}`;
    list.appendChild(failed);
  }
}
$("sop-picker").addEventListener("click", openSopPicker);
$("sop-picker-close").addEventListener("click", () => $("sop-picker-panel").classList.add("hidden"));

$("continue-toggle").addEventListener("click", () => {
  const panel = $("continue-panel");
  const open = panel.classList.toggle("hidden") === false;
  $("continue-toggle").setAttribute("aria-expanded", String(open));
  $("continue-toggle").textContent = t(open ? "收起文案" : "继续文案");
  if (open) $("continue-prompt").focus();
});
$("auto-continue").checked = localStorage.getItem("kz-auto-continue") === "1";
renderAutoStatus();
// 存的是旧默认文案时静默升级:否则刹车契约(【阻塞】标记)与提示词对不上,
// 用户自己改过的文案不动。
{
  const stored = (localStorage.getItem("kz-continue-prompt") || "").trim();
  const isLegacyDefault = LEGACY_CONTINUE_PROMPTS.some((old) => old.trim() === stored);
  if (!stored || isLegacyDefault) {
    // 默认态标记为"未自定义":applyCadenceSettings 拿到配置后据此重渲染。
    lastRenderedPrompt = DEFAULT_CONTINUE_PROMPT;
    localStorage.setItem("kz-continue-prompt", DEFAULT_CONTINUE_PROMPT);
    $("continue-prompt").value = DEFAULT_CONTINUE_PROMPT;
    if (isLegacyDefault) log(t("继续文案已升级到新版(含【阻塞】刹车约定)"));
  } else {
    lastRenderedPrompt = null; // 用户自定义过:后续节奏变化不覆盖。
    $("continue-prompt").value = stored;
  }
}
$("continue-prompt").addEventListener("change", () => {
  const value = $("continue-prompt").value.trim();
  localStorage.setItem("kz-continue-prompt", value || DEFAULT_CONTINUE_PROMPT);
  $("continue-prompt").value = value || DEFAULT_CONTINUE_PROMPT;
});
$("auto-max").value = Math.min(100, Math.max(1, Number.parseInt(localStorage.getItem("kz-auto-max"), 10) || DEFAULT_AUTO_CONTINUE_MAX));
// 「本轮后停」是一次性意图,不是偏好:绝不持久化。
// 曾经持久化过——勾一次后 localStorage 永远是 "1",每次启动都重新武装,
// 表现为"鞭挞跑一轮就停,怎么都停不掉"(D-111)。这里顺手清掉存量键。
localStorage.removeItem("kz-auto-stop-round");
$("auto-stop-round").checked = false;
autoStopAfterRound = false;
$("auto-pause").addEventListener("click", () => {
  autoPaused = !autoPaused;
  $("auto-pause").classList.toggle("active", autoPaused);
  $("auto-pause").textContent = autoPaused ? t("继续鞭挞") : t("暂停鞭挞");
  if (autoPaused) cancelAutoContinueTimer();
  // BUG 修复:恢复时如果正处于轮间空闲,必须重新调度,否则鞭挞静默死亡。
  if (!autoPaused && !running && $("auto-continue").checked && autoContinueAllowed()) {
    setStatus(`${t("鞭挞恢复")},2 ${t("秒后继续")}…`, false);
    scheduleAutoContinue();
  }
  log(autoPaused ? t("鞭挞已暂停") : t("鞭挞已恢复"));
});
$("auto-stop-round").addEventListener("change", () => {
  autoStopAfterRound = $("auto-stop-round").checked;
  log(autoStopAfterRound ? t("本轮结束后将停止鞭挞") : t("已取消本轮后停"));
});
$("auto-max").addEventListener("change", () => {
  const max = autoContinueMax();
  $("auto-max").value = max;
  localStorage.setItem("kz-auto-max", String(max));
  renderAutoStatus();
  autoRounds = 0;
  cancelAutoContinueTimer();
  log(`${t("鞭挞上限已设为")} ${max} ${t("轮")}`);
});
$("auto-continue").addEventListener("change", () => {
  if ($("auto-continue").checked && !autoContinueAllowed()) {
    $("auto-continue").checked = false;
    localStorage.setItem("kz-auto-continue", "0");
    autoRounds = 0;
    cancelAutoContinueTimer();
    toast(t("鞭挞仅适用于自主推进模式，请先切换模式"));
    log(t("鞭挞未开启:结伴开发模式不支持自动续跑"));
    return;
  }
  localStorage.setItem("kz-auto-continue", $("auto-continue").checked ? "1" : "0");
  autoRounds = 0;
  if (!$('auto-continue').checked) cancelAutoContinueTimer();
  log($("auto-continue").checked ? `${t("鞭挞已开启:每轮结束自动推进目标")} (${t("轮")} ${autoContinueMax()})` : t("鞭挞已关闭"));
  // BUG 修复(触发):空闲时勾上鞭挞必须立刻抽第一鞭——原来只挂在"上一轮结束"上,
  // 冷启动勾选后永远没有第一轮,必须手点"继续"才动。
  if ($("auto-continue").checked && !running && !autoPaused) {
    setStatus("鞭挞启动,2 秒后开始…", false);
    scheduleAutoContinue();
  }
});
const PROFILE_STORAGE_KEY = "kz-profile";
const savedProfile = localStorage.getItem(PROFILE_STORAGE_KEY);
if (["dev-pair", "dev-auto", "research"].includes(savedProfile)) {
  $("profile-select").value = savedProfile;
}
// 后端只认 dev/research(决定 agent 选择),dev-auto 是前端的鞭挞档位,按进程单独记住,
// 否则切换进程回显时自主推进会被静默降级成结伴开发。
// R-115:这份映射必须落盘。早期只放在内存里,重启后它是空的,回退分支就把模式
// 降级成结伴开发——哪怕 kz-profile 里明明存着自主推进(D-155)。
const PROCESS_PROFILE_KEY = "kz-process-profile";
const processProfileUi = new Map(
  Object.entries(readJson(PROCESS_PROFILE_KEY, {})).filter(([, v]) =>
    ["dev-pair", "dev-auto", "research"].includes(v),
  ),
);
function persistProcessProfiles() {
  writeJson(PROCESS_PROFILE_KEY, Object.fromEntries(processProfileUi));
}

function syncAutoContinueWithProfile() {
  if (autoContinueAllowed() || !$("auto-continue").checked) return;
  $("auto-continue").checked = false;
  localStorage.setItem("kz-auto-continue", "0");
  autoRounds = 0;
  cancelAutoContinueTimer();
  renderAutoStatus();
  log(t("当前模式不支持鞭挞，已自动关闭"));
  toast(t("鞭挞已关闭：当前进程不是自主推进模式"));
}
function applyProfileValue(backendProfile) {
  const remembered = activeProcessId ? processProfileUi.get(activeProcessId) : null;
  // 回退顺序:本进程的记忆 → 全局上次选择 → dev-pair。少了中间这一档,
  // 新进程与重启后的旧进程都会被静默降级成结伴开发。
  const globalChoice = localStorage.getItem(PROFILE_STORAGE_KEY);
  const fallback = ["dev-pair", "dev-auto"].includes(globalChoice) ? globalChoice : "dev-pair";
  if (backendProfile === "research") $("profile-select").value = "research";
  else $("profile-select").value = remembered && remembered !== "research" ? remembered : fallback;
  localStorage.setItem(PROFILE_STORAGE_KEY, $("profile-select").value);
  syncAutoContinueWithProfile();
}
$("profile-select").addEventListener("change", () => {
  localStorage.setItem(PROFILE_STORAGE_KEY, $("profile-select").value);
  if (activeProcessId) {
    processProfileUi.set(activeProcessId, $("profile-select").value);
    persistProcessProfiles();
    const profile = $("profile-select").value === "research" ? "research" : "dev";
    invoke("process_update", { processId: activeProcessId, profile })
      .catch((error) => reportPersistentError(`${t("进程模式保存失败")}:${error}`));
  }
  syncAutoContinueWithProfile();
});
$("work-priority-select").addEventListener("change", async () => {
  const value = selectedWorkPriority();
  localStorage.setItem(workPriorityStorageKey(), value);
  if (!currentProject) return;
  try {
    // 切换 = 写记忆(真源),不是只改本地开关;记忆页随后可把正文改成任意细度。
    workFocusMemory = await invoke("memory_focus_set", {
      projectDir: currentProject,
      title: WORK_FOCUS_PRESETS[value].title,
      body: WORK_FOCUS_PRESETS[value].body,
    });
    log(localizeDynamic(value === "requirement-first" ? "已切换为需求优先" : "已切换为缺陷优先"));
  } catch (err) {
    toastError(`${t("开发重心保存失败")}:${err}`);
  }
});
$("stop").addEventListener("click", () => {
  // 本地立即复位,不依赖后端事件回执(事件通道故障时停止键也必须有效)。
  cancelAutoContinueTimer();
  autoRounds = 0;
  invoke("stop_run", { projectDir: currentProject, processId: activeProcessId }).catch((err) => reportPersistentError(`停止指令失败:${err}`));
  hideAsk();
  stopElapsed();
  setRunning(false, "已停止");
  // R-086:本地复位同样收敛到该会话状态机,不依赖后端事件回执。
  if (activeSessionId) {
    const state = sessionState(activeSessionId);
    state.running = false;
    state.converged = true;
  }
  log(t("已请求停止(本地已复位)"));
});
promptBox.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("file-suggestions").classList.contains("hidden")) {
    e.preventDefault();
    hideFileSuggestions();
    return;
  }
  if ((e.key === "Tab" || e.key === "Enter") && fileSuggestions.length > 0 && !e.ctrlKey && !e.metaKey) {
    e.preventDefault();
    chooseFileSuggestion();
    return;
  }
  if (e.key === "ArrowDown" && (promptBox.selectionStart === promptBox.value.length || promptBox.value === "")) {
    if (fileSuggestions.length > 0) {
      e.preventDefault();
      fileSuggestionIndex = (fileSuggestionIndex + 1) % fileSuggestions.length;
      renderFileSuggestions();
      return;
    }
    if (navigatePromptHistory(1)) e.preventDefault();
  } else if (e.key === "ArrowUp" && (promptBox.selectionStart === 0 || promptBox.value === "")) {
    if (fileSuggestions.length > 0) {
      e.preventDefault();
      fileSuggestionIndex = (fileSuggestionIndex - 1 + fileSuggestions.length) % fileSuggestions.length;
      renderFileSuggestions();
      return;
    }
    if (navigatePromptHistory(-1)) e.preventDefault();
  } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    send();
  } else if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    send();
  }
});

window.addEventListener("keydown", (e) => {
  const modifier = e.ctrlKey || e.metaKey;
  if (!modifier || e.altKey) return;
  if (e.key.toLowerCase() === "k") {
    e.preventDefault();
    promptBox.focus();
    return;
  }
  if (!e.shiftKey) return;
  if (e.key.toLowerCase() === "c") {
    e.preventDefault();
    $("stop").click();
  } else if (e.key.toLowerCase() === "n") {
    e.preventDefault();
    $("new-chat").click();
  }
});

// ---------- 模型直选 ----------
async function loadModels() {
  const select = $("model-select");
  const saved = localStorage.getItem(prefKey("model")) ?? localStorage.getItem("kz-model") ?? "";
  select.innerHTML = "";
  const def = document.createElement("option");
  def.value = "";
  def.textContent = t("模型:agent 默认");
  select.appendChild(def);
  try {
    const models = await invoke("models_list", { projectDir: currentProject });
    const ids = new Set(models.map((m) => m.id));
    for (const m of models) {
      const opt = document.createElement("option");
      opt.value = m.id;
      opt.textContent = m.label;
      if (m.id === saved) opt.selected = true;
      select.appendChild(opt);
    }
    // D-167:探测不到不等于用不了——端点可能没实现 /models,key 也可能还没配好。
    // 手填过的模型要留在列表里,否则下次重开又得再填一遍。
    for (const id of manualModels()) {
      if (ids.has(id)) continue;
      const opt = document.createElement("option");
      opt.value = id;
      opt.textContent = `${id}(手填)`;
      if (id === saved) opt.selected = true;
      select.appendChild(opt);
    }
    const custom = document.createElement("option");
    custom.value = MANUAL_MODEL_SENTINEL;
    custom.textContent = t("＋ 手填模型…");
    select.appendChild(custom);
    log(`模型列表已刷新(${models.length} 个可选)`);
  } catch (err) {
    reportPersistentError(`模型列表获取失败:${err}`);
  }
}

// 手填模型:provider:model 直指。有些 OpenAI 兼容端点不提供 /models,
// 或者 key 尚未配好导致探测为空,这条通道保证配了 provider 就一定能用。
const MANUAL_MODEL_SENTINEL = "__manual__";
function manualModels() {
  const list = readJson(prefKey("manual-models"), []);
  return Array.isArray(list) ? list.filter((x) => typeof x === "string") : [];
}
function addManualModel(id) {
  const list = manualModels();
  if (!list.includes(id)) list.push(id);
  writeJson(prefKey("manual-models"), list);
}
// R-115:模型与思考强度按项目记——不同项目常配不同模型,共用一个全局键会互相打架。
// 思考强度此前只写不读(kz-reasoning 全仓零处 getItem),等于每次重启都回默认档。
function prefKey(name) {
  return `kz-${name}:${currentProject || "default"}`;
}
function restoreProjectPrefs() {
  const reasoning = localStorage.getItem(prefKey("reasoning"));
  const select = $("reasoning-select");
  // 选项不存在时不要硬塞:赋一个无效值会让 select 落到空串,反而清掉配置默认档。
  if (reasoning !== null && [...select.options].some((o) => o.value === reasoning)) {
    select.value = reasoning;
  }
  const delivery = localStorage.getItem("kz-delivery");
  const deliverySelect = $("delivery-select");
  if (delivery && [...deliverySelect.options].some((o) => o.value === delivery)) {
    deliverySelect.value = delivery;
  }
  restoreDocFilters();
}

// 思考强度:空值=用配置默认档,其余为本进程覆盖。
$("reasoning-select").addEventListener("change", () => {
  const value = $("reasoning-select").value;
  localStorage.setItem(prefKey("reasoning"), value);
  if (activeProcessId) {
    invoke("process_update", { processId: activeProcessId, reasoning: value })
      .catch((error) => reportPersistentError(`${t("进程思考强度保存失败")}:${error}`));
  }
});

$("model-select").addEventListener("change", () => {
  const select = $("model-select");
  if (select.value === MANUAL_MODEL_SENTINEL) {
    const input = (window.prompt(t("填 provider:model,例如 deepseek:deepseek-chat")) || "").trim();
    // provider 名必须对得上配置里的键,否则后端 resolve_model 会直接失败。
    if (!/^[\w.-]+:.+$/.test(input)) {
      if (input) toast(t("格式应为 provider:model"));
      select.value = localStorage.getItem(prefKey("model")) || "";
      return;
    }
    addManualModel(input);
    localStorage.setItem(prefKey("model"), input);
    loadModels().then(() => {
      $("model-select").value = input;
    });
    if (activeProcessId) {
      invoke("process_update", { processId: activeProcessId, model: input })
        .catch((error) => reportPersistentError(`${t("进程模型保存失败")}:${error}`));
    }
    return;
  }
  localStorage.setItem(prefKey("model"), select.value);
  if (activeProcessId) {
    // 空串=清除本进程的模型覆盖(回落 agent 默认);传 null 会被后端当作"不修改"。
    invoke("process_update", { processId: activeProcessId, model: $("model-select").value })
      .catch((error) => reportPersistentError(`${t("进程模型保存失败")}:${error}`));
  }
});

