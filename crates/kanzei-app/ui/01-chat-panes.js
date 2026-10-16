// Conversation DOM ownership; presentation state is scoped to its session pane.
import { $ } from "./00-dom.js";
import { sessionState } from "./00-session-state.js";
import { activeSessionId, currentAssistant, currentReasoning, setCurrentAssistant, setCurrentReasoning } from "./03-shell.js";
import { currentReasoningHead, setCurrentReasoningHead, followLatest, noteProgrammaticScroll, scrollBottom } from "./05-chat-render.js";
import { renderTrimmedHint, sessionLiveNow } from "./15-views-misc.js";
// R-267:`messages` 只是**滚动容器**;消息本体挂在它下面的 per-session pane 里。
// 这个分工是整个改造能低风险落地的关键——滚动/跟随/复制那几处照旧读 `messages`,
// 一行不用改;只有「往哪儿追加」换成 activePane。
export const messages = $("messages");

// ---------- R-267:每会话渲染面 ----------
//
// 改造前:全局唯一容器 + 切走时存一份 innerHTML 字符串(sessionDomCache,上限 30)。
// 于是非活动会话的 kz:text/kz:tool-*/kz:reasoning 只能整条丢掉(渲染进去就串线),
// 切回来把字符串塞回去,中间那段是缺的,只好挂一句「快照截至上次切走时」等轮末补齐。
// 代价还不止缺口:两条运行中的线来回切,每次都是一次多 MB 字符串的 innerHTML 解析。
//
// 改造后:每个会话一个 pane,事件永远渲染进**自己那个** pane,切线只是换显示。
// 缺口消失、切换零重渲染、sessionDomCache 整个不需要了。
export const messagePanes = new Map();
/// pane 保活上限。超出后淘汰最久未访问的(仅**非运行中**会话),下次切回按
/// loadConversation 重建——退化成改造前的行为,而不是退回「有缺口」。
/// 上限取小是因为消息本体没有窗口化(批2 前一个会话可到 993 条/1665 个 part),
/// 多个长会话的 pane 常驻会把内存放大。批2 落地后可以调大。
export const MESSAGE_PANE_MAX = 4;
export let activePane = messages.querySelector(".msg-pane");
export function setActivePane(value) { activePane = value; }

/// 取(必要时新建)某个会话的 pane。新建的 pane **默认隐藏**:只有 showPane 能让
/// 一个 pane 可见。后台会话(典型是开着鞭挞的自主推进线)第一次渲染、或它的 pane
/// 被淘汰后重建时走的是 withSessionRender → paneFor,原先新建即可见,于是它的输出
/// 和活动会话叠在同一个视图里,新对话也清不掉(清的只是 activePane)。
/// `forDisplay` 只给 showPane 用:只有「要显示它」时才认领启动期的占位 pane,
/// 后台渲染不得抢走首屏那一块。
export function paneFor(sessionId, { forDisplay = false } = {}) {
  const key = sessionId || "";
  let pane = messagePanes.get(key);
  if (!pane) {
    // 启动期那个 data-session-id="" 的占位 pane 直接认领给第一个要显示的会话,
    // 免得首屏白闪一下再换。
    const placeholder = forDisplay ? messages.querySelector('.msg-pane[data-session-id=""]') : null;
    pane = placeholder && !messagePanes.has("") ? placeholder : document.createElement("div");
    pane.className = "msg-pane";
    if (!forDisplay) pane.classList.add("hidden");
    pane.dataset.sessionId = key;
    if (!pane.parentNode) messages.appendChild(pane);
    messagePanes.set(key, pane);
  }
  pane.dataset.usedAt = String(Date.now());
  return pane;
}

