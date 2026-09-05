import { defer } from "./01-core.js";
import { setCurrentAssistant, setCurrentReasoning } from "./03-shell.js";
import { setCurrentReasoningHead } from "./05-chat-render.js";
import { setCtxTokens } from "./03-shell.js";
import { setActivePane } from "./01-core.js";
import { escapeHtml } from "./04-markdown.js";
import {
  $,
  activePane,
  appendToPane,
  confirmDialog,
  invoke,
  messages,
  resetPane,
  showPane,
} from "./01-core.js";
import { t } from "./02-i18n.js";
import {
  activeProcessId,
  activeSessionId,
  ctxTokens,
  currentAssistant,
  currentProject,
  currentReasoning,
  log,
  processItems,
  renderTokens,
  runControlPending,
  running,
  sessionState,
  setStatus,
  toast,
  toastError,
} from "./03-shell.js";
import { renderMarkdown } from "./04-markdown.js";
import {
  addMessage,
  buildReasoningBlock,
  buildToolBlock,
  currentReasoningHead,
  fillToolBlock,
  followLatest,
  renderReasoningBlock,
  setFollowLatest,
  scrollBottom,
} from "./05-chat-render.js";
import { bgClear, renderRecoveredTraces } from "./06-activity.js";
import { addSummaryEntry } from "./07-events.js";
import { cancelAutoContinueTimer } from "./08-auto.js";
import { processRunning, processSwitchGeneration, refreshProcesses, switchProcess } from "./09-sessions.js";
import { refreshDocs } from "./14-docs-actions.js";
import { forProject } from "./20-lines.js";
import { active_space, create_workspace_process, project_workspace } from "./03-workspaces.js";

// ---------- R-053 快速记录:独立子代理结构化落库(需求/缺陷通用),不打断主对话 ----------
export function quickCaptureForm(kind, sectionId, noun) {
  const section = $(sectionId);
  const title = section.querySelector(".section-title");
  // 需求与缺陷两种快记现在共用「当前在做」这一个分区标题。旧守卫只看"有没有表单",
  // 于是正在写需求时点「记缺陷」会静默无反应——能力被挡住却不说破,与 D-166/D-210
  // 同一种病。同类再点是幂等(保留已写内容),换一类就换掉表单。
  const opened = title.querySelector(".quickreq-form");
  if (opened) {
    if (opened.dataset.kind === kind) return;
    opened.remove();
  }
  const form = document.createElement("div");
  form.className = "idea-add-form quickreq-form";
  form.dataset.kind = kind;
  const input = document.createElement("textarea");
  input.rows = 3;
  input.placeholder = `${t("自然语言描述")}${t(noun)};Ctrl+Enter 或点${t("提交")},Esc ${t("取消")}。${t("独立子代理后台进行")},不打断当前对话。`;
  const submit = async () => {
    const text = input.value.trim();
    if (!text) {
      toast(t("先写点描述"));
      return;
    }
    // 失败时表单必须还在:提交前销毁会让用户写的描述无处可寻。
    submitBtn.disabled = true;
    cancelBtn.disabled = true;
    input.disabled = true;
    toast(`${t("记录中")}${t(noun)}…(${t("独立子代理后台进行")})`);
    try {
      const msg = await invoke("quick_req", { projectDir: currentProject, description: text, kind });
      form.remove();
      toast(`${t("已记录")}:${msg}`);
      refreshDocs();
    } catch (err) {
      submitBtn.disabled = false;
      cancelBtn.disabled = false;
      input.disabled = false;
      toastError(`${t("记录失败(内容已保留,可重试)")}:${err}`);
    }
  };
  input.addEventListener("keydown", (e) => {
    if (e.key === "Escape") form.remove();
    else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) submit();
  });
  const bar = document.createElement("div");
  bar.className = "quickreq-bar";
  const cancelBtn = document.createElement("button");
  cancelBtn.className = "ghost mini";
  cancelBtn.textContent = t("取消");
  cancelBtn.addEventListener("click", () => form.remove());
  const submitBtn = document.createElement("button");
  submitBtn.className = "primary mini";
  submitBtn.textContent = t("提交");
  submitBtn.addEventListener("click", submit);
  bar.append(cancelBtn, submitBtn);
  form.append(input, bar);
  title.append(form);
  input.focus();
}
defer(() => {
  $("req-quick").addEventListener("click", () => quickCaptureForm("req", "focus-section", "需求"));
});
defer(() => {
  $("defect-quick").addEventListener("click", () => quickCaptureForm("defect", "focus-section", "缺陷"));
});

