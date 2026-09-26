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
    if (isHidden("#bg-panel")) $("#activity-toggle")?.click();
    await ctx.sleep(80);
  },

  async agents(ctx) {
    await replayRunningTurn(ctx);
    // UI-0926 #8:从主对话里运行中的子代理卡片点 ↗,侧栏进该次委派的详情(指令/过程/结果)。
    const open = $('#messages .sa-card[data-sa-key$="|review_gate"] .sa-open') ?? $("#messages .sa-card .sa-open");
    open?.click();
    await waitFor(() => !isHidden("#agent-panel") && $("#agent-panel")?.dataset.mode === "detail");
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

  // ── 分区:星座背景 ── 设置页「对话背景」分组展开(图案卡片缩略图、上传、滑杆)。
  async backdrop(ctx) {
    await openView(ctx, "settings");
    const group = $("#backdrop-settings");
    if (group && !group.open) group.querySelector("summary")?.click();
    await waitFor(() => group?.open);
    await ctx.sleep(120);
    group?.scrollIntoView({ block: "start" });
  },

  // ── 分区:星座背景 ── 语音布局:走语音控制器自己的状态入口(onState,与真实开麦后同一条布局路径),
  // 不开麦克风、不连语音服务。OC 关时星座放文案右侧,OC 开时进人物身后的 art 槽。
  async voice(ctx) {
    const voice = await import("/23-voice.js");
    voice.voiceConversation?.onState?.("listening");
    await waitFor(() => $("#view-chat")?.classList.contains("voice-mode"));
    await ctx.sleep(160);
    await ctx.settle();
  },

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
      void core.inputDialog({ title: "重命名项目", message: "只改侧栏显示名,不移动目录", value: "kanzei code" });
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

export const SCENE_NAMES = Object.keys(SCENES);

export async function runScene(name, ctx) {
  const scene = SCENES[name] ?? SCENES.chat;
  await scene(ctx);
  const anchor = ctx.params.get("anchor");
  if (anchor) {
    try { document.querySelector(anchor)?.scrollIntoView({ block: "center" }); } catch { /* 选择器非法:忽略 */ }
  }
}
