// 并行线路回归护栏：锁住刷新、切换和设置持久化的关键竞态修复。
// 具体 DOM/IPC 行为由 ui-runtime-smoke 覆盖；这里专门防止后续改动把已修复的
// 高频轮询、无序切换或全局 profile 回写重新引入。
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const readUi = (name) => readFile(resolve(root, "crates", "kanzei-app", "ui", name), "utf8");
const [lines, sessions, auto, compose, models, views, docsList, index] = await Promise.all([
  readUi("20-lines.js"),
  readUi("09-sessions.js"),
  readUi("08-auto.js"),
  readUi("08-compose.js"),
  readUi("08-models.js"),
  readUi("15-views-misc.js"),
  readUi("11-docs-list.js"),
  readUi("index.html"),
]);
const composeSources = `${auto}\n${compose}\n${models}`;

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

assert(!lines.includes("setInterval(() =>"), "多线页面不应恢复固定 setInterval 轮询");
assert(lines.includes("LINES_REFRESH_IDLE_MS = 8000"), "多线空闲刷新间隔护栏缺失");
assert(lines.includes("scheduleLinesRefresh(running ? LINES_REFRESH_RUNNING_MS : LINES_REFRESH_IDLE_MS)"), "多线自适应刷新护栏缺失");
assert(!lines.includes("line-lane-initial"), "线路刷新不应重新挂载进入动画 class");
const style = await readUi("style.css");
assert(!/\.line-lane\s*\{[^}]*animation\s*:/.test(style), "线路基础卡片不应在每次刷新时重复播放进入动画");
assert(!style.includes("line-lane-enter"), "线路刷新不应保留会造成闪烁的进入动画");
assert(style.includes(".lines-header > div:first-child"), "线路页标题区域缺少窄区宽度约束");
assert(style.includes("@media (max-width: 1400px)"), "线路页缺少按主区窄态换行规则");
// 不锁死单个文件路径:R-253 把 run.rs 拆成 run/ 目录、停止命令搬去
// commands/run.rs,这里硬编码的 `src/run.rs` 随之 ENOENT,冒烟自 e06a226 起
// 一直红——而那批提交每条都写着「四条前端冒烟全过」(六条只跑了四条,漏的正是
// 本条与 ui-lint)。改成扫整棵 src/ 拼接:断言要的是「这两个契约字符串存在于
// 后端某处」,不是「存在于某个文件名里」,再拆一次也不会断。
async function readAllRustSources(dir) {
  const { readdir } = await import("node:fs/promises");
  let out = "";
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = resolve(dir, entry.name);
    if (entry.isDirectory()) out += await readAllRustSources(full);
    else if (entry.name.endsWith(".rs")) out += await readFile(full, "utf8");
  }
  return out;
}
const run = await readAllRustSources(resolve(root, "crates", "kanzei-app", "src"));
assert(run.includes('"kz:stopped"'), "停止无运行态没有幂等收口事件");
assert(run.includes('"already_idle": true'), "停止无运行态没有标记已空闲结果");
assert(!/open\.addEventListener\([\s\S]{0,220}?await refreshProcesses\(\)/.test(lines), "线路切换前不应额外刷新进程列表");

assert(sessions.includes("processRefreshInFlight"), "进程刷新缺少单飞请求护栏");
assert(sessions.includes("processSwitchGeneration"), "线路切换缺少请求代次护栏");
assert(sessions.includes("function renderParallelTaskStatus(items)"), "左侧线路状态按钮渲染入口缺失");
assert(sessions.includes("function processRunning(item)"), "线路运行态未统一经过状态真源映射");
assert(sessions.includes("state.live_running === true"), "实时运行事件不能被旧 process_list 快照覆盖");
assert(sessions.includes("state.local_start_pending && !item.running"), "发送启动窗口不能被旧空闲快照覆盖");
assert(compose.includes("local_start_pending: true"), "发送启动意图未进入状态投影(R-206:经 transitionSession detail 传入)");
assert((await readUi("01-core.js")).includes("payload?.terminal !== false"), "错误事件缺少终态区分");
assert(sessions.includes("applyAutoUiState(activeProcessId)"), "重载/切项目后未恢复活动线路鞭挞设置");
assert(sessions.includes("applyProfileValue(active?.profile)"), "重载/切项目后未恢复活动线路 profile");
assert(!sessions.includes("process-tabs"), "顶部进程切换条已移除,不应重新引入");
assert(sessions.includes("loadConversation(null, switchGeneration)"), "对话恢复未绑定切换代次");
assert(!sessions.includes("cancelAutoContinueTimer();\n  hideAsk(true)"), "切换线路不应取消原线路自主推进 timer");
assert(sessions.includes('state.phase === "stopping"'), "线路状态投影缺少 stopping 具名态");

assert(compose.includes("const processUpdateQueues = new Map()"), "进程设置缺少按线路保存队列");
assert(composeSources.includes("const autoContinueTimers = new Map()"), "自主推进 timer 未按 session 隔离");
assert(compose.includes("sendAutoToSession(prompt, sessionId)"), "后台自主推进仍按活动线路发送");
assert(compose.includes("const requestSessionId = activeSessionId"), "发送 IPC 未捕获发起线路身份");
assert(compose.includes("transitionSession(targetSessionId, \"stopping\")"), "停止按钮仍缺少 stopping 过渡态");
assert(compose.includes("queueProcessUpdate(activeProcessId"), "模型/profile/reasoning 未走保存队列");
// 切走的线路必须继续被鞭挞:后台线的 kz:done 只走 handleBackgroundSessionDone,活动线
// 那两条在飞释放路径(kz:done/kz:idle handler)它一条都走不到。不在这里释放,下一轮就被
// armAutoContinue 的守卫静默吞掉,线路永久钉在「等待下一轮」。
assert(
  /function handleBackgroundSessionDone\(payload\) \{[\s\S]{0,800}?releaseAutoContinue\(sessionId\)/.test(compose),
  "后台线轮末未释放自主推进在飞标记(切走线路后鞭挞一轮即停)",
);
assert(
  (await readUi("01-core.js")).includes('event === "kz:auto-fail" && typeof handleBackgroundAutoFail'),
  "后台线的失败退避重试仍被路由层整条丢弃(断一次网即永久停摆)",
);
assert(compose.includes("function handleBackgroundAutoFail(payload)"), "后台线失败退避重试缺少专用处理器");
// 线路页按线操控:鞭挞与模型不再只属于「当前打开的那条线」。
// 模型必须按线回显、按线发送。回显只写在 switchProcess 里的话,冷启动/兜底选中活动线
// 都不回显(用户现场只有一条线时永远不触发切换);发送读下拉、鞭挞续跑读 item.model 的话,
// 同一条线会跑在两个模型上。
assert(composeSources.includes("function syncModelSelectToActiveLine()"), "缺少模型下拉按线回显入口");
assert(sessions.includes("syncModelSelectToActiveLine();"), "renderProcesses/switchProcess 未按线回显模型");
assert(compose.includes("function lineModelFor(processId)"), "发送用模型未统一取自线路存档");
assert(!/model: \$\("model-select"\)\.value \|\| null/.test(compose), "发送又回到读下拉显示值(与鞭挞续跑不同源)");
assert(compose.includes("async function setLineAutoState(processId, patch)"), "缺少按线鞭挞写入口(线路页要能操控任意线)");
assert(compose.includes("function lineAutoConfig(processId)"), "缺少按线鞭挞读入口");
assert(lines.includes("buildLineAutoControls(line)"), "线路页每条线缺少鞭挞控件");
assert(lines.includes("buildLineModelSelect(item)"), "线路页每条线缺少模型选择");
assert(lines.includes("queueProcessUpdate(item.id, { model: value })"), "线路页改模型未走按线保存队列");
assert(sessions.includes("worktreeLineCreateInFlight"), "并行线路创建缺少单飞请求护栏");
assert(sessions.includes("worktreeLineCreateSequence"), "并行线路创建缺少进程内唯一序号");
assert(sessions.includes('const addButtons = [$("worktree-add"), $("lines-add")].filter(Boolean)'), "并行线路两个创建入口没有共用忙碌态");
assert(sessions.includes('button.setAttribute("aria-busy", "true")'), "并行线路创建请求期间没有暴露忙碌态");
assert(sessions.includes('button.removeAttribute("aria-busy")'), "并行线路创建结束后忙碌态没有恢复");
assert(sessions.includes('linesAddLabel.textContent = t("创建中…")'), "线路页没有创建中反馈");
assert(sessions.includes("Date.now()"), "并行线路名称没有使用毫秒级时间戳");
assert(index.includes('id="lines-work-item"'), "并行视图缺少开线条目选择器");
assert(lines.includes('function renderLineWorkItemOptions(snapshot = null)'), "开线条目选择器缺少 tracker 快照投影");
assert(sessions.includes('...(workItemId ? { workItemId } : {})'), "process_create 没有携带用户选择的条目 ID");
assert(sessions.includes('trackerWrites: false'), "开线绑定不得顺带开放通用 tracker 写权限");
assert(docsList.includes('entry.claimed_by'), "backlog 被取得标记没有读取 tracker claimed_by");
assert(!docsList.includes('String(candidate.claim ?? "").match'), "backlog 又回退到解析线路 prompt/claim 文本");
const collaboration = await readFile(resolve(root, "crates", "kanzei-app", "src", "collaboration.rs"), "utf8");
assert(!collaboration.includes("claim_from_prompt"), "协作快照仍保留 prompt 头猜测取得条目的路径");
assert(collaboration.includes("active_claims_by_line"), "协作快照没有接入 tracker 取得线真源");
const profileFunction = compose.match(/function applyProfileValue\(backendProfile\) \{([\s\S]*?)\n\}/)?.[1] ?? "";
assert(profileFunction && !profileFunction.includes("localStorage.setItem(PROFILE_STORAGE_KEY"), "切换线路不能回写全局 profile");

assert(views.includes("switchGeneration = null"), "对话恢复缺少可选切换代次参数");
assert(views.includes("if (!isCurrent()) return;"), "旧线路对话响应缺少丢弃护栏");

const core = await readUi("01-core.js");
assert(core.includes("handleBackgroundSessionDone(eventPayload.payload)"), "后台 done 事件副作用仍被路由层截断");
assert(lines.includes("harvest.disabled = lineRunning"), "运行中线路仍可进入收活");
assert(lines.includes('close.textContent = t("关闭线路")'), "线路页缺少关闭线路入口");
assert(sessions.includes("async function closeParallelProcess(processId)"), "左侧线路状态缺少统一关闭流程");
assert(sessions.includes('await invoke("process_close", { processId })'), "关闭线路没有调用后端 process_close");
assert(sessions.includes("if (!item.id.startsWith(\"d|\"))"), "关闭入口没有排除默认主线路");
assert(lines.includes('invoke("worktree_harvest_candidates"'), "收活没有读取线路对话中的 tracker 候选");
assert(lines.includes("请选择本次交付条目"), "多条 tracker 候选缺少人工选择入口");
// D-305 的性质不变:不得存在绕过收活六格的直接合并入口。侧栏工作树已降级为只读呈现,
// 收活/放弃迁到并行线路页的工作树操作台——入口还在,只是换了地方,所以这两条改成
// 「合并按钮哪儿都不许有」+「收活入口仍然存在(现在由 renderLinesWorktrees 提供)」。
assert(!sessions.includes('"worktree-merge"'), "不应保留绕过收活六格的直接合并按钮");
assert(sessions.includes('"worktree-harvest"'), "工作树缺少统一收活入口");
assert(
  sessions.includes("function renderLinesWorktrees"),
  "并行线路页缺少工作树操作台(侧栏只读之后,孤儿树就没有出口了)",
);
assert(
  /renderWorktrees[\s\S]{0,900}?\n}/.test(sessions)
    && !/renderWorktrees[\s\S]{0,900}?createElement\("button"\)/.test(sessions),
  "侧栏 renderWorktrees 不得再造按钮(操作全在并行线路页)",
);

console.log("并行线路回归护栏通过:刷新节流、左侧线路状态/切换代次、设置串行保存和 profile 隔离");