export function renderConventions(conv) {
  const el = $("conv-list");
  el.innerHTML = "";
  if (!conv || !conv.exists) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = t("未创建,点 ＋ 生成模板;agent 会自动遵守此文件");
    el.appendChild(empty);
    return;
  }
  // 规范不再铺开章节列表占满侧边栏:一行入口,点开进应用内 MD 查看器。
  const item = document.createElement("button");
  item.type = "button";
  item.className = "doc-item conv-entry";
  item.setAttribute("aria-label", `${t("打开开发规范")}，${conv.headings.length}${t("个章节")}`);
  item.textContent = `${conv.headings.length}${t("个章节")} · ${t("点击查看")}`;
  item.title = conv.headings.slice(0, 12).join("\n");
  item.addEventListener("click", () => openDocViewer("conventions"));
  el.appendChild(item);
}

// 新建想法:内联输入(webview 无 window.prompt)。录入不过模型,原样收下(R-252 ②)。
defer(() => {
  $("idea-add").addEventListener("click", () => {
    const list = $("idea-list");
    if (list.querySelector(".idea-add-form")) return;
    const form = document.createElement("div");
    form.className = "idea-add-form";
    const input = document.createElement("input");
    input.placeholder = t("想法描述,回车创建(Esc 取消)");
    input.addEventListener("keydown", async (e) => {
      if (e.key === "Escape") {
        form.remove();
        return;
      }
      if (e.key !== "Enter" || !input.value.trim()) return;
      try {
        const msg = await invoke("docs_update", {
          projectDir: currentProject,
          kind: "idea",
          action: "add",
          id: "",
          title: input.value.trim(),
        });
        log(msg);
        form.remove();
        refreshDocs();
      } catch (err) {
        toastError(String(err));
      }
    });
    form.appendChild(input);
    list.prepend(form);
    input.focus();
  });
});

defer(() => {
  $("conv-init").addEventListener("click", async () => {
    try {
      const path = await invoke("conventions_init", { projectDir: currentProject });
      toast(`${t("规范文件已就绪")}:${path}`);
      refreshDocs();
    } catch (err) {
      toastError(String(err), { retry: () => $("conv-init").click() });
    }
  });
});
defer(() => {
  $("conv-open").addEventListener("click", () => openDocViewer("conventions"));
});

// ---------- 应用内文档查看器:markdown/代码直接渲染,外部打开是兜底 ----------
export let viewerKind = null;
export function openRuntimeMarkdown(title, content) {
  viewerKind = null;
  $("viewer-title").textContent = title;
  const body = $("viewer-body");
  body.className = "md";
  body.innerHTML = renderMarkdown(content ?? "");
  body.scrollTop = 0;
  $("viewer-external").classList.add("hidden");
  $("viewer-overlay").classList.remove("hidden");
  $("viewer-close").focus();
}
export async function openDocViewer(kind) {
  try {
    const doc = await invoke("docs_read", { projectDir: currentProject, kind });
    viewerKind = kind;
    $("viewer-external").classList.remove("hidden");
    $("viewer-title").textContent = doc.name;
    const body = $("viewer-body");
    if (doc.name.endsWith(".md")) {
      body.className = "md";
      body.innerHTML = renderMarkdown(doc.content);
    } else {
      body.className = "";
      body.innerHTML = `<pre class="code">${escapeHtml(doc.content)}</pre>`;
    }
    body.scrollTop = 0;
    $("viewer-overlay").classList.remove("hidden");
    $("viewer-close").focus();
  } catch (err) {
    toastError(String(err), { retry: () => openDocViewer(kind) });
  }
}
defer(() => {
  $("viewer-close").addEventListener("click", () => $("viewer-overlay").classList.add("hidden"));
});
defer(() => {
  $("viewer-overlay").addEventListener("click", (e) => {
    if (e.target === $("viewer-overlay")) $("viewer-overlay").classList.add("hidden");
  });
});
defer(() => {
  $("viewer-external").addEventListener("click", () => {
    if (viewerKind) invoke("docs_open", { projectDir: currentProject, kind: viewerKind }).catch((e) => toastError(String(e), { retry: () => $("viewer-external").click() }));
  });
});

