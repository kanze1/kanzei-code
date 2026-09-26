// kanzei UI 预览:场景编排(浏览器端 ESM,由 mock-ipc.js 在启动序列完成后动态加载)。
//
// 每个场景只通过**应用自己的入口**驱动界面:点 rail 按钮、回放 kz:* 事件、调用导出的弹窗函数。
// 不直接改 DOM 结构——截图必须是真实渲染路径的产物,否则预览会替缺陷打掩护。
//
// ctx: { emit, fixtures, params, settle, sleep, scene, theme }

const $ = (selector) => document.querySelector(selector);

async function waitFor(predicate, timeoutMs = 4000) {
  const started = performance.now();
  while (performance.now() - started < timeoutMs) {
    try { if (predicate()) return true; } catch { /* 节点未就绪 */ }
    await new Promise((resolve) => setTimeout(resolve, 30));
  }
  return false;
}

async function openView(ctx, view) {
  const button = $(`.activity-item[data-view="${view}"]`);
  if (!button) return false;
  button.click();
  await waitFor(() => $(`#view-${view}`)?.classList.contains("active"));
  // navigate_view 在 rAF + setTimeout(0) 之后才调 loader,再等 IPC 静默。
  await ctx.sleep(60);
  await ctx.settle();
  return true;
}

function isHidden(selector) {
  const el = $(selector);
  return !el || el.classList.contains("hidden");
}

/// 主会话一轮「正在跑」的现场:状态栏、思考、流式文本、终端工具、勘察子代理(运行中)、模型自派子代理(已完成)。
async function replayRunningTurn(ctx) {
  const { emit, fixtures, sleep } = ctx;
  const e = fixtures.events;
  emit("kz:meta", e.meta);
  emit("kz:turn", e.turn);
  emit("kz:status", e.status);
  emit("kz:reasoning", e.reasoning);
  await sleep(40);
  emit("kz:text", e.text);
  await sleep(40);
  emit("kz:tool-start", e.selfTaskStart);
  for (const progress of e.selfTaskProgress) emit("kz:task-progress", progress);
  emit("kz:tool-end", e.selfTaskEnd);
  emit("kz:tool-start", e.bashStart);
  emit("kz:tool-progress", e.bashProgress);
  emit("kz:tool-start", e.scoutStart);
  for (const progress of e.scoutProgress) emit("kz:task-progress", progress);
  emit("kz:step", e.step);
  await sleep(120);
  await ctx.settle();
}