/// 切到某个会话的 pane:隐藏其余全部、显示这一个。**不重建 DOM**。
/// 不变式:同一时刻 #messages 下只有一个 pane 可见。只隐藏「上一个 activePane」不够——
/// 越界可见的后台 pane、没被认领的占位 pane 都不是上一个 activePane。
/// 返回 true = 该 pane 已有内容(切回来即见),false = 新建的空 pane(调用方需要装历史)。
export function showPane(sessionId) {
  const pane = paneFor(sessionId, { forDisplay: true });
  if (activePane !== pane) {
    // switchProcess 已先改 activeSessionId;流式指针仍属于离开的 pane。
    const outgoingId = activePane?.dataset.sessionId;
    if (messagePanes.get(outgoingId) === activePane) {
      const outgoing = streamStateFor(outgoingId);
      outgoing.assistant = currentAssistant;
      outgoing.reasoning = currentReasoning;
      outgoing.reasoningHead = currentReasoningHead;
    }
    const incoming = streamStateFor(sessionId);
    setCurrentAssistant(incoming.assistant);
    setCurrentReasoning(incoming.reasoning);
    setCurrentReasoningHead(incoming.reasoningHead);
  }
  for (const other of [...messages.children]) {
    if (other === pane || !other.classList.contains("msg-pane")) continue;
    other.classList.add("hidden");
    delete other.dataset.active;
  }
  pane.classList.remove("hidden");
  // 当前显示的 pane 打属性标记:隐藏的 pane 仍在 DOM 里,断言/查询要能只看这一个。
  pane.dataset.active = "1";
  activePane = pane;
  evictStalePanes();
  // 对话搜索条等「按当前 pane 算」的东西要等 activePane 真换了才能重算:kz:conversation-selected 发在 loadConversation 之前,
  // 那一刻 activePane 还是旧的。
  document.dispatchEvent(new CustomEvent("kz:pane-shown", { detail: { sessionId } }));
  return pane.dataset.hasContent === "1";
}

/// 丢弃某个**非活动**会话的 pane 与流式装配状态,下次进来按 loadConversation 重建。
/// 用于「那条线的内容已作废」(新对话开了新段、段被删除)而它此刻不在前台的情形。
export function discardSessionPane(sessionId) {
  const key = sessionId || "";
  const pane = messagePanes.get(key);
  if (pane && pane !== activePane) {
    pane.remove();
    messagePanes.delete(key);
  }
  dropSessionStream(key);
}

/// D-202 家族的真正机制:恢复历史走 PANE_WINDOW_SIZE 窗口化,但**实时追加从不裁剪**。
/// 一次自主推进跑几百轮,pane 会无上界地长下去,而每次 appendToPane 之后的
/// scrollBottom 都要对整棵树强制一次布局——单次追加的代价随 DOM 线性增长,整段
/// 会话就是 O(n²)。
///
/// 实测(playwright,真实渲染路径,每批 200 条):
///   pane 顶层 200 → 追加 200 条耗时 36ms;800 → 151ms;1600 → 305ms;2400 → 476ms。
/// 即单条追加在 2400 节点时已经要 ~2.4ms 布局;上万节点时主线程基本被追加占满,
/// 表现就是「侧栏点了没反应」「跑着跑着卡住」。
///
/// 这里给实时面加上界:超过 PANE_LIVE_MAX 就从**头部**砍到 PANE_LIVE_KEEP。被砍掉的
/// 只是本地视图,消息本身在后端对话历史里,切走再切回按 loadConversation 重建。
export const PANE_LIVE_MAX = 600;
export const PANE_LIVE_KEEP = 400;

/// 往当前 pane 追加消息节点。顺手打上「有内容」标志——切回时靠它判断是否需要装载,
/// 而不是去数子节点或找 .empty-state:空状态本身也是子节点,而冒烟的假 DOM 对
/// 类选择器支持有限,判空一旦不准就会把「已有完整内容」误判成「空 pane」再重建一遍。
export function appendToPane(node) {
  activePane.dataset.hasContent = "1";
  activePane.appendChild(node);
  trimLivePane(activePane);
}

/// 从头部裁掉超出上界的部分,并把累计裁掉的条数记在 pane 上(供顶部提示条显示)。
/// 只在**超过** MAX 时动手并一次砍到 KEEP,避免每追加一条就删一条的抖动。
/// 用户翻上去读历史时,允许暂时超出上界多少倍。上界本身是为了让主线程不被布局
/// 吃掉;而**正在读的人**比那点布局更重要,所以读历史期间不裁,等他滚回底部再补。
/// 但也不能无限让步——挂在半空的会话照样会把内存和布局拖垮,所以给一个硬顶。
export const PANE_LIVE_HARD_MAX = PANE_LIVE_MAX * 3;