// ---------- git 状态 ----------
// 输入框上方的「本轮改动」条。数据取 git 真源(git_status 的 numstat),不靠把
// kz:tool-end 的 diff 事件在前端累加——事件累加会漏掉手工改动、漏掉 agent 用 bash
// 改的文件,而且切会话/重连后归零,给出的数字与工作树对不上。
export let changeBarOpen = false;
export function renderChangeBar(status) {
  const bar = $("change-bar");
  if (!bar) return;
  const files = status?.files ?? [];
  const additions = status?.additions ?? 0;
  const deletions = status?.deletions ?? 0;
  // 没有改动就整条收起来:它是「这一轮把工作树改成了什么样」的答案,没答案不占位置。
  if (!files.length) {
    bar.classList.add("hidden");
    return;
  }
  bar.classList.remove("hidden");
  $("change-bar-repo").textContent = `${files.length} ${t("个文件")}`;
  $("change-bar-branch").textContent = status?.branch ? `⎇ ${status.branch}` : "";
  $("change-bar-add").textContent = `+${additions}`;
  $("change-bar-del").textContent = `−${deletions}`;
  const box = $("change-bar-files");
  box.classList.toggle("hidden", !changeBarOpen);
  $("change-bar-toggle").setAttribute("aria-expanded", String(changeBarOpen));
  const caret = $("change-bar-toggle").querySelector(".change-bar-caret");
  if (caret) caret.textContent = changeBarOpen ? "▾" : "▸";
  if (!changeBarOpen) return;
  box.replaceChildren();
  for (const file of files) {
    const row = document.createElement("div");
    row.className = "change-file";
    const path = document.createElement("span");
    path.className = "change-file-path";
    path.textContent = file.path;
    path.title = file.path;
    row.appendChild(path);
    if (file.untracked) {
      const tag = document.createElement("span");
      tag.className = "change-file-tag dim";
      tag.textContent = t("未跟踪");
      row.appendChild(tag);
    } else if (file.binary) {
      const tag = document.createElement("span");
      tag.className = "change-file-tag dim";
      tag.textContent = t("二进制");
      row.appendChild(tag);
    } else {
      const add = document.createElement("span");
      add.className = "change-add";
      add.textContent = `+${file.additions}`;
      const del = document.createElement("span");
      del.className = "change-del";
      del.textContent = `−${file.deletions}`;
      row.append(add, del);
    }
    box.appendChild(row);
  }
}
defer(() => {
  $("change-bar-toggle")?.addEventListener("click", () => {
    changeBarOpen = !changeBarOpen;
    void refreshGit();
  });
});

export let gitRefreshGeneration = 0;
export async function refreshGit(processId = activeProcessId) {
  if (!currentProject) return;
  const forProject = currentProject;
  const forProcessId = processId || null;
  const target = processItems.find((item) => item.id === forProcessId);
  const worktreePath = target?.worktree_path || null;
  const generation = ++gitRefreshGeneration;
  try {
    const g = await invoke("git_status", {
      projectDir: forProject,
      worktreePath,
    });
    // Git 查询是异步的;切线或切项目后,旧线路的迟到结果不得覆盖当前线路。
    if (
      currentProject !== forProject
      || activeProcessId !== forProcessId
      || generation !== gitRefreshGeneration
    ) return;
    $("status-git").textContent = g.branch
      ? `⎇ ${g.branch}${g.changes ? ` +${g.changes}` : ""}`
      : "";
    $("status-git").title = g.last ? `${t("最近提交")}:${g.last}` : "";
    renderChangeBar(g);
  } catch {
    if (
      currentProject !== forProject
      || activeProcessId !== forProcessId
      || generation !== gitRefreshGeneration
    ) return;
    $("status-git").textContent = "";
    renderChangeBar(null);
  }
}

// 运行中改文件/跑命令后刷新工作区徽章,合并 600ms 内的连续变更。
export let gitLiveTimer = null;
export function refreshGitSoon() {
  clearTimeout(gitLiveTimer);
  gitLiveTimer = setTimeout(() => {
    gitLiveTimer = null;
    refreshGit();
  }, 600);
}

// R-267 批2:消息窗口化。
//
// 恢复历史时**只渲染尾部一窗**,其余留在内存里,向上滚到顶再按窗补齐。
// 两个理由缺一不可:
//   - 长会话的全量渲染本身就贵(实测主会话 993 条消息 / 1665 个 part,其中 272 处
//     要走 renderMarkdown),切一次线卡一次;
//   - 批1 之后 pane 常驻,多个长会话叠起来的 DOM 是新的内存来源——不窗口化的话,
//     批1 省下的重渲染会换成常驻内存,拆东墙补西墙。
export const PANE_WINDOW_SIZE = 120;
/// 每条会话的完整历史与「已渲染到哪」:sessionId → { items, rendered }。
/// 存的是数据不是 DOM,长会话的未渲染部分只占它自己那点 JSON。
export const paneHistory = new Map();

/// 把 `items` 渲染进 `container`。复用同一套配对/思考块/markdown 逻辑——
/// 窗口化不能有第二份渲染实现,否则「首屏」与「补齐」两段迟早长歪。
export function renderMessagesInto(container, items) {
  const savedPane = activePane;
  setActivePane(container);
  try {
    renderMessageParts(items);
  } finally {
    setActivePane(savedPane);
  }
}