const SCENES = {
  async chat(ctx) {
    await replayRunningTurn(ctx);
    // UI2-0926 #14:不点任何按钮——子代理开跑后台任务侧栏自动停靠出来(窄窗口只亮徽标)。
    await waitFor(() => !isHidden("#tasks-panel") || !isHidden("#tasks-badge"));
    await ctx.sleep(80);
  },

  async agents(ctx) {
    await replayRunningTurn(ctx);
    // UI-0926 #8:从主对话里运行中的子代理卡片点 ↗,侧栏进该次委派的详情(指令/过程/结果)。
    const open = $('#messages .sa-card[data-sa-key$="|review_gate"] .sa-open') ?? $("#messages .sa-card .sa-open");
    open?.click();
    await waitFor(() => !isHidden("#tasks-panel") && $("#tasks-panel")?.dataset.mode === "detail");
    // 合成点击没有真实指针,程序聚焦「‹ 返回」会被当成键盘焦点弹出提示;截图不要它。
    document.activeElement?.blur?.();
    await ctx.sleep(80);
  },

  /// UI-0926 #8:同一轮并行派发 3 个子代理(一组、每个一行、各自计数),其中一个已完成。
  async parallel(ctx) {
    const { emit, fixtures, sleep } = ctx;
    const e = fixtures.events;
    emit("kz:meta", e.meta);
    emit("kz:turn", e.turn);
    emit("kz:status", e.status);
    emit("kz:text", { ...e.text, text: "分三路并行勘察:调用点、刷新方案、相关测试。" });
    await sleep(40);
    for (const start of e.parallelStarts) emit("kz:tool-start", start);
    for (const progress of e.parallelProgress) emit("kz:task-progress", progress);
    emit("kz:tool-end", e.parallelEnd);
    emit("kz:step", e.step);
    await sleep(120);
    await ctx.settle();
  },

  // ── 分区:后台任务侧栏与可调框 ──
  /// UI2-0926 #14:用户手动打开后台任务侧栏(rail 开关)。宽窗口停靠在右侧;对话列不足 600px(如 --width 1000,
  /// 或 1280 且左侧栏开着)时是抽屉 + 遮罩。
  async "tasks-drawer"(ctx) {
    await replayRunningTurn(ctx);
    if (isHidden("#tasks-panel")) $("#tasks-toggle")?.click();
    await waitFor(() => !isHidden("#tasks-panel"));
    document.activeElement?.blur?.();
    await ctx.sleep(120);
  },

  /// UI2-0926 #14:一批并行委派里有一个失败——侧栏不自动收起,「需要关注」置顶,rail 徽标转红。
  async "tasks-failure"(ctx) {
    const { emit, fixtures, sleep } = ctx;
    const e = fixtures.events;
    emit("kz:meta", e.meta);
    emit("kz:turn", e.turn);
    emit("kz:status", e.status);
    emit("kz:text", { ...e.text, text: "分三路并行勘察:调用点、刷新方案、相关测试。" });
    await sleep(40);
    for (const start of e.parallelStarts) emit("kz:tool-start", start);
    for (const progress of e.parallelProgress) emit("kz:task-progress", progress);
    emit("kz:tool-end", e.parallelEnd);
    emit("kz:tool-end", { ...e.parallelEnd, id: "call_par_a", preview: "12 处调用点 (+12 lines)", content: "12 处调用点", durationMs: 31200 });
    emit("kz:tool-end", {
      ...e.parallelEnd, id: "call_par_b", ok: false, outcome: "failed", code: "subagent_timeout",
      preview: "subagent timed out after the wall-clock safety limit", content: "", durationMs: 300000,
    });
    emit("kz:step", e.step);
    await sleep(120);
    await ctx.settle();
    await waitFor(() => !isHidden("#tasks-panel") || $("#tasks-badge")?.dataset.tone === "err");
    await ctx.sleep(80);
  },

  async settings(ctx) {
    await openView(ctx, "settings");
    await waitFor(() => !isHidden("#settings-effective"));
    $("#settings-effective")?.closest("details")?.scrollIntoView({ block: "start" });
  },

  async docs(ctx) {
    await openView(ctx, "documents");
    const target = ctx.params.get("open") || "R-364";
    await waitFor(() => $(`#documents-req-list .doc-item[data-doc-id="${target}"]`));
    const item = $(`#documents-req-list .doc-item[data-doc-id="${target}"]`);
    if (item && item.querySelector(".doc-detail")?.classList.contains("hidden")) item.querySelector(".doc-row")?.click();
    await ctx.sleep(80);
    // 让展开的条目落在工具栏下方而不是被分组吸顶头盖住:只在它不在视口上半部时才滚。
    const scroller = $("#documents-scroll");
    if (item && scroller) {
      const offset = item.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
      if (offset > scroller.clientHeight * 0.5) scroller.scrollTop += offset - 120;
    }
  },

  async lines(ctx) {
    await openView(ctx, "lines");
  },

  // ── 分区:侧栏与需求页 ──
  /// UI2-0926 #1:项目总览页(rail ⌂)。卡片数据来自 workspace_snapshot 夹具(三个项目,形状对照后端)。
  async workspace(ctx) {
    await openView(ctx, "workspace");
    await waitFor(() => document.querySelectorAll("#workspace-projects .workspace-card").length > 0);
    document.activeElement?.blur?.();
    await ctx.sleep(60);
  },
  /// UI2-0926 #1:侧栏项目卡展开的项目菜单(各项目 ✓ 当前 / 打开文件夹… / 新建项目… / 项目总览)。
  async projects(ctx) {
    $("#project-switch")?.click();
    await waitFor(() => document.querySelector(".project-menu"));
    // 合成点击没有真实指针,程序聚焦的菜单项会带键盘焦点环;截图要的是鼠标打开的样子。
    document.activeElement?.blur?.();
    await ctx.sleep(80);
  },
  /// UI2-0926 侧栏密度:侧栏滚到底——各线当前在做(未取得条目的线一行)、待办计数、隔离工作树(一行一棵、最多 6 棵)。
  async sidebar(ctx) {
    const sidebar = $("#sidebar");
    if (sidebar) sidebar.scrollTop = sidebar.scrollHeight;
    await ctx.sleep(60);
  },
  /// UI2-0926 #5:需求页列表本身(不展开详情),看行的字阶与明暗层级。docs 场景展开了 R-364 的详情,
  /// 详情占满一屏,列表行反而看不全。
  async doclist(ctx) {
    await openView(ctx, "documents");
    await waitFor(() => document.querySelector("#documents-req-list .doc-item[data-doc-id]"));
    document.activeElement?.blur?.();
    await ctx.sleep(60);
  },
  // ── 分区:侧栏与需求页(完) ──

  async empty(ctx) {
    // 走真实的「新对话」入口:它同时是缺陷 #2(旧内容残留)的复现路径。
    $("#new-chat")?.click();
    await ctx.sleep(120);
    await ctx.settle();
  },

  async overlays(ctx) {
    const dialog = ctx.params.get("dialog") || "ask";
    if (dialog === "ask" || dialog === "question") {
      // 待答队列来自 pending_asks_get(夹具按 dialog 返回),启动时 refreshPendingAsks 已弹出。
      await waitFor(() => !isHidden("#ask-overlay"));
      return;
    }
    if (dialog === "confirm") {
      const core = await import("/01-core.js");
      const { t } = await import("/02-i18n.js");
      void core.confirmDialog({
        title: t("确认删除"),
        message: `${t("将删除勾选的")} 2 ${t("份历史对话快照")}${t("此操作不可撤销")}`,
        list: [t("会话事件与投影"), t("运行轨迹与工具结果"), t("草稿与未完成输入"), t("引用中的 artifact 保留,无引用 artifact 才可整理")],
        okText: t("仅删除"),
        safeText: t("删除并安全整理"),
        danger: true,
      });
      await waitFor(() => !isHidden("#confirm-overlay"));
      return;
    }
    if (dialog === "input") {
      const core = await import("/01-core.js");
      void core.inputDialog({ title: "重命名项目", message: "只改显示名,不移动目录", value: "kanzei code" });
      await waitFor(() => !isHidden("#input-overlay"));
      return;
    }
    if (dialog === "viewer") {
      const views = await import("/15-views-misc.js");
      views.openRuntimeMarkdown("R-364 发现记录", [
        "## 发现记录",
        "",
        "```json",
        JSON.stringify(JSON.parse(ctx.fixtures.state.docs.requirements[0].fields.find(([key]) => key === "发现记录")[1]), null, 2),
        "```",
        "",
        "| 字段 | 值 |",
        "|---|---|",
        "| 复杂度 | 大 |",
        "| 批次 | 0/4 |",
      ].join("\n"));
      await waitFor(() => !isHidden("#viewer-overlay"));
      return;
    }
    if (dialog === "palette") {
      const palette = await import("/21-palette.js");
      palette.openPalette();
      await ctx.sleep(80);
      return;
    }
    if (dialog === "toast") {
      const shell = await import("/03-shell.js");
      shell.toast("已复制到剪贴板");
      await ctx.sleep(80);
    }
  },
};

