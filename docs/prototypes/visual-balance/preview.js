// Runs inside the real frontend's isolated preview, not in the review window.
const params = new URLSearchParams(location.search);
const scene = params.get("visualScene"), request = Number(params.get("visualRequest"));
const announce = value => parent.postMessage({ ...value, request }, location.origin);

async function prepare() {
  await window.__kzPreview.readyPromise;
  const shell = await import("/03-shell.js");
  const sessions = await import("/09-sessions.js");
  const fixtures = await window.__kzPreview.fixtures();
  document.body.classList.add("visual-balance");
  document.addEventListener("kz:theme", event => announce({ type: "visual-theme", theme: event.detail.theme }));
  window.addEventListener("message", event => {
    if (event.source === parent && event.origin === location.origin && event.data.type === "visual-set-theme") shell.applyTheme(event.data.theme);
  });
  for (const process of fixtures.state.processes) process.running = false;
  if (shell.activeSessionId) window.__kzPreview.emit("kz:idle", { sessionId: shell.activeSessionId });
  await sessions.refreshProcesses();
  if (scene === "chat") {
    const views = await import("/15-views-misc.js");
    views.renderRecoveredMessages([
      { role: "user", parts: [{ type: "text", text: "对话里的代码和复制按钮整理一下，后台任务也要能继续接收消息。" }] },
      { role: "assistant", parts: [{ type: "text", text: "## 这两处已经整理好了。\n\n代码块保留原始格式，复制操作收在右上角。任务在后台运行时，你可以继续提问，新的消息会接着处理。\n\n### 这次调整\n\n- **代码与正文分开排版**，长命令在代码块内横向滚动。\n- **复制后就地确认**，反馈完成后恢复为安静的图标。\n- **后台任务持续进行**，当前对话仍然可以接收新的问题。\n\n可以用这条命令检查对话界面：\n\n```powershell\nnode scripts/ui-project-conversations-smoke.mjs\n```" }] },
      { role: "assistant", parts: [{ type: "tool_call", id: "visual-check", name: "bash", input: { command: "node scripts/ui-project-conversations-smoke.mjs" } }] },
      { role: "tool", parts: [{ type: "tool_result", call_id: "visual-check", content: "原型示例：检查结束，可以查看记录。", is_error: false }] },
      { role: "assistant", parts: [{ type: "text", text: "后续可以继续补充问题，等这一轮一起确认后再发版。" }] },
    ]);
    const selection = await import("/31-work-selection.js");
    selection.selectWork(shell.currentProject, shell.activeProcessId, "R-364");
    await (await import("/26-project-conversations.js")).refreshConversationWork();
    (await import("/05-chat-render.js")).scrollBottom(true);
  }
  if (scene === "work") {
    await (await import("/12-workbench.js")).openProjectSpace(shell.currentProject, "project");
  } else if (scene === "skills") {
    shell.navigate_view("skills");
  } else if (scene === "empty") {
    const hint = document.createElement("p");
    hint.className = "hint prototype-auto-hint";
    hint.dataset.prototypeOnly = "true";
    hint.hidden = document.getElementById("visual-proposal").disabled;
    hint.textContent = "需要持续推进任务时，可在「更多」中切换为「自主推进」。";
    document.querySelector(".empty-copy>.hint").after(hint);
  }
  const tasksClose = document.getElementById("tasks-close");
  if (tasksClose && tasksClose.getClientRects().length) tasksClose.click();
  document.activeElement?.blur();
  await window.__kzPreview.settle();
  await document.fonts.ready;
  announce({ type: "visual-ready" });
}
prepare().catch(error => announce({ type: "visual-error", message: String(error) }));