/// 向上补齐一窗。保持滚动位置:前插会把内容顶下去,按高度差回补 scrollTop,
/// 否则用户每次触顶都会被弹到别处。
export function loadEarlierMessages() {
  const history = paneHistory.get(activeSessionId || "");
  if (!history) return false;
  const remaining = history.items.length - history.rendered;
  if (remaining <= 0) return false;
  const start = Math.max(0, remaining - PANE_WINDOW_SIZE);
  const chunk = history.items.slice(start, remaining);
  const holder = document.createElement("div");
  renderMessagesInto(holder, chunk);
  const before = messages.scrollHeight;
  activePane.prepend(...[...holder.childNodes]);
  history.rendered += chunk.length;
  messages.scrollTop += messages.scrollHeight - before;
  // 这里**不能**去冲抵 droppedLive。补进来的 chunk 取自 history.items 里
  // rendered 之前、从未渲染过的段;而 droppedLive 记的是已被 trimLivePane 从 pane
  // 头部裁掉、且因 history.rendered 从不回退而永不重渲的那批。两个集合按构造互斥,
  // 相减无条件是错的:受控 A/B 实测,减了之后每补一窗就少记一窗,补两次提示条直接
  // 归零消失,而中间那段断层仍在——正好造出这段注释本想避免的「无标记断层」。
  renderEarlierHint();
  return true;
}

/// 实时裁剪的顶部说明条。与「载入更早的消息」分开是因为语义不同:那条是「数据还在
/// 手上,点一下就补齐」;这条是「本地视图为了保持流畅丢掉了,完整内容在后端对话历史里,
/// 切走再切回会按窗口重建」。做成静态说明而不是按钮——运行中重载对话会把正在写入的
/// pane 整个换掉,不该给一个跑着的时候点了会出事的入口。
export function renderTrimmedHint(pane) {
  const target = pane || activePane;
  if (!target || typeof target.querySelector !== "function") return;
  const dropped = Number(target.dataset.droppedLive || 0);
  const existing = target.querySelector(".pane-trimmed-hint");
  if (dropped <= 0) {
    if (existing) existing.remove();
    return;
  }
  const label = `${t("较早的")} ${dropped} ${t("条已移出视图以保持流畅")}`;
  if (existing) {
    existing.textContent = label;
    return;
  }
  const hint = document.createElement("div");
  hint.className = "pane-trimmed-hint";
  hint.textContent = label;
  target.prepend(hint);
}

/// 顶部提示条:还剩多少条没渲染。它同时是入口(点它补齐)与状态(还剩多少)。
export function renderEarlierHint() {
  const history = paneHistory.get(activeSessionId || "");
  const remaining = history ? history.items.length - history.rendered : 0;
  const existing = activePane.querySelector(".earlier-hint");
  if (remaining <= 0) {
    if (existing) existing.remove();
    return;
  }
  const label = `${t("载入更早的消息")} · ${t("还有")} ${remaining} ${t("条")}`;
  if (existing) {
    existing.textContent = label;
    return;
  }
  const hint = document.createElement("button");
  hint.type = "button";
  hint.className = "earlier-hint";
  hint.textContent = label;
  hint.addEventListener("click", () => loadEarlierMessages());
  activePane.prepend(hint);
}

// 空态标记 = app 图标的 K 几何(竖干 + 右上三条平行记忆层 + 右下一笔行动),
// 与 index.html 里那份首屏静态副本同一份形状——改一处必须改两处,别让它们漂移。
export const EMPTY_STATE_LOGO = '<svg viewBox="0 0 64 64"><g fill="none" stroke="currentColor" stroke-linecap="square">'
  + '<path d="M14 8v48" stroke-width="7"/><path d="M21 33 44 56" stroke-width="7"/>'
  + '<path d="M21.5 31.5 43 8M25.5 35 50 8M29.5 38.5 57 8" stroke-width="3"/></g></svg>';
export function emptyStateMarkup() {
  return `<div class="empty-state"><div class="logo-mark" aria-hidden="true">${EMPTY_STATE_LOGO}</div>`
    + `<div class="hint hint-lead">${t("输入任务开始 · 权限请求会弹窗询问")}</div>`
    + `<div class="hint hint-keys">${t("Ctrl+Enter 发送 · Ctrl/Cmd+P 命令面板 · Ctrl/Cmd+K 聚焦输入 · Ctrl/Cmd+Shift+N 新对话 · Ctrl/Cmd+Shift+C 停止")}</div></div>`;
}

export function renderRecoveredMessages(items) {
  setFollowLatest(true);
  resetPane();
  setCurrentAssistant(null);
  setCurrentReasoning(null);
  setCurrentReasoningHead(null);
  const all = items ?? [];
  paneHistory.set(activeSessionId || "", { items: all, rendered: 0 });
  const tail = all.slice(Math.max(0, all.length - PANE_WINDOW_SIZE));
  paneHistory.get(activeSessionId || "").rendered = tail.length;
  renderMessageParts(tail);
  if (!all.length) {
    resetPane();
    activePane.innerHTML = emptyStateMarkup();
  }
  renderEarlierHint();
  scrollBottom(true);
}