// ── 分区:对话单列与输入区 ──
// UI2-0926 #11 #12:复现用户截图 13 的现场(对话列对齐、工具组、⎿ 行内代码、本轮结束、用户气泡、输入区)。
// 仍只走应用入口:回放 kz:* 事件、在输入框里打字点发送(预览的 run_prompt 回放一轮假回复)、点 rail 开关。
async function closeActivityPanel(ctx) {
  if (!isHidden("#bg-panel")) $("#activity-toggle")?.click();
  await ctx.sleep(60);
}
/// 一轮已结束的回合:正文(列表 + 行内代码)、两次成功的条目/工作队列调用、一次带反引号的失败调用、本轮结束。
async function replayFinishedTurn(ctx) {
  const { emit, fixtures, sleep } = ctx;
  const e = fixtures.events;
  const sessionId = e.meta.sessionId;
  emit("kz:meta", e.meta);
  emit("kz:turn", { sessionId, step: 1, maxSteps: 0 });
  emit("kz:reasoning", e.reasoning);
  emit("kz:tool-start", { sessionId, id: "col-req-1", name: "req", summary: "update R-001", input: { action: "update", id: "R-001" } });
  emit("kz:tool-end", { sessionId, id: "col-req-1", name: "req", ok: true, outcome: "success", preview: "updated: R-001", content: "updated: R-001 (发现记录, 来源, 确认记录)", contentBytes: 40, contentTruncated: false, durationMs: 300 });
  emit("kz:tool-start", { sessionId, id: "col-work-1", name: "work", summary: "claim", input: { action: "claim", id: "R-001" } });
  emit("kz:tool-end", { sessionId, id: "col-work-1", name: "work", ok: true, outcome: "success", preview: "claimed R-001", content: "claimed R-001 · 移动端多模态 Markdown 上下文库与 API Agent", contentBytes: 60, contentTruncated: false, durationMs: 200 });
  await sleep(30);
  emit("kz:text", { sessionId, text: "已更新 **R-001「移动端多模态 Markdown 上下文库与 API Agent」**,并记下你刚确认的范围:\n\n- PDF、图片保留原件,作为本地附件由 Markdown 关联;尽可能提取 PDF 文字和图片 OCR 内容用于搜索,解析失败时仍可访问原件。\n- Agent 通过 API 对**你选定的内容**进行基础问答/检索,不默认上传全库;优先保持轻便、快速。\n- 当前工作队列仍选中 R-001,需求保持 `todo`,没有开始实现。" });
  await sleep(30);
  emit("kz:tool-start", { sessionId, id: "col-req-2", name: "req", summary: "update R-001", input: { action: "update", id: "R-001" } });
  const bad = "批次字段要写成 `k/N`(如 `0/3`),实际收到 `批次: 0/5`;B1 核心模型、快速捕获与多模态附件";
  emit("kz:tool-end", { sessionId, id: "col-req-2", name: "req", ok: false, outcome: "failed", preview: bad, content: `${bad}\n请只写进度计数`, contentBytes: 120, contentTruncated: false, durationMs: 100 });
  emit("kz:step", e.step);
  emit("kz:done", { sessionId, steps: 6, halted: false, history: 23, elapsedMs: 9400, input: 148200, output: 2310, cacheRead: 131000, cacheWrite: 0, tools: { req: 2, work: 1 }, autoAction: { type: "NoContinue" } });
  emit("kz:idle", { sessionId, reason: "completed" });
  await sleep(60);
  await ctx.settle();
}
Object.assign(SCENES, {
  /// 空闲态的对话列:历史 + 一轮已结束的回合 + 用户发一条消息后的假回复(预览 run_prompt 回放)。
  async column(ctx) {
    await replayFinishedTurn(ctx);
    await closeActivityPanel(ctx);
    const prompt = $("#prompt");
    if (prompt && $("#send")) {
      prompt.value = "API 暂时只适配 deepseek 的就行,深度适配然后就可以开始了,直到交付";
      prompt.dispatchEvent(new Event("input", { bubbles: true }));
      $("#send").click();
      // 假回复约 1.8s 回放完(run_prompt 每步 140ms),等到 kz:idle 落地。
      await waitFor(() => document.documentElement.dataset.kzActivity === "idle" && $('.msg-pane[data-active="1"]')?.querySelectorAll(".msg.notice").length >= 2, 6000);
    }
    // 预览没有系统通知权限,轮末提示(异步)会把运行日志面板顶出来;截图只要对话列,等它出来再经状态栏开关收起。
    await waitFor(() => !isHidden("#log-panel"), 800);
    if (!isHidden("#log-panel")) $("#log-toggle")?.click();
    await ctx.settle();
    const chat = await import("/05-chat-render.js");
    chat.scrollBottom(true);
    await ctx.sleep(60);
    document.activeElement?.blur?.();
  },
  /// 运行态的输入区(用户截图 13 的底部):一轮正在跑、活动面板关、自主推进、鞭挞开着。
  async composer(ctx) {
    await replayRunningTurn(ctx);
    await closeActivityPanel(ctx);
    const mode = $("#profile-select");
    if (mode && mode.value !== "dev-auto") {
      mode.value = "dev-auto";
      mode.dispatchEvent(new Event("change", { bubbles: true }));
    }
    const whip = $("#auto-continue");
    if (whip && !whip.checked) whip.click();
    await ctx.sleep(80);
    document.activeElement?.blur?.();
  },
});

export const SCENE_NAMES = Object.keys(SCENES);

export async function runScene(name, ctx) {
  const scene = SCENES[name] ?? SCENES.chat;
  await scene(ctx);
  const anchor = ctx.params.get("anchor");
  if (anchor) {
    try { document.querySelector(anchor)?.scrollIntoView({ block: "center" }); } catch { /* 选择器非法:忽略 */ }
  }
}