export function trimLivePane(pane) {
  if (!pane || typeof pane.children?.length !== "number") return;
  // renderMessagesInto 会把 activePane 临时换成一个**游离的 holder** 再渲染
  // (15-views-misc.js),补齐一窗历史时那个 holder 可能有上千个节点。在游离节点上
  // 裁剪等于把还没 prepend 进页面的消息直接删掉,而调用方随后只搬 childNodes,
  // 被删的那批再也回不来。没挂进文档的容器一律不裁。
  if (pane.isConnected === false) return;
  // UI2-0926 #12:连续工具调用合成一个 .tool-group 顶层节点(最多 30 行),按子节点数已不代表行数——
  // 鞭挞长跑几百次调用、没有正文时只有十几个子节点,永远裁不掉。按「单位」计权:工具组按行数计,
  // 其余节点各计 1;阈值、裁剪循环与 droppedLive 都用单位。
  const unitsOf = (el) => el?.classList?.contains("tool-group") ? Math.max(1, Number(el.dataset?.count) || 1) : 1;
  // 快路:子节点全是满组也超不过上限时不必逐个数。
  if (pane.children.length * 30 <= PANE_LIVE_MAX) return;
  let units = 0;
  for (const el of pane.children) units += unitsOf(el);
  if (units <= PANE_LIVE_MAX) return;
  // 看得见的那个 pane 才有「阅读位置」可言;后台 pane 随便裁。
  const visible = pane === activePane;
  const reading = visible && typeof followLatest !== "undefined" && !followLatest;
  if (reading && units <= PANE_LIVE_HARD_MAX) return;
  // 从**头部**删节点会把视口下的内容整体前移。浏览器的 scroll anchoring 只在锚点
  // 节点自己没被删时才兜得住,而「翻上去读刚才那段」恰好落在删除区里:实测 scrollTop
  // 数值一动不动,画面却无声跳过 200 条,正在读的那条已从 DOM 消失。
  // loadEarlierMessages 早就用了正确做法(按高度差回补 scrollTop),裁剪这条路上漏了。
  // 两个取样都必须在**删除之前**。删头部节点时浏览器会先把 scrollTop 夹到新的上限,
  // 删完再读到的已经是夹紧后的值——拿它再减一次高度差,等于同一个 delta 扣两遍,
  // 落点直接溢出到 0。实测:纯流式、用户零操作,第 601 条触发裁剪就被甩到离底
  // 7635px 且 followLatest 永久翻成 false(于是 pane 进入「读历史」态、裁剪推迟到
  // 硬顶,反噬性能目标本身)。落点为 0 还会顺手触发触顶补齐,凭空塞进一窗更早的历史。
  const heightBefore = visible ? messages.scrollHeight : 0;
  const topBefore = visible ? messages.scrollTop : 0;
  let dropped = Number(pane.dataset.droppedLive || 0);
  const isHint = (el) =>
    el?.classList?.contains("earlier-hint") || el?.classList?.contains("pane-trimmed-hint");
  // 只用 children 索引寻址:firstElementChild/nextElementSibling 在冒烟的假 DOM 里
  // 不存在,用它们这段逻辑在测试里会静默变成空操作(改了但没人验)。
  while (units > PANE_LIVE_KEEP) {
    // 两条顶部提示条自己不算消息,跳过它们再取受害者(否则每次裁剪都把提示删掉再建一个,
    // 或者把「载入更早的消息」那个真入口误删)。它们只会被 prepend,所以至多在最前两位。
    const kids = pane.children;
    let index = 0;
    while (index < kids.length && index < 3 && isHint(kids[index])) index += 1;
    const victim = kids[index];
    if (!victim) break;
    const weight = unitsOf(victim);
    victim.remove();
    units -= weight;
    dropped += weight;
  }
  pane.dataset.droppedLive = String(dropped);
  // 回补只在**用户正在读历史**时才是对的语义(硬顶强裁那一支)。跟随态下用户要的
  // 不是「保住看的位置」而是「一直贴着底」,此时算位置反而会把自己算离底——而且
  // 这次赋值产生的 scroll 事件会被当成人为滚动,把 followLatest 关掉,从此再也回不来。
  // 跟随态直接交给 scrollBottom 重新钉底,并把落点登记成程序滚动。
  if (visible && heightBefore) {
    if (reading) {
      messages.scrollTop = topBefore - (heightBefore - messages.scrollHeight);
    }
    // 删头部节点时浏览器会**自己**把 scrollTop 夹到新上限,那一下同样会发 scroll 事件。
    // 不先把落点登记成程序滚动,它就会被当成「用户往上滚了」——实测跟随态下 601 条触发
    // 裁剪后 followLatest 立刻翻成 false,此后 flushScrollBottom 不再钉底,永远回不到最新。
    if (typeof noteProgrammaticScroll === "function") noteProgrammaticScroll();
    // 跟随态下用户要的是「一直贴着底」,不是保住某个位置:交给 scrollBottom 重新钉底。
    if (!reading && typeof scrollBottom === "function") scrollBottom(true);
  }
  if (typeof renderTrimmedHint === "function") renderTrimmedHint(pane);
}
/// 清空当前 pane 并复位「有内容」标志。
export function resetPane() {
  activePane.replaceChildren();
  delete activePane.dataset.hasContent;
  delete activePane.dataset.droppedLive;
  if (typeof renderTrimmedHint === "function") renderTrimmedHint(activePane);
}