/// 渲染一段消息(配对 tool_call/tool_result、思考块、markdown)。
/// 首屏与向上补齐共用它。
export function renderMessageParts(items) {
  // 调用与结果按 call_id 配对成一块渲染:原先每个 part 各占一行,
  // 结果行只显示原始 call id,对人毫无信息量(用户 2026-08-08 反馈"太丑")。
  const pending = new Map();
  for (const message of items ?? []) {
    for (const part of message.parts ?? []) {
      if (part.type === "tool_call") {
        const block = buildToolBlock(part.name || "tool", part.input);
        appendToPane(block.wrap);
        if (part.id) pending.set(part.id, { block, input: part.input });
        continue;
      }
      if (part.type === "tool_result") {
        const entry = pending.get(part.call_id);
        if (entry) {
          pending.delete(part.call_id);
          fillToolBlock(entry.block, {
            ok: !part.is_error,
            content: part.content,
            input: entry.input,
          });
        } else {
          // 配对不上(历史被压缩过):独立成块,总比丢掉强。
          const orphan = buildToolBlock("tool result", {});
          appendToPane(orphan.wrap);
          fillToolBlock(orphan, { ok: !part.is_error, content: part.content });
        }
        continue;
      }
      if (part.type === "reasoning") {
        // 思考块此前在恢复时被整个丢弃(循环只认 text/tool_*):重开会话后思维链从
        // DOM 消失,复制上下文也拿不到。按实时同款折叠块恢复,完整 raw 进 dataset。
        if (part.text?.trim()) {
          const block = buildReasoningBlock(part.text);
          appendToPane(block.wrap);
          renderReasoningBlock(block.body);
        }
        continue;
      }
      if (part.type !== "text" || !part.text?.trim()) continue;
      const el = addMessage(message.role === "assistant" ? "assistant md" : "user", "");
      if (message.role === "assistant") {
        el.dataset.raw = part.text;
        el.querySelector(".message-body").innerHTML = renderMarkdown(part.text);
      } else {
        el.querySelector(".message-body").textContent = part.text;
      }
    }
  }
  // 没等到结果的调用(轮次被中断,或**窗口边界**把调用与结果切开了):标出来,
  // 不要停在"运行中"的假象上。窗口边界这一侧补齐后会重新配上,不影响最终形态。
  for (const { block } of pending.values()) {
    block.wrap.classList.remove("running");
    block.result.textContent = `⎿ ${t("无结果(轮次中断)")}`;
    block.result.classList.remove("hidden");
  }
}

export async function loadConversation(sequence = null, switchGeneration = null) {
  if (!currentProject) return;
  // 启动时项目列表与历史恢复并行触发,先确保进程列表已选出主会话,再锁定
  // processId。否则首次 conversation_get 可能带着 null,历史会被竞态丢掉。
  // D-355:refreshProcesses 按项目键控(单飞去项目化),这里 await 到的是**当前项目**
  // 自己的列表刷新 Promise——A 的 process_list 在途时切到 B,等到的就是 B 的列表,
  // B 的 activeProcessId 就绪后 conversation_get 才带着 B 的 projectDir/processId 发出。
  if (!activeProcessId && typeof refreshProcesses === "function") await refreshProcesses();
  if (!currentProject || !activeProcessId) return;
  // R-267:D-356 的「快照 + 补齐」整套退役。
  //
  // 原来的做法是切走时存一份 innerHTML、切回时塞回去,再挂一句「快照截至上次切走时,
  // 本轮完成后自动补齐」——因为后台会话的渲染事件被丢弃了,那段确实是缺的。
  // 现在每个会话有自己的 pane 且后台事件直接渲染进去,pane 里已经是**最新**的:
  // 已有内容就直接用,既不重拉也不需要那句免责声明。
  //
  // 仅当 pane 是空的(首次进入该会话,或它被 MESSAGE_PANE_MAX 淘汰过)才往下走
  // 完整装载。这也是淘汰策略敢做的原因:最坏情况退化成改造前的重建,不是缺口。
  // 但这条捷径只对「装载当前会话」成立。指定了 sequence 就是用户在侧栏点了某份
  // **历史快照**——它跟当前 pane 里那段是两回事,直接复用等于什么都没做,而调用方
  // (openConversationForProcess)紧接着还会写一句「已打开历史对话 #N」。用户看到
  // 提示、屏幕没变化,只会以为界面坏了。有 sequence 时一律走下面的真装载。
  if (sequence === null && activeSessionId && showPane(activeSessionId)) {
    scrollBottom(true);
    return;
  }
  // 走真装载:只认领 pane,**不提前清空**。renderRecoveredMessages 拿到结果后自己会
  // resetPane,提前清只在两种坏情况下露头:conversation_get 失败时当前会话内容被抹掉
  // 且错误行又把 hasContent 置回 1(切走切回都不自愈),以及请求在途时主区白屏。
  if (activeSessionId) showPane(activeSessionId);
  // 线程切换是异步的:conversation_get 与 trace_get 之间用户可能再次切线。
  // 两个 IPC 必须锁定同一项目/同一进程,且晚返回的旧请求不能覆盖当前线程。
  const forProject = currentProject;
  const forProcessId = activeProcessId;
  const isCurrent = () =>
    (switchGeneration === null || switchGeneration === processSwitchGeneration) &&
    currentProject === forProject &&
    activeProcessId === forProcessId;
  try {
    bgClear();
    const history = await invoke("conversation_get", {
      projectDir: forProject,
      processId: forProcessId,
      sequence,
    });
    if (!isCurrent()) return;
    renderRecoveredMessages(history);
    const traces = await invoke("conversation_trace_get", {
      projectDir: forProject,
      processId: forProcessId,
      sequence,
    });
    if (!isCurrent()) return;
    renderRecoveredTraces(traces);
    log(`${t("已恢复")} ${history.length} ${t("条")} ${t("历史消息")} ${traces.length} ${t("组工具轨迹")}`);
  } catch (err) {
    addMessage("error", `${t("历史消息恢复失败")}:${err}`);
    toastError(`${t("历史消息恢复失败")}:${err}`, { retry: () => loadConversation(sequence) });
  }
}

// 历史对话按线路归属渲染:后端本来就按 process_id 隔离 session,前端不能再把
// 当前线路的快照扁平化到一个全局列表,否则用户看不出「这段历史属于哪条线」。
// 勾选态必须活在 DOM 之外。侧栏线路列表每 3 秒被 process_list 轮询整体
// replaceChildren 重建一次(09-sessions.js renderParallelTaskStatus),勾选框是
// 每次新建的——只要有任何一条线在跑,用户永远勾不满三条就被抹掉一次,
// 「勾选后点删除」这条唯一的删除路径在运行期间根本走不完。
export const conversationChecked = new Map(); // processId -> Set(JSON.stringify(sequences))
export const conversationItemsByProcess = new Map();
export const conversationErrorsByProcess = new Map();
export let conversationListGeneration = 0;

export function lineHistoryElement(processId) {
  return [...document.querySelectorAll(".parallel-line-history")]
    .find((element) => element.dataset.processId === processId) ?? null;
}

export function historyProcessItem(processId) {
  return processItems.find((item) => item.id === processId) ?? null;
}

export async function openConversationForProcess(processId, sequence) {
  const target = historyProcessItem(processId);
  if (!target) return;
  if (processRunning(target)) {
    toast(t("运行中请先完成或停止当前任务，再打开历史对话"));
    return;
  }
  if (processId !== activeProcessId) await switchProcess(processId);
  await loadConversation(sequence);
  addMessage("notice", `${t("已打开历史对话")} #${sequence}`);
}

export async function deleteConversationsForProcess(processId, sequences) {
  if (!sequences.length) {
    toast(t("先勾选要删除的历史对话"));
    return;
  }
  // D-418:R-245 设计——删除弹窗列清单,取消不产生任何写入；safe 分支另行调用
  // 显式 storage cleanup，失败时保留可重试的清理入口。
  const mode = await confirmDialog({
    title: t("确认删除"),
    message: `${t("将删除勾选的")} ${sequences.length} ${t("份历史对话快照")}${t("此操作不可撤销")}`,
    list: [
      t("会话事件与投影"),
      t("运行轨迹与工具结果"),
      t("草稿与未完成输入"),
      t("引用中的 artifact 保留,无引用 artifact 才可整理"),
    ],
    okText: t("仅删除"),
    safeText: t("删除并安全整理"),
    danger: true,
  });
  if (!mode) return;
  let n;
  try {
    n = await invoke("conversation_delete", { projectDir: currentProject, processId, sequences });
  } catch (err) {
    toastError(String(err), { retry: () => deleteConversationsForProcess(processId, sequences) });
    return;
  }
  if (mode === "safe") {
    const retryCleanup = async () => {
      try {
        const cleanup = await invoke("conversation_cleanup", { projectDir: currentProject });
        const failures = [
          ...(cleanup.artifact_cleanup_errors ?? []),
          ...(cleanup.backup_cleanup_errors ?? []),
        ];
        if (failures.length) {
          toastError(`${t("安全整理部分失败")}\n${failures.join("\n")}`, { retry: retryCleanup });
          return;
        }
        toast(`${t("已删除")} ${n}${t("份对话快照")}; ${t("安全整理释放")} ${cleanup.actual_freed_bytes ?? 0} bytes`);
      } catch (err) {
        toastError(`${t("安全整理失败")}: ${String(err)}`, { retry: retryCleanup });
      }
    };
    await retryCleanup();
  } else {
    toast(`${t("已删除")} ${n}${t("份对话快照")}`);
  }
  conversationChecked.delete(processId);
  await refreshConversationLists();
}