/// 超出上限时淘汰最久未访问的 pane。**运行中的会话永不淘汰**——它正在往里写,
/// 淘汰了就等于把缺口又造回来。
export function evictStalePanes() {
  if (messagePanes.size <= MESSAGE_PANE_MAX) return;
  const candidates = [...messagePanes.entries()]
    .filter(([id, pane]) => pane !== activePane && !(typeof sessionLiveNow === "function" && sessionLiveNow(id)))
    .sort((a, b) => Number(a[1].dataset.usedAt || 0) - Number(b[1].dataset.usedAt || 0));
  for (const [id, pane] of candidates) {
    if (messagePanes.size <= MESSAGE_PANE_MAX) break;
    pane.remove();
    messagePanes.delete(id);
    dropSessionStream(id);
  }
}

// ---------- R-267:每会话的流式装配状态 ----------
//
// currentAssistant / currentReasoning / currentReasoningHead 是「正在拼接的那条
// 消息/思考块」,原本是模块级单例(全局共 38 处读写)。后台会话也要渲染之后,
// 它们必须按会话分开,否则两条线的文本增量会拼进同一个气泡。
//
// **不改那 38 处读写**:改成在渲染前把目标会话的状态**存进**全局变量、渲染后
// **取回**。渲染函数照旧读写全局,语义不变;代价只是一次存取。这比把 sessionId
// 一路穿进 addMessage/appendAssistant/chatToolStart 要小一个数量级,也不会在
// 调用链上留下几十个「这个 id 是哪来的」。
export const sessionStreams = new Map();
export function streamStateFor(sessionId) {
  const key = sessionId || "";
  let state = sessionStreams.get(key);
  if (!state) {
    state = { assistant: null, reasoning: null, reasoningHead: null };
    sessionStreams.set(key, state);
  }
  return state;
}
export function dropSessionStream(sessionId) {
  sessionStreams.delete(sessionId || "");
}

/// 在 `sessionId` 的渲染上下文里执行 `fn`:pane 与流式状态都临时切过去,结束后还原。
/// 活动会话直接跑(省掉一次存取),后台会话走存取路径。
/// 后台渲染进行中。事件 handler 里混着**全局 UI**副作用(状态栏文案、首响应计时),
/// 那些只属于活动会话——后台会话触发它们就会把别人的状态盖在当前线上。
/// 用一个标志让那几处自我屏蔽,比把 sessionId 穿进每个 handler 便宜得多。
export let renderingBackground = false;
export function withSessionRender(sessionId, fn) {
  if (!sessionId || sessionId === activeSessionId) return fn();
  const state = streamStateFor(sessionId);
  const savedPane = activePane;
  const savedAssistant = currentAssistant;
  const savedReasoning = currentReasoning;
  const savedHead = currentReasoningHead;
  const savedBackground = renderingBackground;
  renderingBackground = true;
  activePane = paneFor(sessionId);
  setCurrentAssistant(state.assistant);
  setCurrentReasoning(state.reasoning);
  setCurrentReasoningHead(state.reasoningHead);
  try {
    return fn();
  } finally {
    state.assistant = currentAssistant;
    state.reasoning = currentReasoning;
    state.reasoningHead = currentReasoningHead;
    renderingBackground = savedBackground;
    activePane = savedPane;
    setCurrentAssistant(savedAssistant);
    setCurrentReasoning(savedReasoning);
    setCurrentReasoningHead(savedHead);
  }
}
export const promptBox = $("prompt");