// 历史对话默认收起。每条线路都挂一份完整快照列表,四五条线一起展开时侧栏
// 前两屏全是历史标题,当前在做什么反而被挤下去。展开态按线路记在 localStorage,
// 用户手动展开过的线路下次进来仍是展开的。
export const LINE_HISTORY_OPEN_KEY = "kz-line-history-open";
export const lineHistoryOpen = new Set(readLineHistoryOpen());
export function readLineHistoryOpen() {
  try {
    const raw = JSON.parse(localStorage.getItem(LINE_HISTORY_OPEN_KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((id) => typeof id === "string") : [];
  } catch {
    return [];
  }
}
export function saveLineHistoryOpen() {
  try {
    localStorage.setItem(LINE_HISTORY_OPEN_KEY, JSON.stringify([...lineHistoryOpen]));
  } catch {
    /* localStorage 不可用时仅本次会话生效 */
  }
}

export function renderLineConversationHistory(processId) {
  const el = lineHistoryElement(processId);
  if (!el) return;
  el.replaceChildren();
  const error = conversationErrorsByProcess.get(processId);
  if (error) {
    el.textContent = `${t("历史对话加载失败")}:${error}`;
    return;
  }
  const items = conversationItemsByProcess.get(processId);
  if (!items) {
    el.textContent = t("加载中…");
    return;
  }
  const open = lineHistoryOpen.has(processId);
  el.classList.toggle("open", open);
  const head = document.createElement("button");
  head.type = "button";
  head.className = "parallel-history-head";
  head.setAttribute("aria-expanded", open ? "true" : "false");
  head.title = t("展开或收起该线路的历史对话");
  const caret = document.createElement("span");
  caret.className = "parallel-history-caret";
  caret.setAttribute("aria-hidden", "true");
  caret.textContent = "▸";
  const label = document.createElement("span");
  label.className = "parallel-history-label";
  label.textContent = `${t("历史对话")} (${items.length})`;
  head.append(caret, label);
  head.addEventListener("click", (event) => {
    event.stopPropagation();
    if (lineHistoryOpen.has(processId)) lineHistoryOpen.delete(processId);
    else lineHistoryOpen.add(processId);
    saveLineHistoryOpen();
    renderLineConversationHistory(processId);
  });
  el.appendChild(head);
  if (!open) return;
  const body = document.createElement("div");
  body.className = "parallel-history-body";
  const action = document.createElement("button");
  action.type = "button";
  action.className = "ghost mini parallel-history-delete";
  action.textContent = t("删除");
  action.title = t("删除勾选的对话");
  action.setAttribute("aria-label", t("删除勾选的对话"));
  action.addEventListener("click", (event) => {
    event.stopPropagation();
    const sequences = [...el.querySelectorAll(".parallel-history-check:checked")]
      .flatMap((check) => JSON.parse(check.dataset.seqs));
    void deleteConversationsForProcess(processId, sequences);
  });
  if (!items.length) {
    const empty = document.createElement("div");
    empty.className = "parallel-history-empty";
    empty.textContent = t("暂无历史对话");
    body.appendChild(empty);
    el.appendChild(body);
    return;
  }
  const list = document.createElement("div");
  list.className = "parallel-history-list";
  for (const item of [...items].reverse()) {
    const row = document.createElement("div");
    row.className = "parallel-history-row";
    row.title = t("点击打开 · 勾选后点删除");
    const check = document.createElement("input");
    check.type = "checkbox";
    check.className = "parallel-history-check";
    check.dataset.seqs = JSON.stringify(item.sequences ?? [item.sequence]);
    const checkedSet = conversationChecked.get(processId);
    check.checked = Boolean(checkedSet?.has(check.dataset.seqs));
    check.addEventListener("click", (event) => event.stopPropagation());
    check.addEventListener("change", () => {
      let set = conversationChecked.get(processId);
      if (!set) {
        set = new Set();
        conversationChecked.set(processId, set);
      }
      if (check.checked) set.add(check.dataset.seqs);
      else set.delete(check.dataset.seqs);
    });
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = `${item.title || t("新对话")} (${item.message_count} ${t("条")})`;
    row.append(check, title);
    row.addEventListener("click", () => void openConversationForProcess(processId, item.sequence));
    list.appendChild(row);
  }
  body.append(list, action);
  el.appendChild(body);
}

export async function refreshConversationLists() {
  if (!currentProject) return;
  const forProject = currentProject;
  const generation = ++conversationListGeneration;
  const targets = processItems.slice();
  if (!targets.length) return;
  const results = await Promise.all(targets.map(async (process) => {
    try {
      return { processId: process.id, items: await invoke("conversation_list", { projectDir: forProject, processId: process.id }) };
    } catch (error) {
      return { processId: process.id, error };
    }
  }));
  if (generation !== conversationListGeneration || currentProject !== forProject) return;
  const errors = [];
  for (const result of results) {
    if (result.error) {
      conversationErrorsByProcess.set(result.processId, result.error);
      errors.push(`${result.processId}:${result.error}`);
    } else {
      conversationErrorsByProcess.delete(result.processId);
      conversationItemsByProcess.set(result.processId, result.items ?? []);
    }
    renderLineConversationHistory(result.processId);
  }
  const known = new Set(targets.map((process) => process.id));
  for (const processId of conversationItemsByProcess.keys()) {
    if (!known.has(processId)) conversationItemsByProcess.delete(processId);
  }
  if (errors.length) {
    const message = `${t("历史对话加载失败")}:${errors.join("; ")}`;
    log(message, "warn");
    toastError(message, { retry: refreshConversationLists });
  }
}

export async function refreshConversationList() {
  return refreshConversationLists();
}

// ---------- 新对话 ----------
// R-267:D-356 的 sessionDomCache(切走存 innerHTML 字符串、上限 30 份)整套删除。
// 它存在的唯一理由是「后台会话的渲染事件被丢弃,切回时得有个东西顶上」;现在
// per-session pane 就是活的 DOM,不需要把它序列化成字符串再解析回来——那既是缺口的
// 来源,也是切换卡顿的来源(每次切换一次多 MB 的 innerHTML 解析)。
// `cacheSessionDom` / `dropSessionDomCache` 一并退役,调用点改为无操作或直接删除。

// 会话是否仍处于运行中。pane 淘汰要用它——正在往里写的会话永不淘汰。
// 口径:只有 starting/running/stopping 判活。
export function sessionLiveNow(sessionId) {
  return ["starting", "running", "stopping"].includes(sessionState(sessionId).phase);
}
export function clearChat(noticeText) {
  resetPane();
  setCurrentAssistant(null);
  setCurrentReasoning(null);
  setCurrentReasoningHead(null);
  setCtxTokens(0);
  renderTokens();
  if (noticeText) addMessage("notice", noticeText);
}

defer(() => {
  $("new-chat").addEventListener("click", async () => {
    if (active_space === "research") {
      try { await create_workspace_process(project_workspace().research.topic); }
      catch (error) { toastError(String(error)); }
      return;
    }
    // 鞭挞的轮间等待(runControlPending)后端已收尾、running 为假,但下一轮定时器还挂着:
    // 这个窗口里清空会被随即开跑的那一轮立刻灌满,用户看到的是「点了没用」。它和运行中
    // 一样属于"这条线还没停",一并挡住,并由 setRunning/setRunPending 把按钮真的禁掉——
    // 按钮看着能点、点了只弹一句 toast,正是"要点好几次才生效"的来源。
    if (running || runControlPending) {
      toast(t("任务运行中,先停止再开新对话"));
      return;
    }
    try {
      await invoke("conversation_clear", { projectDir: currentProject, processId: activeProcessId });
      // 开新段是明确的人为介入:armed 的自动续跑必须一起撤掉,否则新段刚建立就被
      // 上一轮排好的续跑写满,新对话名存实亡。
      if (typeof cancelAutoContinueTimer === "function") cancelAutoContinueTimer();
      clearChat(t("已开启新对话(历史保留可审计)"));
      await refreshConversationList();
      log(t("新对话:历史保留,开启新段"));
    } catch (err) {
      toastError(String(err), { retry: () => $("new-chat").click() });
    }
  });
});

// ---------- 对话总结 ----------
defer(() => {
  $("summarize-btn").addEventListener("click", async () => {
    if (!currentProject) {
      toast(t("先选择一个项目"));
      return;
    }
    const transcript = [...messages.querySelectorAll(".msg, .tool-chip")]
      .map((el) => el.textContent.trim())
      .filter(Boolean)
      .join("\n\n")
      .slice(0, 60000);
    if (!transcript) {
      toast(t("当前没有可总结的对话"));
      return;
    }
    $("summarize-btn").disabled = true;
    setStatus(`${t("总结中")}(fast model)`, true);
    log(t("开始总结当前对话…"));
    try {
      const r = await invoke("summarize_chat", { projectDir: currentProject, transcript });
      addSummaryEntry(r.summary, r.path);
      toast(t("小总结已收纳到活动面板"));
      log(`${t("总结完成,已收纳并存档")}:${r.path}`);
    } catch (err) {
      toastError(`${t("总结失败")}:${err}`, { retry: () => $("summarize-btn").click() });
    } finally {
      $("summarize-btn").disabled = false;
      setStatus(running ? t("运行中") : t("空闲"), running);
    }
  });
});

defer(() => {
  for (const [btn, kind] of [["req-open", "req"], ["defect-open", "defect"], ["idea-open", "idea"]]) {
    $(btn).addEventListener("click", () => openDocViewer(kind));
  };
});
